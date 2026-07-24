#!/usr/bin/env node
// E2E survive-restart harness (Task 23 / spec §14.1 + §13 survival truth table).
//
// Proves the S1 core promise entirely at the daemon layer, over the real Hop-B wire
// protocol (spec §7), without driving the WKWebView / Tauri GUI:
//
//   create a terminal -> run a command -> observe OSC-driven status ->
//   "quit the app" (hard-close the client socket) -> daemon + shell SURVIVE ->
//   "relaunch" (fresh client) -> reattach -> scrollback intact.
//
// Exit code 0 = full pass. Any failed assertion throws, is logged with a diagnostic
// message, and the process exits non-zero. No phase is skipped or weakened to pass
// vacuously: a missing daemon binary, broken handshake, absent lifecycle pushes, or
// lost scrollback each fail loudly with a specific, actionable message.
//
// Run: `npm run e2e:survive` (see tests/e2e/README.md for prerequisites + the
// launchd-managed and full-GUI variants).

import assert from "node:assert/strict";
import fs from "node:fs";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import { setTimeout as sleep } from "node:timers/promises";
import {
  connect,
  request,
  nextPush,
  resolveSocketPath,
  pgrepDaemon,
  pgrepShell,
  pidAlive,
  spawnDaemon,
  killProcessGroup,
  preambleHandshake,
  cborEncode,
  cborDecode,
} from "./lib/daemon-harness.mjs";

const REPO = path.resolve(import.meta.dirname, "..", "..");
const DAEMON_BIN = process.env.BPA_SESSIOND ?? path.join(REPO, "target", "debug", "bpa-sessiond");
// When true (the launchd-managed variant, see README §2), the harness does not spawn its
// own daemon and does not send SIGTERM to it during cleanup — launchd owns the lifecycle.
const EXTERNAL_DAEMON = process.env.BPA_E2E_EXTERNAL_DAEMON === "1";

// Name of the workspace phase1 creates. It is deliberately NOT a bare "e2e": under
// `BPA_E2E_EXTERNAL_DAEMON=1` this row is written into the REAL user database
// (`~/Library/Application Support/ai.builderpro.desktop/bpa.db`) and shows up in the app's
// sidebar, so if teardown ever fails to remove it again the leftover must be immediately
// attributable to a specific harness run rather than looking like a workspace the user made
// themselves. `<pid>` identifies the run, the millisecond stamp keeps two runs (or a reused pid)
// distinct. History: a `name = "e2e"` era of this harness left 12 such rows behind on a real
// machine, all pointing at long-deleted `target/e2e-ws-*` temp dirs.
const WORKSPACE_NAME = `e2e-${process.pid}-${Date.now()}`;

// ---- state isolation (never touch the real user socket/DB at /tmp/bpa-<uid>, $XDG_RUNTIME_DIR/bpa,
// or ~/Library/Application Support/ai.builderpro.desktop) ----
//
// The harness used to call `resolveSocketPath()` unconditionally, which resolves against this
// process's REAL `XDG_RUNTIME_DIR` (or `/tmp/bpa-<uid>`) — the same path a real, user-facing
// daemon binds. Spawning a throwaway test daemon there risks colliding with (or, on cleanup,
// SIGTERM-ing) an actual running daemon, and a failed run was observed to leak a stray
// `bpa-sessiond` bound to that real path. For the harness-spawned variant we instead mint a fresh
// `mkdtemp` directory and point `XDG_RUNTIME_DIR` at it for BOTH this process and the daemon child
// (`spawnDaemon`'s `envOverrides`), so `resolve_socket_path()`/`resolve_lockfile()` on the daemon
// side (`crates/sessiond/src/singleton.rs`, which derives both from `XDG_RUNTIME_DIR`) and
// `resolveSocketPath()` here agree on an isolated `<tmp>/bpa/d.sock` that cannot collide with any
// real daemon on this machine.
//
// The daemon's DURABLE state (the SQLite DB + logs) is a SEPARATE resolution path
// (`crates/sessiond/src/boot.rs::app_support_dir()` / `main.rs::init_tracing()`), keyed off `HOME`
// — `~/Library/Application Support/ai.builderpro.desktop/bpa.db` — NOT `XDG_RUNTIME_DIR`. Phase 5
// (daemon-restart rehydration) needs the relaunched daemon to open the exact same `bpa.db` the
// pre-restart daemon wrote to, so `HOME` is isolated here too via a scratch `<tmp>/home` dir passed
// through the SAME `envOverrides` object every `spawnDaemon` call in this file uses — one isolated
// env, reused verbatim across phase 0's initial spawn and phase 5's relaunch, guaranteeing both
// runs resolve to the identical `bpa.db` path without hardcoding or re-deriving it here.
// The launchd-managed variant (`EXTERNAL_DAEMON`) deliberately keeps using the real, ambient
// `XDG_RUNTIME_DIR`/`HOME` — it is attaching to an already-running, launchd-supervised daemon at
// the real path, by design (see README §2).
let isolatedTmpDir = null;
let daemonEnvOverrides = {};
if (!EXTERNAL_DAEMON) {
  isolatedTmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "bpa-e2e-"));
  process.env.XDG_RUNTIME_DIR = isolatedTmpDir;
  const isolatedHomeDir = fs.mkdtempSync(path.join(os.tmpdir(), "bpa-e2e-home-"));
  daemonEnvOverrides = { XDG_RUNTIME_DIR: isolatedTmpDir, HOME: isolatedHomeDir };
}
const SOCK = resolveSocketPath();

