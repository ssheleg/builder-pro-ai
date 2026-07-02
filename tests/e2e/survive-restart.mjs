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
  hello,
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

// ---- socket isolation (never touch the real user socket at /tmp/bpa-<uid> or $XDG_RUNTIME_DIR/bpa) ----
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
// real daemon on this machine. The launchd-managed variant (`EXTERNAL_DAEMON`) deliberately keeps
// using the real, ambient `XDG_RUNTIME_DIR` — it is attaching to an already-running,
// launchd-supervised daemon at the real path, by design (see README §2).
let isolatedTmpDir = null;
if (!EXTERNAL_DAEMON) {
  isolatedTmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "bpa-e2e-"));
  process.env.XDG_RUNTIME_DIR = isolatedTmpDir;
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
  // ---- phase 0: spawn (or attach to) the daemon and complete the Hop-B handshake ----
  let daemonProc = null;
  if (EXTERNAL_DAEMON) {
    log(`phase0: attaching to externally-managed daemon at socket ${SOCK}`);
  } else {
    assertRealBinary(DAEMON_BIN);
    log(`phase0: spawn daemon ${DAEMON_BIN} (isolated XDG_RUNTIME_DIR=${isolatedTmpDir})`);
    // envOverrides pins the CHILD's XDG_RUNTIME_DIR too (belt-and-suspenders alongside setting it
    // on `process.env` above): the daemon's own lockfile/log-dir resolution must never fall back
    // to the real user path even if something upstream of `spawn()` stripped an inherited var.
    daemonProc = spawnDaemon(DAEMON_BIN, SOCK, isolatedTmpDir ? { XDG_RUNTIME_DIR: isolatedTmpDir } : {});
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
  const welcome = await hello(conn);
  assert.equal(welcome.t, "Welcome", `expected Welcome, got ${JSON.stringify(welcome)}`);
  assert.equal(welcome.protoVersion, 1, `proto version mismatch: ${JSON.stringify(welcome)}`);
  log("phase0 OK: handshake");

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

  // Relaunch: a brand-new client connects, lists sessions, reattaches, and expects the
  // scrollback replay to still contain the marker written before the "quit".
  const conn2 = await connect(SOCK);
  cleanup.conns.push(conn2);
  const w2 = await hello(conn2);
  assert.equal(w2.t, "Welcome", `reattach handshake failed: ${JSON.stringify(w2)}`);

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
    // process itself in this variant (see `EXTERNAL_DAEMON` guard there).
    await request(conn2, { t: "KillSession", sessionId: sid }).catch(() => {});
  }
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
}

main()
  .catch((e) => {
    console.error("[e2e] FAIL:", e);
    process.exitCode = 1;
  })
  .finally(async () => {
    await cleanupAll();
  });
