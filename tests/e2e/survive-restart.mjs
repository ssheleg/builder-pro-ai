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
} from "./lib/daemon-harness.mjs";

const REPO = path.resolve(import.meta.dirname, "..", "..");
const DAEMON_BIN = process.env.BPA_SESSIOND ?? path.join(REPO, "target", "debug", "bpa-sessiond");
// When true (the launchd-managed variant, see README §2), the harness does not spawn its
// own daemon and does not send SIGTERM to it during cleanup — launchd owns the lifecycle.
const EXTERNAL_DAEMON = process.env.BPA_E2E_EXTERNAL_DAEMON === "1";

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
 */
const cleanup = {
  daemonPid: null,
  conns: [],
  workspaceRoot: null,
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
  // `connect()` itself performs the v2 preamble handshake (magic/version negotiation) before
  // resolving — see `preambleHandshake()` in daemon-harness.mjs. A bad magic, an Incompatible
  // result, or an unexpected chosen version throws loudly from inside `connect()` itself, so
  // reaching here already proves the handshake succeeded; assert the negotiated version explicitly
  // too, matching the old harness's self-checking `Welcome` roundtrip assertion style.
  assert.equal(conn.chosenVersion, 2, `preamble negotiated unexpected version: ${JSON.stringify(conn)}`);
  log(`phase0 OK: preamble handshake (chosen=${conn.chosenVersion}, daemonBuild=${JSON.stringify(conn.daemonBuild)})`);

  // ---- phase 1: create a workspace + session rooted in a temp dir ----
  log("phase1: create workspace + session");
  const root = fs.mkdtempSync(path.join(REPO, "target", "e2e-ws-"));
  cleanup.workspaceRoot = root;
  const ws = await request(conn, { t: "CreateWorkspace", name: "e2e", rootPath: root });
  assert.equal(ws.t, "Workspace", `CreateWorkspace -> ${JSON.stringify(ws)}`);
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
  log(`phase1 OK: session ${sid}`);

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
  assert.ok(pidAlive(cleanup.daemonPid), `daemon (pid ${cleanup.daemonPid}) not running before client quit`);
  const daemonPid = cleanup.daemonPid;

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
  assert.equal(conn2.chosenVersion, 2, `reattach handshake negotiated unexpected version: ${JSON.stringify(conn2)}`);

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
    // Under the launchd-managed variant we own only the session, not the daemon lifecycle: best-effort
    // kill the test session so it doesn't linger under the shared, externally-managed daemon. The
    // shared `cleanupAll()` in `finally` still closes our connections but must NOT touch the daemon
    // process itself in this variant (see `EXTERNAL_DAEMON` guard there), and phase 5 below (a real
    // DaemonShutdown + process-level relaunch) is skipped entirely — under this variant the daemon
    // lifecycle belongs to launchd (README §2's `KeepAlive{Crashed}` is the mechanism that tracks;
    // this harness deliberately doesn't drain/replace a daemon it doesn't own).
    await request(conn2, { t: "KillSession", sessionId: sid }).catch(() => {});
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
    2,
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

/**
 * Tear down everything this run created, regardless of how `main()` exited (return, thrown
 * assertion, or a request/push timeout). Runs unconditionally from `main()`'s `finally` below.
 * Every step is independently best-effort (wrapped so one failure can't skip the rest) — this is
 * cleanup code, not test assertions; a cleanup failure must never mask (or be mistaken for) the
 * actual pass/fail result already recorded in `process.exitCode`.
 */
async function cleanupAll() {
  for (const c of cleanup.conns) {
    try {
      c.sock.destroy();
    } catch {
      /* already closed */
    }
  }

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
    await cleanupAll();
  });