function log(msg) {
  console.log(`[e2e] ${msg}`);
}

/** Best-effort binary sanity check: refuse to run against the known S0 placeholder stub
 * (a `sh` script that always exits 1) rather than failing confusingly at connect-time. */
function assertRealBinary(binPath) {
  assert.ok(
    fs.existsSync(binPath),
    `daemon binary missing at ${binPath} (build with: cargo build -p bpa-sessiond --bin bpa-sessiond)`,
  );
  const head = Buffer.alloc(64);
  const fd = fs.openSync(binPath, "r");
  const n = fs.readSync(fd, head, 0, 64, 0);
  fs.closeSync(fd);
  const text = head.subarray(0, n).toString("utf8");
  assert.ok(
    !text.startsWith("#!/bin/sh") && !text.startsWith("#!/usr/bin/env sh"),
    `daemon binary at ${binPath} is the S0 scaffold placeholder shell script, not a real ` +
      `build — run: cargo build -p bpa-sessiond --bin bpa-sessiond (see tests/e2e/README.md)`,
  );
}

/**
 * Cleanup state tracked across phases so the top-level `finally` in `main()` can tear everything
 * down on ANY exit path — success, a failed assertion mid-phase, or a request/push timeout — not
 * just the happy path. Nothing here is optional: a daemon leaked mid-test (the failure mode this
 * task started from — `pgrep -fl bpa-sessiond` found a stray process after an earlier failed run)
 * must never survive the harness process exiting, regardless of why it exited.
 *
 * The same rule applies to DAEMON-SIDE rows, not just OS processes and temp dirs: `workspaceId` /
 * `sessionIds` below record every entity this run asked the daemon to create, so `cleanupAll()`
 * can delete them again over the wire. Under `BPA_E2E_EXTERNAL_DAEMON=1` those rows land in the
 * REAL user database (README §2 — attaching to the launchd-supervised daemon on the real socket
 * with the ambient `HOME` is intentional; leaving droppings in the user's sidebar is not).
 */
const cleanup = {
  daemonPid: null,
  conns: [],
  workspaceRoot: null,
  /** Workspace id returned by phase1's `CreateWorkspace`, cleared once it is removed again. */
  workspaceId: null,
  /** Every session id this run created (phase1), cleared per-id as each is killed. */
  sessionIds: [],
};

