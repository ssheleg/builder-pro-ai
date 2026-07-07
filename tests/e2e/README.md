# E2E survive-restart harness

Proves the S1 core promise end-to-end (spec §14.1):

> create a terminal → run a command → observe OSC-driven status → quit the app →
> the daemon + shell **survive** → relaunch → reattach → scrollback intact.

There are three ways to exercise this property, in increasing order of fidelity to a
real user session and decreasing order of how cheaply/automatically they run:

1. **Socket harness (this directory)** — CI-runnable, no GUI, speaks the Hop-B wire
   protocol directly against a locally-spawned `bpa-sessiond`. This is the
   deterministic, automatable core and is documented in detail below.
2. **launchd-managed variant** — the same harness, but against a daemon that launchd
   (not the harness) started and supervises, proving `KeepAlive`/kickstart wiring
   rather than just the daemon's own client-disconnect handling.
3. **Full-GUI manual/CI confirmation** — launch the signed, bundled `.app`, drive the
   actual terminal UI, quit and relaunch the window. Requires a full `tauri build`
   (disk/CI cost) and is a human or dedicated-CI step, documented in §3 below.

## 1. Socket harness (`npm run e2e:survive`)

### What it does

`survive-restart.mjs` speaks the v2 wire (Pv2 §4.2/§7) directly against a
locally-spawned `bpa-sessiond`: a codec-agnostic **preamble** handshake (raw
little-endian primitives, magic `0x42504141` "BPAA") immediately after connect,
followed by a **CBOR** (RFC 8949) frame stream — `u32-LE length prefix | CBOR(Frame)
body`, matching `crates/protocol/src/framing.rs` exactly. `daemon-harness.mjs`
hand-rolls both the preamble codec and a minimal CBOR encoder/decoder (no
dependency — see its module doc comment) rather than pulling in a general-purpose
CBOR library, no Rust bridge, no test-runner dependency, no WKWebView. It drives the
daemon through the full lifecycle:

- **phase0** — spawn the daemon on a scratch socket **isolated from the real user
  socket AND the real user state dir** (a fresh `mkdtemp` dir for `XDG_RUNTIME_DIR`
  and a SEPARATE fresh `mkdtemp` dir for `HOME`, so it can never collide with — or
  during cleanup, signal — an actual running daemon at `/tmp/bpa-<uid>` or
  `$XDG_RUNTIME_DIR/bpa`, nor read/write the real `~/Library/Application
  Support/ai.builderpro.desktop/bpa.db`), connect, and complete the v2 preamble
  handshake (`connect()` performs this internally — see `preambleHandshake()` in
  `daemon-harness.mjs`). Asserts the negotiated `chosenVersion === 2`.
- **phase1** — `CreateWorkspace` (rooted in a fresh temp dir under `target/`),
  `CreateSession` (a real `/bin/zsh`).
- **phase2** — `AttachSession` → first push is `Replay`; `WriteStdin` an
  `echo <unique-marker>` command; collect `Output` pushes until the marker appears.
- **phase3** — `WriteStdin` a `sleep 1` command; assert a `StateChanged` push with
  `lifecycle.kind === "running"` (driven by the shell-integration OSC 133 `C`
  sequence), then a further `StateChanged` back to `lifecycle.kind === "atPrompt"`
  (OSC 133 `B`/`D`) once the command returns.
- **phase4 (the load-bearing assertion)** — hard-close the client socket
  (`sock.destroy()`) **without** sending `DetachSession` or `KillSession`, exactly
  reproducing what happens when a user Cmd-Qs or force-quits the app. Then:
  - assert `pgrep bpa-sessiond` still lists the daemon's pid;
  - assert the shell's pid (found as a child of the daemon via `pgrep -P`) is still
    alive (`kill -0`);
  - open a **fresh** client connection (simulating app relaunch), preamble handshake
    again, `ListSessions` and assert the session is still listed;
  - `AttachSession` again and assert the `Replay` push's `content` still contains the
    marker written in phase2 (scrollback survived the "restart" — but note the
    daemon PROCESS itself never stopped in this phase; only the client reconnected).
