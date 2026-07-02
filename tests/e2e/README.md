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

`survive-restart.mjs` re-implements the exact bincode 1.3.3 (fixint, little-endian)
encoding for the Hop-B wire protocol (spec §7) directly in Node — no Rust bridge, no
test-runner dependency, no WKWebView. It drives a locally-spawned `bpa-sessiond`
through the full lifecycle:

- **phase0** — spawn the daemon on a scratch socket **isolated from the real user socket**
  (a fresh `mkdtemp` dir with its own `XDG_RUNTIME_DIR`, so it can never collide with — or
  during cleanup, signal — an actual running daemon at `/tmp/bpa-<uid>` or
  `$XDG_RUNTIME_DIR/bpa`), connect, `Hello` → `Welcome` handshake. The `Hello` request is
  sent with the fixed request id `0` (spec §7) — the daemon's handshake gate
  (`crates/sessiond/src/socket_server.rs`) matches the first frame as literally `id: 0` and
  always replies `Response { id: 0, .. }` for it; any other id on that first frame silently
  desyncs response correlation and manifests as `request Hello timed out`.
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
  - open a **fresh** client connection (simulating app relaunch), `Hello` again,
    `ListSessions` and assert the session is still listed;
  - `AttachSession` again and assert the `Replay` push's `content` still contains the
    marker written in phase2 (scrollback survived the "restart").
  - clean up: `SIGTERM` the daemon it spawned, remove the temp workspace dir.

No assertion is weakened to pass vacuously — a missing daemon binary, a broken
handshake, absent lifecycle pushes, or lost scrollback each fail with a specific,
actionable message and a non-zero exit code.

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
[e2e] phase0 OK: handshake
[e2e] phase1 OK: session <uuid>
[e2e] phase2 OK: command output observed
[e2e] phase3 OK: OSC-133 lifecycle running -> atPrompt
[e2e] phase4a OK: daemon + shell survived client quit
[e2e] phase4b OK: reattach + scrollback intact
[e2e] ALL PHASES PASSED
```
exit code `0`.

### `tests/e2e/lib/daemon-harness.mjs`

The reusable client library: frame codec (`encodeFrame`/`decodeFrame`), a
length-prefixed-framing socket wrapper (`connect`, `request`, `nextPush`, `hello`),
and process probes (`pgrepDaemon`, `pgrepShell`, `pidAlive`, `spawnDaemon`,
`launchctlKickstart`, `killGui`). The variant orders transcribed into the codec are
commented with exactly which `crates/protocol/src/lib.rs` enum they must track
(`Frame`, `Request`, `Response`, `Push`) — if that file's variant order ever changes,
update the corresponding `u32le(N)` calls here to match, and the codec doc comments
name the enum to diff against.

One subtlety worth calling out explicitly: `SessionLifecycle` is **not** encoded as a
raw bincode enum discriminant on the wire. Per the dual-codec note in
`crates/protocol/src/lib.rs`, it has a hand-written `Serialize`/`Deserialize` that,
under a non-human-readable codec (bincode/Hop-B), serializes the tagged JSON shape
(`{"kind":"atPrompt"}`, `{"kind":"exited","code":0,"signal":null}`, …) into a plain
bincode `String`. `decLifecycle` in the harness therefore reads a length-prefixed
UTF-8 string and `JSON.parse`s it, rather than reading a `u32` variant index — get this
wrong and every `SessionMeta`/`StateChanged` decode desyncs the cursor on the very
first field after it.

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

With both fixes, `npm run e2e:survive` passes all phases (`phase0` through `phase4b`,
`ALL PHASES PASSED`, exit code 0) against a real `cargo build -p bpa-sessiond` binary,
and `pgrep -fl bpa-sessiond` finds nothing left running afterward. The bincode codec
itself (the part validated by re-deriving it from `crates/protocol/src/lib.rs` /
`framing.rs` at authoring time) needed no changes — the `SessionLifecycle` dual-codec
handling (`decLifecycle` reads a length-prefixed JSON **string**, not a raw `u32` enum
discriminant — see the "Dual-codec note" in `crates/protocol/src/lib.rs`) was correct
from the start; the bug was the handshake request id, not frame encoding.

Build the daemon and run the harness with:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
cargo build -p bpa-sessiond --bin bpa-sessiond
npm run e2e:survive
```