async function main() {
  // ---- phase 0: spawn (or attach to) the daemon and complete the v2 preamble handshake ----
  let daemonProc = null;
  if (EXTERNAL_DAEMON) {
    log(`phase0: attaching to externally-managed daemon at socket ${SOCK}`);
  } else {
    assertRealBinary(DAEMON_BIN);
    log(`phase0: spawn daemon ${DAEMON_BIN} (isolated XDG_RUNTIME_DIR=${isolatedTmpDir}, HOME=${daemonEnvOverrides.HOME})`);
    // envOverrides pins the CHILD's XDG_RUNTIME_DIR + HOME too (belt-and-suspenders alongside
    // setting XDG_RUNTIME_DIR on `process.env` above): the daemon's own lockfile/log-dir/DB
    // resolution must never fall back to the real user path even if something upstream of
    // `spawn()` stripped an inherited var.
    daemonProc = spawnDaemon(DAEMON_BIN, SOCK, daemonEnvOverrides);
    cleanup.daemonPid = daemonProc.pid;
  }

  let conn;
  for (let i = 0; i < 50; i++) {
    try {
      conn = await connect(SOCK);
      break;
    } catch {
      await sleep(100);
    }
  }
  assert.ok(conn, `could not connect to daemon socket at ${SOCK} within 5s`);
  cleanup.conns.push(conn);
  // `connect()` itself performs the v3 preamble handshake (magic/version negotiation) before
  // resolving — see `preambleHandshake()` in daemon-harness.mjs. A bad magic, an Incompatible
  // result, or an unexpected chosen version throws loudly from inside `connect()` itself, so
  // reaching here already proves the handshake succeeded; assert the negotiated version explicitly
  // too, matching the old harness's self-checking `Welcome` roundtrip assertion style.
  assert.equal(conn.chosenVersion, 3, `preamble negotiated unexpected version: ${JSON.stringify(conn)}`);
  log(`phase0 OK: preamble handshake (chosen=${conn.chosenVersion}, daemonBuild=${JSON.stringify(conn.daemonBuild)})`);

  // ---- phase 1: create a workspace + session rooted in a temp dir ----
  log("phase1: create workspace + session");
  const root = fs.mkdtempSync(path.join(REPO, "target", "e2e-ws-"));
  cleanup.workspaceRoot = root;
  const ws = await request(conn, { t: "CreateWorkspace", name: WORKSPACE_NAME, rootPath: root });
  assert.equal(ws.t, "Workspace", `CreateWorkspace -> ${JSON.stringify(ws)}`);
  // Record it for teardown BEFORE anything else can throw: from this line on, `cleanupAll()` is
  // responsible for removing this workspace from the daemon on EVERY exit path.
  cleanup.workspaceId = ws.value.id;
  const created = await request(conn, {
    t: "CreateSession",
    workspaceId: ws.value.id,
    shell: "/bin/zsh",
    cwd: root,
    envOverrides: [],
    cols: 80,
    rows: 24,
  });
  assert.equal(created.t, "Session", `CreateSession -> ${JSON.stringify(created)}`);
  const sid = created.value.id;
  cleanup.sessionIds.push(sid);
  log(`phase1 OK: workspace ${ws.value.id} ("${WORKSPACE_NAME}"), session ${sid}`);

  // ---- phase 2: attach, run a marker command, capture output ----
  log("phase2: attach + run command");
  await request(conn, { t: "AttachSession", sessionId: sid });
  const replay = await nextPush(conn, (p) => p.t === "Replay" && p.sessionId === sid);
  assert.equal(replay.t, "Replay", "expected Replay first on attach");

  const MARKER = `E2E_MARK_${Date.now()}`;
  await request(conn, { t: "WriteStdin", sessionId: sid, bytes: Buffer.from(`echo ${MARKER}\n`, "utf8") });
  let acc = "";
  const outputDeadline = Date.now() + 15000;
  while (!acc.includes(MARKER) && Date.now() < outputDeadline) {
    const out = await nextPush(conn, (p) => p.t === "Output" && p.sessionId === sid);
    acc += Buffer.from(out.bytes).toString("utf8");
  }
  assert.ok(acc.includes(MARKER), `marker ${MARKER} not seen in Output within 15s (got: ${JSON.stringify(acc)})`);
  log("phase2 OK: command output observed");

  // ---- phase 3: observe OSC-133-driven lifecycle transitions (running -> atPrompt) ----
  log("phase3: observe OSC status");
  await request(conn, { t: "WriteStdin", sessionId: sid, bytes: Buffer.from("sleep 1\n", "utf8") });
  const running = await nextPush(
    conn,
    (p) => p.t === "StateChanged" && p.sessionId === sid && p.lifecycle.kind === "running",
  );
  assert.equal(running.lifecycle.kind, "running", "expected running lifecycle after sleep");
  const backToPrompt = await nextPush(
    conn,
    (p) => p.t === "StateChanged" && p.sessionId === sid && p.lifecycle.kind === "atPrompt",
  );
  assert.equal(backToPrompt.lifecycle.kind, "atPrompt", "expected atPrompt after command finished");
  log("phase3 OK: OSC-133 lifecycle running -> atPrompt");

  // ---- phase 4: survive-restart core ----
  // Quit the client, prove the daemon + shell child both survive, then a fresh client
  // reattaches and replays scrollback containing the pre-quit marker.
  log("phase4: quit client, assert daemon + shell survive, reattach, replay");

  // Use the PID this harness itself spawned (`cleanup.daemonPid`) rather than re-discovering it via
  // `pgrep -x bpa-sessiond` by name — that name match is not scoped to this test and would be wrong
  // (or ambiguous) if another `bpa-sessiond` happens to be running on the same machine. `pgrepDaemon`
  // is still used below purely as the liveness probe spec §14.1 asks for ("assert via pgrep/kill -0").
  // In EXTERNAL_DAEMON mode this harness never spawned the daemon (launchd owns it), so
  // `cleanup.daemonPid` is null and the assertion below used to fail on `pidAlive(null)` — the
  // variant could not get past phase 4 at all. That is almost certainly how the leaked "e2e"
  // workspaces happened: phase 1 created one, phase 4 died, and the old teardown removed nothing.
  // For that mode `pgrep` is the ONLY way to find the pid, so use it — but demand it be
  // unambiguous, because acting on the wrong daemon is worse than not running the phase.
  let resolvedDaemonPid = cleanup.daemonPid;
  if (resolvedDaemonPid === null) {
    const candidates = pgrepDaemon();
    assert.strictEqual(
      candidates.length,
      1,
      `EXTERNAL_DAEMON: expected exactly one running bpa-sessiond to attach to, found ${candidates.length} (${candidates.join(", ") || "none"})`,
    );
    resolvedDaemonPid = Number(candidates[0]);
    log(`phase4: attached daemon pid ${resolvedDaemonPid} (discovered via pgrep — launchd-owned)`);
  }
  assert.ok(pidAlive(resolvedDaemonPid), `daemon (pid ${resolvedDaemonPid}) not running before client quit`);
  const daemonPid = resolvedDaemonPid;

  const childPids = pgrepShell(daemonPid).map(Number);
  assert.ok(childPids.length >= 1, `no shell child process found under daemon pid ${daemonPid}`);
  const shellPid = childPids[0];

  // Simulate the GUI quitting: hard-close the client socket WITHOUT DetachSession or
  // KillSession. This is the exact failure mode a Cmd-Q / force-quit produces — the
  // daemon must treat it as "client gone", not "session should die".
  conn.sock.destroy();
  await sleep(1500);

  assert.ok(
    pgrepDaemon().includes(String(daemonPid)),
    `daemon (pid ${daemonPid}) died when the client disconnected — it MUST survive a client quit`,
  );
  assert.ok(
    pidAlive(shellPid),
    `shell child (pid ${shellPid}) died when the client disconnected — it MUST survive a client quit`,
  );
  log("phase4a OK: daemon + shell survived client quit");

  // Relaunch: a brand-new client connects (performing its own fresh preamble handshake inside
  // `connect()`), lists sessions, reattaches, and expects the scrollback replay to still contain
  // the marker written before the "quit".
  const conn2 = await connect(SOCK);
  cleanup.conns.push(conn2);
  assert.equal(conn2.chosenVersion, 3, `reattach handshake negotiated unexpected version: ${JSON.stringify(conn2)}`);

  const sessions = await request(conn2, { t: "ListSessions" });
  assert.equal(sessions.t, "Sessions", `ListSessions failed after relaunch: ${JSON.stringify(sessions)}`);
  assert.ok(
    sessions.value.some((s) => s.id === sid),
    `session ${sid} lost across client restart (ListSessions returned: ${JSON.stringify(sessions.value.map((s) => s.id))})`,
  );

  await request(conn2, { t: "AttachSession", sessionId: sid });
  const replay2 = await nextPush(conn2, (p) => p.t === "Replay" && p.sessionId === sid);
  const replayText = Buffer.from(replay2.content).toString("utf8");
  assert.ok(
    replayText.includes(MARKER),
    `scrollback replay missing marker ${MARKER} after reattach (survive-restart property violated)`,
  );
  log("phase4b OK: reattach + scrollback intact");

  if (EXTERNAL_DAEMON) {
    // Under the launchd-managed variant we own only the workspace + session we created, not the
    // daemon lifecycle. Both are torn down by `cleanupAll()`'s `removeDaemonSideStateBestEffort()`
    // in the top-level `finally` — deliberately NOT here on the happy path only, because those rows
    // live in the user's REAL `bpa.db` and must also be removed when a phase above threw. The shared
    // `cleanupAll()` still closes our connections but must NOT touch the daemon process itself in
    // this variant (see the `EXTERNAL_DAEMON` guard there), and phase 5 below (a real DaemonShutdown
    // + process-level relaunch) is skipped entirely — under this variant the daemon lifecycle
    // belongs to launchd (README §2's `KeepAlive{Crashed}` is the mechanism that tracks; this
    // harness deliberately doesn't drain/replace a daemon it doesn't own).
    log("ALL PHASES PASSED (phase5 skipped: BPA_E2E_EXTERNAL_DAEMON=1)");
    return;
  }

  // ---- phase 5: daemon-restart rehydration (closes BL-7, Pv2 §9.8) ----
  // Drain-shutdown the daemon over the wire, wait for the OS process to actually exit, relaunch the
  // SAME binary against the SAME state dir (isolated XDG_RUNTIME_DIR + HOME from phase 0/4, reused
  // verbatim via `daemonEnvOverrides`), reconnect, and assert the phase-1 session rehydrates as
  // present-but-inactive with its phase-2 scrollback marker intact — proving persistence survived a
  // real daemon process restart, not just a client reconnect (phase 4 above only killed the CLIENT
  // side; the daemon process itself never stopped).
  log("phase5: DaemonShutdown{drain:true} -> wait for exit -> relaunch same state dir -> rehydrate");

  const shutdownAck = await request(conn2, { t: "DaemonShutdown", drain: true });
  assert.equal(shutdownAck.t, "Ack", `DaemonShutdown{drain:true} -> ${JSON.stringify(shutdownAck)}`);
  log("phase5 OK: DaemonShutdown Ack received");

  // The Ack is enqueued before the shared shutdown watch flips (socket_server.rs's ordering
  // guarantee — see the comment on the Rust `Request::DaemonShutdown` dispatch arm), so by the time
  // we've read the Ack the daemon is already draining/exiting. Poll `kill -0` on the exact pid this
  // harness spawned (never a name-based `pgrep`, for the same disambiguation reason phase4 uses
  // `cleanup.daemonPid`) until it's gone, bounded so a daemon that fails to exit fails the phase
  // loudly instead of hanging the suite forever.
  const exitDeadline = Date.now() + 10000;
  while (pidAlive(daemonPid) && Date.now() < exitDeadline) {
    await sleep(100);
  }
  assert.ok(
    !pidAlive(daemonPid),
    `daemon (pid ${daemonPid}) did not exit within 10s of DaemonShutdown{drain:true} — graceful exit failed`,
  );
  log(`phase5 OK: daemon (pid ${daemonPid}) process exited`);

  // Also drop this harness's own two live connections to the now-dead daemon before relaunching —
  // both sockets are already EOF/reset from the daemon's own teardown, but destroying them
  // explicitly avoids any stale entry in `cleanup.conns` racing the fresh connections below.
  for (const c of [conn, conn2]) {
    try {
      c.sock.destroy();
    } catch {
      /* already closed */
    }
  }
  cleanup.conns = [];

  // Relaunch the SAME daemon binary, bound to the SAME socket path, with the SAME env overrides
  // (`daemonEnvOverrides` — the identical `XDG_RUNTIME_DIR`/`HOME` object phase 0 used) so
  // `resolve_socket_path()` and `app_support_dir()` on the daemon side resolve to the exact same
  // paths as before — same `d.sock`, and critically the same `bpa.db` SQLite file the pre-restart
  // daemon's `DaemonShutdown{drain:true}` flush persisted the session + scrollback into.
  log(`phase5: relaunching ${DAEMON_BIN} against the same state dir`);
  const daemonProc2 = spawnDaemon(DAEMON_BIN, SOCK, daemonEnvOverrides);
  cleanup.daemonPid = daemonProc2.pid;

  let conn3;
  for (let i = 0; i < 50; i++) {
    try {
      conn3 = await connect(SOCK);
      break;
    } catch {
      await sleep(100);
    }
  }
  assert.ok(conn3, `could not reconnect to relaunched daemon socket at ${SOCK} within 5s`);
  cleanup.conns.push(conn3);
  assert.equal(
    conn3.chosenVersion,
    3,
    `post-relaunch preamble handshake negotiated unexpected version: ${JSON.stringify(conn3)}`,
  );
  log(`phase5 OK: reconnected to relaunched daemon (pid ${cleanup.daemonPid})`);

  // Assertion 1: the phase-1 session `sid` reappears in ListSessions, and is rehydrated INACTIVE
  // (`isActive === false`) — its PTY died along with the old daemon process, so a fresh daemon
  // reading it back out of SQLite must report it as inactive rather than fabricating liveness.
  const rehydratedSessions = await request(conn3, { t: "ListSessions" });
  assert.equal(
    rehydratedSessions.t,
    "Sessions",
    `ListSessions after daemon relaunch -> ${JSON.stringify(rehydratedSessions)}`,
  );
  const rehydrated = rehydratedSessions.value.find((s) => s.id === sid);
  assert.ok(
    rehydrated,
    `session ${sid} did NOT reappear after daemon restart (ListSessions returned: ` +
      `${JSON.stringify(rehydratedSessions.value.map((s) => s.id))}) — rehydration from SQLite failed`,
  );
  assert.equal(
    rehydrated.isActive,
    false,
    `session ${sid} reappeared but isActive=${rehydrated.isActive} (expected false: rehydrated ` +
      `sessions must be inactive — their PTY died with the old daemon process)`,
  );
  assert.equal(rehydrated.workspaceId, ws.value.id, `rehydrated session ${sid} lost its workspaceId`);
  log(`phase5 OK: session ${sid} rehydrated with isActive=false`);

  // Assertion 2: reattaching still replays scrollback containing the phase-1/2 MARKER — the
  // pre-restart `DaemonShutdown{drain:true}` flush persisted it to SQLite, and the fresh daemon's
  // cold-rehydrate path (boot loads every persisted session into the Supervisor as an inactive
  // replay-only entry; `AttachSession` on it Acks and sends `Push::Replay` with the persisted
  // scrollback) must load it back rather than starting the session's scrollback from empty.
  const attachResp = await request(conn3, { t: "AttachSession", sessionId: sid });
  assert.equal(
    attachResp.t,
    "Ack",
    `AttachSession on rehydrated session ${sid} -> ${JSON.stringify(attachResp)} (expected Ack + ` +
      `Replay push: attach-on-inactive must serve the persisted scrollback after a daemon restart)`,
  );
  const replay3 = await nextPush(conn3, (p) => p.t === "Replay" && p.sessionId === sid);
  const replayText3 = Buffer.from(replay3.content).toString("utf8");
  assert.ok(
    replayText3.includes(MARKER),
    `scrollback replay missing marker ${MARKER} after daemon restart (BL-7 rehydration property ` +
      `violated — got: ${JSON.stringify(replayText3)})`,
  );
  log("phase5 OK: reattach after daemon restart replays scrollback with marker intact (BL-7 closed)");

  log("ALL PHASES PASSED");
}