- **phase5 (daemon-restart rehydration, closes BL-7, Pv2 §9.8)** — a REAL daemon
  process restart, not just a client reconnect:
  - `DaemonShutdown{drain:true}` over the still-open phase4 connection; assert `Ack`.
  - poll `kill -0` on the exact pid this harness spawned until the daemon PROCESS
    actually exits (bounded, 10s deadline — fails loudly if it doesn't).
  - relaunch the SAME `bpa-sessiond` binary against the SAME socket path AND the SAME
    isolated `XDG_RUNTIME_DIR`/`HOME` env overrides phase0 used (`daemonEnvOverrides`,
    reused verbatim) — this is what makes `resolve_socket_path()` AND
    `app_support_dir()` (`crates/sessiond/src/boot.rs`, keyed off `HOME`, NOT
    `XDG_RUNTIME_DIR`) resolve to the identical `bpa.db` the pre-restart daemon's
    drain-flush wrote to.
  - reconnect (fresh preamble handshake) and **assert 1**: `ListSessions` shows the
    phase-1 session `sid` present with `isActive === false` — proving
    `Db::rehydrate()`/`list_sessions()` correctly rehydrates session metadata from
    SQLite as inactive after a real process restart.
  - **assert 2**: reattach (`AttachSession{sid}` must `Ack`) and assert the `Replay`
    push's scrollback still contains the phase-2 marker. This exercises the daemon's
    cold-rehydrate path (`bpa-sessiond` boot loads every persisted session from
    SQLite into the Supervisor as an inactive, replay-only entry, so `AttachSession`
    on a rehydrated session serves `Push::Replay` from the persisted scrollback
    rather than returning `NoSuchSession`; unknown session ids still error).

No assertion is weakened to pass vacuously — a missing daemon binary, a broken
handshake, absent lifecycle pushes, or lost scrollback each fail with a specific,
actionable message and a non-zero exit code.

> **History note:** phase5's assertion 2 was blocked when first authored — at the
> time, the daemon's `AttachRegistry` refused attach on any session absent from the
> in-memory Supervisor (returning `NoSuchSession` for every rehydrated-from-SQLite
> session by design), and no wire request exposed `Db::load_scrollback` out-of-band.
> The harness failed loudly at that assertion rather than weakening it. The gap was
> closed daemon-side by the cold-rehydrate change (`feat(sessiond): cold-rehydrate
> persisted sessions as inactive + attach-inactive replays scrollback`), after which
> phases 0-5 pass unmodified. See `.superpowers/sdd/task-12-report.md` for the full
> investigation.

### Prerequisites

- A real (not placeholder-stub) `bpa-sessiond` binary. Build it with:
  ```sh
  export PATH="$HOME/.cargo/bin:$PATH"   # if cargo isn't already on PATH
  cargo build -p bpa-sessiond --bin bpa-sessiond
  ```
  The harness refuses to run against the S0 scaffold placeholder (a `sh` stub that
  always exits 1, left at `target/debug/bpa-sessiond` before T13 lands a real build)
  and fails with an explicit message telling you to build it, rather than failing
  confusingly at connect-time.
- Node ≥ 18 (uses `node:timers/promises`, `import.meta.dirname`; developed against
  Node 25). No npm packages required — the harness is dependency-free ESM.
- macOS or Linux (`pgrep`, Unix domain sockets). Not runnable on Windows as written.

### Running it

```sh
npm run e2e:survive
```

Equivalent to `node tests/e2e/survive-restart.mjs`. Override the daemon binary path
with `BPA_SESSIOND=/path/to/bpa-sessiond npm run e2e:survive` (defaults to
`target/debug/bpa-sessiond` relative to the repo root).

Expected output on success:

```
[e2e] phase0 OK: preamble handshake (chosen=2, daemonBuild="0.1.0")
[e2e] phase1 OK: session <uuid>
[e2e] phase2 OK: command output observed
[e2e] phase3 OK: OSC-133 lifecycle running -> atPrompt
[e2e] phase4a OK: daemon + shell survived client quit
[e2e] phase4b OK: reattach + scrollback intact
[e2e] phase5 OK: DaemonShutdown Ack received
[e2e] phase5 OK: daemon (pid <pid>) process exited
[e2e] phase5 OK: reconnected to relaunched daemon (pid <pid>)
[e2e] phase5 OK: session <uuid> rehydrated with isActive=false
[e2e] phase5 OK: reattach after daemon restart replays scrollback with marker intact (BL-7 closed)
[e2e] ALL PHASES PASSED
```
exit code `0`.

### `tests/e2e/lib/daemon-harness.mjs`

The reusable client library:
- **Preamble handshake** (`encodeClientPreamble`, `preambleHandshake`) — the fixed,
  codec-independent header that precedes any framed traffic (Pv2 §4.2), mirroring
  `crates/protocol/src/preamble.rs::encode_client_preamble`/`decode_daemon_reply`
  byte-for-byte. `connect()` performs this automatically before resolving.
- **Minimal hand-rolled CBOR codec** (`cborEncode`/`cborDecode`) plus the
  `Frame`/`Request`/`Response`/`Push` shape mapping (`encodeFrame`/`decodeFrame`) —
  see the codec's module doc comment for the exact externally-tagged/camelCase/
  `Vec<u8>`-as-array shape rules transcribed from `crates/protocol/src/lib.rs`. Every
  shape here was cross-verified byte-for-byte against the REAL `ciborium`/protocol
  crate (both encode and decode directions) during authoring — see
  `.superpowers/sdd/task-12-report.md` for the verification transcript.
- A length-prefixed-framing socket wrapper (`connect`, `request`, `nextPush`) and
  process probes (`pgrepDaemon`, `pgrepShell`, `pidAlive`, `spawnDaemon`,
  `launchctlKickstart`, `killGui`).

If `crates/protocol/src/lib.rs`'s field names/variant names ever change, update the
corresponding `enc*`/`dec*` functions in `daemon-harness.mjs` to match — the codec's
doc comments name the exact struct/field being mirrored throughout.

One subtlety worth calling out explicitly: `SessionLifecycle` is internally tagged on
`kind` (camelCase — `crates/protocol/src/lib.rs`, `#[serde(tag = "kind", rename_all =
"camelCase")]`). Under CBOR this derives plainly (unlike the old bincode dual-codec
shim it used to need) — it decodes as `{"kind":"atPrompt"}` or
`{"kind":"exited","code":0,"signal":null}` directly, no JSON re-parsing step needed.

## 2. launchd-managed variant

The socket harness above spawns its own `bpa-sessiond` as a detached child process,
which proves the daemon survives a **client** disconnecting but does not exercise
`launchd.rs`'s install/bootstrap/kickstart path or `KeepAlive{Crashed}` supervision.
To prove launchd (not the harness) is what keeps the daemon reachable across a GUI
quit/relaunch, run the same harness against a launchd-managed daemon instead:

1. Build and run the app once (or call `install_agent()` / `bootstrap()` from
   `src-tauri/src/launchd.rs` directly in a small dev harness) so
   `~/Library/LaunchAgents/ai.builderpro.desktop.sessiond.plist` exists and
   `launchctl bootstrap gui/$UID <plist>` has been run. The label is
   `ai.builderpro.desktop.sessiond` (spec §8.3).
2. Start (or restart) it on demand:
   ```sh
   launchctl kickstart -k gui/$(id -u)/ai.builderpro.desktop.sessiond
   ```
3. Resolve the socket path the daemon actually bound (spec §8.1):
   `$XDG_RUNTIME_DIR/bpa/d.sock` if `XDG_RUNTIME_DIR` is set, else
   `/tmp/bpa-<uid>/d.sock`. Export the same `XDG_RUNTIME_DIR` (or leave it unset to
   fall through to `/tmp/bpa-<uid>`) in the shell that runs the harness so
   `resolveSocketPath()` in `daemon-harness.mjs` agrees with the daemon.
4. Run the harness **without** letting it spawn its own daemon or `SIGTERM` it on
   cleanup:
   ```sh
   BPA_E2E_EXTERNAL_DAEMON=1 npm run e2e:survive
   ```
   With `BPA_E2E_EXTERNAL_DAEMON=1`, phase0 skips `spawnDaemon()` and connects
   directly to the already-running, launchd-managed daemon; cleanup skips the
   `SIGTERM` and only kills the harness's own test session (`KillSession`), leaving
   the daemon itself under launchd's supervision as it would be in production.
5. To specifically prove the *crash-restart* half of `KeepAlive{Crashed}` (as opposed
   to the "client disconnects, daemon simply never dies" property phase4 already
   covers): `kill -9` the daemon pid mid-test and observe `launchctl print
   gui/$UID/ai.builderpro.desktop.sessiond` transition back to running via launchd's
   own restart — this is a `launchd.rs` mock-runner unit-test concern (spec §14.1
   "launchd mock tests") rather than something this harness re-asserts, since a killed
   daemon means the in-flight session PTYs beneath it are also gone (a fresh
   `bpa.db`-backed rehydrate, not a live scrollback replay) — a different code path
   than phase4's live-survive assertion.

## 3. Full-GUI confirmation (human / dedicated-CI step — NOT run by this harness)

This is the manual (or dedicated macOS-CI) procedure that mirrors spec §14.1's E2E
wording exactly, driving the actual bundled `.app` and its WKWebView-hosted terminal
UI. It requires a full `tauri build` (the disk-heavy step this harness's design
explicitly avoids — see "Disk-scoping decision" below) and, for full automation, a
GUI driver (`tauri-driver` + `webdriverio`); absent that tooling it is run by hand.

**Prerequisites:** a machine/CI runner with enough free disk for `npm run tauri
build` (a full release build of `src-tauri`, universal binary, code-signing +
notarization per spec §14.3), and either a human at the keyboard or `tauri-driver`
set up per the [Tauri WebDriver
guide](https://v2.tauri.app/develop/tests/webdriver/) (out of scope to install here;
covered by T24/T25's bundling task).

**Manual procedure:**

1. `npm run tauri build` — produces the signed, notarized universal `.app` bundle
   (T24/T25).
2. Launch the built `.app` (double-click in Finder, or `open
   /path/to/Builder Pro AI.app`).
3. In the app UI: create a workspace (pick a folder), create a terminal in it.
4. In the terminal, run `echo hi` and confirm the output renders in the pane.
5. Watch the status dot: it should go from idle/at-prompt color to "running" color
   while a command executes (e.g. run `sleep 2`) and back to idle once it returns —
   this is the OSC-133-driven `StatusDot` component (spec §12) reacting to the same
   `StateChanged` pushes phase3 of the socket harness asserts directly on the wire.
6. Quit the app (Cmd-Q, or the Dock "Quit").
7. From a separate Terminal.app / shell:
   ```sh
   pgrep bpa-sessiond   # must still list a pid — the daemon survived the GUI quitting
   pgrep -P $(pgrep bpa-sessiond)   # the shell child must still be listed too
   ```
8. Relaunch the `.app`.
9. Confirm the terminal pane reattaches automatically and repaints the prior
   scrollback (including the `echo hi` output and the OSC-driven status history) —
   this is the `Replay` push driving `TerminalManager`'s keep-alive `term.write` path
   (spec §12), the GUI-level counterpart of phase4b's wire-level assertion.

**Automated (tauri-driver) procedure:** identical steps, but driven via WebDriver
(`webdriverio`) instead of a human: navigate/click through steps 3-5 using CSS
selectors on the workspace sidebar / terminal tab / xterm.js DOM, assert the status
dot's computed background-color at steps 5, kill the WebDriver session (which leaves
the underlying app process running — do **not** call `driver.deleteSession()`'s app
teardown, or send the app a real quit event instead) at step 6, re-launch and
re-attach the WebDriver session at step 8, and assert the xterm.js viewport's
rendered text (or `terminal.serialize()` via the `@xterm/addon-serialize` already a
project dependency) contains the phase4-equivalent marker text at step 9. This is
left as a documented procedure rather than implemented here because it needs the
signed bundle (T24/T25) as an input and does not fit the disk budget available while
authoring this task — the CSS-selector/session-teardown specifics belong in that
follow-up automation, not guessed at here.

## History: authoring under a disk constraint, then verifying against the real daemon

This harness (`tests/e2e/*.mjs`) is a dependency-free Node ESM script — no Rust bridge,
no `npm install` — rather than a `crates/sessiond/tests/*.rs` integration test. It was
originally authored on a machine with the workspace `target/` directory already ~8.7 GB
and **under 1 GB free**, where `cargo build -p bpa-sessiond` was off the table (even an
incremental link risks "No space left on device"). At that point the harness could only
be validated by `node --check` (syntax) and by running it against the **S0 scaffold
placeholder** binary (a one-line `sh` stub that always `exit 1`s) and confirming
`assertRealBinary()` caught it with the correct diagnostic — proving the harness fails
loudly rather than passing vacuously, but NOT proving the wire codec was actually
correct against the real daemon.

It was not: once disk pressure was resolved and the harness ran against a real
`cargo build -p bpa-sessiond` binary, phase0 failed with `request Hello timed out`. Root
cause (Task 23 follow-up fix) — **not** a daemon bug:

- `daemon-harness.mjs`'s `hello()` sent the handshake `Hello` frame through the same
  auto-incrementing request-id counter as every other request, so it went out with
  `id: 1`. The daemon's handshake gate
  (`crates/sessiond/src/socket_server.rs::handle_client`) pattern-matches the very
  first frame as literally `Frame::Request { id: 0, req: Request::Hello { .. } }` and
  always replies `Frame::Response { id: 0, .. }` for it. An `id: 1` Hello therefore
  failed that `matches!` guard, so the daemon replied `Incompatible` (still framed as
  `id: 0`) and closed the connection — a reply the harness could never correlate
  against its own `id: 1` waiter, so it just sat there until the 10 s request timeout
  fired. Fix: `hello()` now calls `request(conn, req, 0)`, pinning the Hello frame's id
  to 0 explicitly (see the comment above `nextId` in `daemon-harness.mjs`).
- Separately, the harness was hardened to spawn its daemon on an **isolated** socket
  (a fresh `mkdtemp` dir with its own `XDG_RUNTIME_DIR`) instead of the real user path
  (`/tmp/bpa-<uid>` or `$XDG_RUNTIME_DIR/bpa`), and to tear down the spawned daemon's
  whole process group (`killProcessGroup` in `daemon-harness.mjs`) plus the temp dir on
  every exit path — success, a failed assertion, or a timeout — via a `finally` block
  in `survive-restart.mjs`. A prior failed run (while phase0 was still broken) had left
  a stray `bpa-sessiond` running on the real socket path; this closes that gap.

With both fixes, `npm run e2e:survive` passed all phases (`phase0` through `phase4b`,
`ALL PHASES PASSED`, exit code 0) against a real `cargo build -p bpa-sessiond` binary,
and `pgrep -fl bpa-sessiond` found nothing left running afterward. The bincode codec
itself (the part validated by re-deriving it from `crates/protocol/src/lib.rs` /
`framing.rs` at authoring time) needed no changes at that point — the `SessionLifecycle`
dual-codec handling (`decLifecycle` reads a length-prefixed JSON **string**, not a raw
`u32` enum discriminant — see the "Dual-codec note" in `crates/protocol/src/lib.rs`)
was correct from the start; the bug was the handshake request id, not frame encoding.

### v2 wire migration (preamble + CBOR) + phase5 daemon-restart rehydration

The retired v1 wire (bincode 1.3.3 + a `Hello`/`Welcome` framed handshake, magic
`0x42504131` "BPA1") was replaced with the v2 wire (Pv2 §4.2/§4.3): a codec-agnostic
raw-bytes preamble handshake (magic `0x42504141` "BPAA") ahead of a standard CBOR
frame stream. `daemon-harness.mjs`'s bincode encoder/decoder and `hello()` helper were
deleted outright and replaced with `preambleHandshake()` + a hand-rolled minimal CBOR
codec (`cborEncode`/`cborDecode` + the `Frame`/`Request`/`Response`/`Push` shape
mapping) — no new dependency; `cbor-x` was considered but its default non-standard
record/tag-105 extension is rejected by `ciborium`, and the shape surface here
(externally-tagged enums, camelCase nested structs, `Vec<u8>`-as-array) is small
enough that a ~350-line hand-rolled codec (matching the harness's existing
hand-rolled-codec philosophy) avoids that whole class of interop risk.

One real bug surfaced and was fixed during this migration: the first working draft of
`readExactly()` (the preamble reply reader) used `sock.unshift()` to push back any
bytes read past what the CURRENT call needed, so the NEXT `readExactly` call could
pick them up. This is unsafe once a `data` listener has already put the stream into
flowing mode — the unshifted bytes re-emit as a fresh `data` event on a later tick,
and if the next `readExactly` call's own listener isn't attached yet (it wasn't; the
`await` between the header read and the build-bytes read gives the event loop a tick
to fire the re-emission first), the event fires with nothing listening and the bytes
are lost, hanging the caller forever. Fixed by threading one shared mutable buffer
`state` across both `readExactly` calls in `preambleHandshake` instead of round-tripping
data through the socket. See `readExactly`'s doc comment in `daemon-harness.mjs`.

**Phase5 (daemon-restart rehydration, BL-7) was added.** Its second assertion
(reattach-after-restart replays scrollback) was initially blocked by a daemon
architecture gap (see the "History note" in §1 above); once the daemon's
cold-rehydrate change landed, phases 0-5 all pass unmodified (`ALL PHASES PASSED`,
exit code 0).

Build the daemon and run the harness with:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
cargo build -p bpa-sessiond --bin bpa-sessiond
npm run e2e:survive
```