// ============================================================================
// Daemon-side teardown of the rows THIS run created (workspace + sessions).
//
// Why this does not simply reuse `connect()`/`request()` from `lib/daemon-harness.mjs`:
//
//  1. `RemoveWorkspace { workspace_id }` is a NEW protocol verb landing in parallel with this
//     change. `daemon-harness.mjs`'s `encodeRequest()` switch has a hard `default: throw` for
//     unknown verbs, and this harness file does not own that module — so going through it would
//     make teardown depend on someone else also teaching the shared codec the verb. Encoding the
//     one request shape here (over the module's exported, protocol-agnostic `cborEncode`
//     primitive, exactly as `orchd-survive.mjs` does for its own frame contract) keeps teardown
//     working either way.
//  2. `daemon-harness.mjs`'s `decodeFrame()`/`decodePush()` throw on any variant they don't know,
//     and they run INSIDE the socket's `data` listener — i.e. an unknown push (say a future
//     `Push::WorkspaceRemoved` broadcast by the very request we send here) would surface as an
//     uncaught exception and take the process down with a non-zero exit, MASKING an otherwise
//     green run. The reader below is deliberately tolerant: it looks only for the `Response` frame
//     whose id it is waiting on and silently ignores everything else, including bodies it cannot
//     decode at all.
//
// Teardown therefore runs on its own short-lived connection, opened AFTER the test's own
// connections have been dropped (so no shared-codec listener is left attached to receive a
// broadcast it cannot parse).
// ============================================================================

let nextCleanupFrameId = 1_000_000;

/**
 * Open a throwaway connection for teardown only: real preamble handshake (via the shared
 * `preambleHandshake`, so the wire version range is never hardcoded here), then a minimal,
 * failure-tolerant frame reader. Rejects — rather than hanging — if the socket is unreachable, the
 * handshake fails, or either exceeds `timeoutMs`.
 */
function cleanupConnect(sockPath, timeoutMs = 5000) {
  return new Promise((resolve, reject) => {
    const sock = net.connect(sockPath);
    // Declared before `failEarly` closes over it (no temporal-dead-zone window even if the socket
    // errors synchronously). `failEarly` clears it too, not just the success path: a connect or
    // handshake that fails FAST must not leave a live timer holding the event loop open — this
    // runs during the process's last gasp, and a teardown that visibly hangs reads as a bug.
    let connectTimer = null;
    const failEarly = (e) => {
      clearTimeout(connectTimer);
      try {
        sock.destroy();
      } catch {
        /* already gone */
      }
      reject(e instanceof Error ? e : new Error(String(e)));
    };
    connectTimer = setTimeout(
      () => failEarly(new Error(`connect to ${sockPath} timed out after ${timeoutMs}ms`)),
      timeoutMs,
    );
    sock.once("error", failEarly);
    sock.once("connect", async () => {
      try {
        const { chosen, leftover } = await preambleHandshake(sock, { timeoutMs });
        clearTimeout(connectTimer);
        sock.off("error", failEarly);
        const conn = { sock, chosenVersion: chosen, buf: leftover, pending: [], closed: false };
        // Reject every in-flight waiter the moment the daemon hangs up. An older daemon that does
        // not know a verb we send fails to deserialize the `Frame` and DISCONNECTS without a reply
        // (`crates/sessiond/src/socket_server.rs`: "framing/protocol error ⇒ disconnect"), so this
        // is the path that reports "this daemon doesn't understand the verb" promptly instead of
        // waiting out the request timeout.
        const drop = (err) => {
          conn.closed = true;
          const waiters = conn.pending;
          conn.pending = [];
          for (const w of waiters) w.reject(err);
        };
        sock.on("close", () => drop(new Error("daemon closed the connection without replying")));
        sock.on("error", (e) => drop(new Error(`socket error: ${e.message}`)));
        sock.on("data", (chunk) => {
          conn.buf = Buffer.concat([conn.buf, chunk]);
          for (;;) {
            if (conn.buf.length < 4) break;
            const len = conn.buf.readUInt32LE(0);
            if (conn.buf.length < 4 + len) break;
            const body = conn.buf.subarray(4, 4 + len);
            conn.buf = conn.buf.subarray(4 + len);
            let frame;
            try {
              frame = cborDecode(body);
            } catch {
              continue; // undecodable body: not ours to interpret, never fatal during teardown
            }
            const response = frame && typeof frame === "object" ? frame.Response : null;
            if (!response) continue; // a Push (or any other frame variant) — irrelevant here
            const w = conn.pending.find((p) => p.id === response.id);
            if (w) {
              conn.pending = conn.pending.filter((p) => p !== w);
              w.resolve(response.res);
            }
          }
        });
        resolve(conn);
      } catch (e) {
        clearTimeout(connectTimer);
        failEarly(e);
      }
    });
  });
}

/**
 * Send one already-wire-shaped request (`{ VariantName: { snake_case_field: ... } }`, the
 * externally-tagged `crates/protocol/src/lib.rs::Request` encoding) over a `cleanupConnect`
 * connection and resolve with the RAW CBOR `Response` value — no variant table, so a response
 * shape this file has never heard of still resolves instead of throwing. Rejects on timeout or if
 * the daemon hangs up first; the timer is always cleared, so a rejected teardown request can never
 * hold the event loop open past the end of the run.
 */
function cleanupRequest(conn, wireReq, timeoutMs = 5000) {
  const id = nextCleanupFrameId++;
  return new Promise((resolve, reject) => {
    if (conn.closed || conn.sock.destroyed) {
      reject(new Error("connection already closed"));
      return;
    }
    const timer = setTimeout(() => {
      conn.pending = conn.pending.filter((p) => p.id !== id);
      reject(new Error(`request timed out after ${timeoutMs}ms`));
    }, timeoutMs);
    conn.pending.push({
      id,
      resolve: (v) => {
        clearTimeout(timer);
        resolve(v);
      },
      reject: (e) => {
        clearTimeout(timer);
        reject(e);
      },
    });
    const body = cborEncode({ Request: { id, req: wireReq } });
    const out = Buffer.alloc(4 + body.length);
    out.writeUInt32LE(body.length, 0);
    body.copy(out, 4);
    conn.sock.write(out);
  });
}

/** Summarize a raw CBOR `Response` value: `Ack` -> ok, `{Error:{code,message}}` -> not ok, any
 * other single-key variant -> ok (the daemon answered the verb; the exact payload is irrelevant to
 * teardown). */
function describeWireResponse(res) {
  if (res === "Ack") return { ok: true, text: "Ack" };
  if (res && typeof res === "object") {
    const [variant] = Object.keys(res);
    if (variant === "Error") {
      const inner = res.Error ?? {};
      return { ok: false, text: `Error{code=${inner.code}, message=${inner.message}}` };
    }
    return { ok: true, text: variant ?? JSON.stringify(res) };
  }
  return { ok: false, text: JSON.stringify(res) };
}

/**
 * Report state this run created in the daemon and could NOT remove again. Loud and specific: the
 * whole point of this task is that a leftover must never be silent again. Severity is scaled to
 * where the row actually lives:
 *
 *  - `BPA_E2E_EXTERNAL_DAEMON=1` — the row is in the USER's real
 *    `~/Library/Application Support/ai.builderpro.desktop/bpa.db` and will be visible in the app's
 *    sidebar until someone deletes it by hand: a full `stderr` banner naming the id.
 *  - default (harness-spawned) variant — the row is in a throwaway `bpa.db` under the scratch
 *    `HOME` this file `rm -rf`s a few lines later, so nothing survives: one honest note, no banner
 *    (crying wolf on every green run would train people to ignore the real banner).
 *
 * Never throws and never touches `process.exitCode`: teardown is not allowed to convert a pass
 * into a fail, nor to overwrite a failure that already happened.
 */
function reportUnremovedState(reason, { workspaceId = null, sessionIds = [] } = {}) {
  if (!EXTERNAL_DAEMON) {
    log(
      `note: could not remove daemon-side state (${reason}); harmless in this variant — ` +
        `workspace ${workspaceId ?? "-"} / sessions [${sessionIds.join(", ")}] live only in the ` +
        `isolated bpa.db under ${daemonEnvOverrides.HOME}, deleted below`,
    );
    return;
  }
  const bar = "!".repeat(88);
  console.error(`[e2e] ${bar}`);
  console.error("[e2e] !! LEAKED DAEMON STATE — the harness could not clean up after itself");
  if (workspaceId != null) {
    console.error(
      `[e2e] !! workspace id : ${workspaceId}  (name "${WORKSPACE_NAME}", rootPath ${cleanup.workspaceRoot})`,
    );
  }
  if (sessionIds.length > 0) {
    console.error(`[e2e] !! session ids  : ${sessionIds.join(", ")}`);
  }
  console.error(`[e2e] !! daemon socket: ${SOCK}`);
  console.error(`[e2e] !! reason       : ${reason}`);
  console.error(
    "[e2e] !! This ran with BPA_E2E_EXTERNAL_DAEMON=1, so the above lives in your REAL database",
  );
  console.error(
    "[e2e] !! (~/Library/Application Support/ai.builderpro.desktop/bpa.db) and WILL show up in the",
  );
  console.error(
    "[e2e] !! app's workspace sidebar, pointing at a temp dir that no longer exists. Remove it by",
  );
  console.error("[e2e] !! hand (or from the app) — and update this harness if the verb changed.");
  console.error(`[e2e] ${bar}`);
}

/**
 * Kill every session and remove the workspace this run created, over a dedicated teardown
 * connection. Entirely best-effort: every failure is reported (see `reportUnremovedState`) and
 * swallowed, so the run's real pass/fail verdict is preserved exactly as `main()` left it.
 *
 * Sessions are killed BEFORE the workspace is removed — both because that is the correct order
 * (no orphaned session rows under a removed workspace) and because `RemoveWorkspace` is the
 * request that may hit an older daemon and get the connection dropped underneath us.
 */
async function removeDaemonSideStateBestEffort() {
  const sessionIds = cleanup.sessionIds.slice();
  const workspaceId = cleanup.workspaceId;
  if (workspaceId == null && sessionIds.length === 0) return;

  // The harness-spawned variant keeps every durable row under the scratch `HOME` deleted at the
  // end of `cleanupAll()`, so once its daemon is gone there is nothing left to remove and nothing
  // to warn about. (The EXTERNAL_DAEMON variant never takes this branch: that daemon is launchd's,
  // still running, and its DB is the user's own.)
  if (!EXTERNAL_DAEMON && (cleanup.daemonPid == null || !pidAlive(cleanup.daemonPid))) {
    log(
      "skipping daemon-side workspace/session teardown: the harness's own daemon is no longer " +
        "running and its entire isolated state dir is deleted below",
    );
    return;
  }

  let conn;
  try {
    conn = await cleanupConnect(SOCK);
  } catch (e) {
    reportUnremovedState(`could not connect to the daemon to clean up: ${e.message}`, {
      workspaceId,
      sessionIds,
    });
    return;
  }

  const unkilled = [];
  try {
    for (const sid of sessionIds) {
      // Same `KillSession { session_id }` request shape phase4/phase5 use over the shared codec —
      // only the transport differs (see this section's header comment).
      try {
        const described = describeWireResponse(await cleanupRequest(conn, { KillSession: { session_id: sid } }));
        // A daemon-side `Error` here is almost always benign ("already gone" — e.g. the session's
        // shell exited, or a rehydrated-inactive session after phase5's daemon restart), and
        // `RemoveWorkspace` below cleans up the row either way, so it is logged, not escalated.
        log(`teardown: KillSession ${sid} -> ${described.text}`);
        cleanup.sessionIds = cleanup.sessionIds.filter((x) => x !== sid);
      } catch (e) {
        unkilled.push(sid);
        log(`teardown: KillSession ${sid} FAILED: ${e.message}`);
      }
    }

    if (workspaceId != null) {
      try {
        const described = describeWireResponse(
          await cleanupRequest(conn, { RemoveWorkspace: { workspace_id: workspaceId } }),
        );
        if (described.ok) {
          cleanup.workspaceId = null;
          log(`teardown: RemoveWorkspace ${workspaceId} -> ${described.text}`);
        } else {
          reportUnremovedState(`daemon rejected RemoveWorkspace with ${described.text}`, {
            workspaceId,
            sessionIds: unkilled,
          });
        }
      } catch (e) {
        // The expected shape of "this daemon predates the `RemoveWorkspace` verb": the daemon
        // cannot deserialize the frame and disconnects without replying, so this rejects with
        // "daemon closed the connection without replying" (or, if it stalls instead, the request
        // timeout). Either way: reported loudly, never fatal.
        reportUnremovedState(
          `RemoveWorkspace failed: ${e.message} — this daemon most likely predates the ` +
            "RemoveWorkspace verb (rebuild/restart it from a current checkout, then remove the " +
            "workspace named above by hand)",
          { workspaceId, sessionIds: unkilled },
        );
      }
    } else if (unkilled.length > 0) {
      reportUnremovedState("one or more KillSession requests failed", { workspaceId: null, sessionIds: unkilled });
    }
  } finally {
    try {
      conn.sock.destroy();
    } catch {
      /* already closed */
    }
  }
}

/**
 * Tear down everything this run created, regardless of how `main()` exited (return, thrown
 * assertion, or a request/push timeout). Runs unconditionally from `main()`'s `finally` below.
 * Every step is independently best-effort (wrapped so one failure can't skip the rest) — this is
 * cleanup code, not test assertions; a cleanup failure must never mask (or be mistaken for) the
 * actual pass/fail result already recorded in `process.exitCode`.
 */
async function cleanupAll() {
  // Drop the test's own connections FIRST: they carry `daemon-harness.mjs`'s strict frame decoder,
  // which throws (inside a `data` listener — i.e. uncatchably, taking the process with it) on any
  // Push variant it doesn't know. The workspace/session teardown below may well provoke exactly
  // such a broadcast, and it runs on its own tolerant connection instead. Sessions are unaffected
  // by this: surviving a client disconnect is the very property phase4 asserts.
  for (const c of cleanup.conns) {
    try {
      c.sock.destroy();
    } catch {
      /* already closed */
    }
  }
  cleanup.conns = [];

  // Then remove what this run created INSIDE the daemon — before any SIGTERM below, since a dead
  // daemon can no longer service the requests. Best-effort and self-reporting; never throws.
  await removeDaemonSideStateBestEffort();

  if (!EXTERNAL_DAEMON && cleanup.daemonPid != null) {
    // Kill the WHOLE process group (daemon + its shell child, per spec: a leaked daemon during test
    // cleanup must not orphan its shell either), then fall back to SIGKILL if it's still around after
    // the grace period — a hung daemon must never survive the harness process exiting.
    killProcessGroup(cleanup.daemonPid, "SIGTERM");
    for (let i = 0; i < 50 && pidAlive(cleanup.daemonPid); i++) await sleep(100);
    if (pidAlive(cleanup.daemonPid)) {
      log(`daemon (pid ${cleanup.daemonPid}) still alive after SIGTERM grace period; sending SIGKILL`);
      killProcessGroup(cleanup.daemonPid, "SIGKILL");
      for (let i = 0; i < 20 && pidAlive(cleanup.daemonPid); i++) await sleep(100);
    }
    if (pidAlive(cleanup.daemonPid)) {
      log(`WARNING: daemon (pid ${cleanup.daemonPid}) survived SIGKILL — manual cleanup may be needed`);
    }
  }

  if (cleanup.workspaceRoot) {
    try {
      fs.rmSync(cleanup.workspaceRoot, { recursive: true, force: true });
    } catch {
      /* best-effort cleanup */
    }
  }

  if (isolatedTmpDir) {
    try {
      fs.rmSync(isolatedTmpDir, { recursive: true, force: true });
    } catch {
      /* best-effort cleanup */
    }
  }

  if (daemonEnvOverrides.HOME) {
    try {
      fs.rmSync(daemonEnvOverrides.HOME, { recursive: true, force: true });
    } catch {
      /* best-effort cleanup */
    }
  }
}

main()
  .catch((e) => {
    console.error("[e2e] FAIL:", e);
    process.exitCode = 1;
  })
  .finally(async () => {
    // `main()`'s verdict is already recorded in `process.exitCode` by the `.catch` above, and
    // teardown runs strictly after it. Anything that escapes `cleanupAll()`'s own per-step
    // best-effort guards is reported here and goes no further: an unhandled rejection out of this
    // `finally` would replace a real assertion failure's diagnostics with a teardown stack trace
    // (and would flip a green run red), which is exactly the masking this harness must not do.
    try {
      await cleanupAll();
    } catch (e) {
      console.error(
        "[e2e] WARNING: cleanup failed — the pass/fail verdict above still stands, but this run " +
          "may have left state behind:",
        e,
      );
    }
  });
