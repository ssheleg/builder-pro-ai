# Builder Pro AI — Protocol v2 Design (Cycle 2)

**Date:** 2026-07-06
**Status:** unfrozen — awaiting owner review → implementation cycle (amended 2026-07-06 for vision v2–v4; see "Vision v2–v4 amendments" below)
**Parent slice:** S0+S1 (merged @ `285cb2e`), docs-truth/CI pass (merged @ `45891b1`)
**Seed:** [`2026-07-04-docs-truth-ci-fix-pass-design.md` §8](2026-07-04-docs-truth-ci-fix-pass-design.md); overview [`Protocol evolution & upgrade policy`](2026-07-01-builderpro-platform-overview.md)
**Context7 verification (2026-07-06):** ciborium/CBOR is self-describing (tagged enums via plain
`#[derive]`); bincode 2 (`decode_from_slice` + `serde` feature) and postcard both remain
**non-self-describing** (`deserialize_any` → `WontImplement`/unsupported), so neither fixes the
tagged-enum limitation. CBOR is therefore the only codec that retires the dual-codec hack.

---

## 0. Locked decisions

| # | Decision | Choice |
|---|----------|--------|
| D3′ | Wire codec | **CBOR via `ciborium`** (self-describing, IETF RFC 8949). Replaces bincode 1.3.3 + the JSON-in-bincode dual-codec bridge. |
| — | Handshake | **Codec-agnostic fixed preamble** (raw LE bytes), then CBOR frames — so version negotiation survives any future codec change. |
| — | v1→v2 transition | **Hard break for LIVE sessions** (unavoidable: upgrading the daemon binary = restart = PTYs die; SCM_RIGHTS handoff explicitly out of scope for v0.x). Records + scrollback rehydrate as inactive. Made non-silent by the upgrade-consent dialog (D4). |
| D5 | Multi-subscriber attach | **Wire + daemon only** this cycle; GUI stays a single subscriber (no co-view UI consumer until S6). |
| D4 | Upgrade UX | **Full consent dialog** + real `DaemonShutdown{drain}` + `launchctl kickstart -k`. Permanent mechanism (serves every future bump). |
| — | `command_events` | **Schema v2 adds the table AND the daemon writes it now** (from already-parsed OSC-133 C/D marks). No UI. |
| — | Daemon supported version range | `[2, 2]` now (clean break). The negotiation machinery is built for `[2, 3]`-style ranges in future cycles. |

**Non-goals (explicit):** GUI co-view UI (S6); Tauri auto-updater (BL-19); PTY-fd handoff for
live-session survival across upgrade; agent identity tokens (S6). No S1 behavior changes beyond
what's listed here.

---

## 1. Goals / non-goals

**Goals.**
1. Retire the dual-codec bridge: `SessionLifecycle` / `TerminalEvent` become plain
   `#[derive(Serialize, Deserialize)]` tagged enums, encoded natively by CBOR — the spec matches
   the code, the never-re-derive contract box is deleted.
2. Robust, future-proof version negotiation: a codec-agnostic preamble carrying a client version
   range; the daemon answers with the chosen version or a typed `Incompatible`.
3. Honest daemon-upgrade choreography with user consent + real drain.
4. Multi-subscriber attach at the wire and daemon: N independent subscribers per session, each
   with its own Replay and backpressure.
5. Schema v2 with a `command_events` history table, written best-effort from OSC-133 marks.
6. Every change is TDD-covered incl. cross-version decode tests and a fail-closed migration test;
   all 8 gates + CI stay green.

**Non-goals:** see §0.

---

## 2. Architecture (what changes, what doesn't)

Unchanged: two-process split (webview ⇄ core ⇄ daemon), Unix-domain-socket transport, launchd
supervision, `u32`-LE length-prefixed framing for post-handshake frames, the ts-rs → `types.ts`
pipeline, the `bpa-paths` validation crate, all S1 PTY/OSC/persistence behavior.

Changed, by crate:
- `crates/protocol` — codec (bincode→ciborium), preamble types, version range constants, deletion
  of the dual-codec impls, `DaemonShutdown` drain semantics doc, additive attach multiplicity.
- `crates/sessiond` — supervisor fan-out (`Option<Sink>`→`Vec<Sink>`), attach registry
  multi-subscriber, socket-server preamble handshake + negotiation + real drain, persistence
  schema v2 + `command_events` writer.
- `src-tauri` — socket-client preamble handshake + negotiation, stale/incompatible-daemon
  detection, the upgrade-consent command + dialog + `kickstart -k` + reconnect.
- `src/` — one new `#[tauri::command]` surface for the upgrade flow + a minimal banner/dialog
  (no terminal-render changes; GUI remains a single subscriber).

---

## 3. Codec: CBOR via ciborium (§1 goal 1)

### 3.1 Framing (`crates/protocol/src/framing.rs`)

Post-handshake frames keep the exact envelope: `u32`-LE length prefix + body, `MAX_FRAME_LEN`
= 16 MiB, `FrameDecoder` buffering partial frames across reads, `Oversized` rejection without
allocation. Only the body codec changes:

```rust
// encode_frame:
let mut body = Vec::new();
ciborium::into_writer(frame, &mut body).map_err(|e| FrameError::Encode(e.to_string()))?;
// ... length check + u32-LE prefix (unchanged) ...

// decode (inside FrameDecoder::decode, per complete body):
let frame: Frame = ciborium::from_reader(&body[..]).map_err(|e| FrameError::Decode(e.to_string()))?;
```

`FrameError` variants unchanged (Oversized / Decode / Encode); the doc comment updates from
"bincode 1.3.3" to "CBOR (ciborium)".

### 3.2 Tagged enums become plain derives (`crates/protocol/src/lib.rs`)

DELETE the hand-written `impl Serialize/Deserialize for SessionLifecycle` and
`… for TerminalEvent` (the `is_human_readable()` branches + `*Shape` intermediary structs +
`serde_json` tunneling). Replace with the logical derive that ts-rs already documents:

```rust
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]   // internally tagged (per ts_export test name)
pub enum SessionLifecycle { AtPrompt, Typing, Running, Exited { code: Option<u8>, signal: Option<String> } }
// TerminalEvent: adjacently tagged (per `terminal_event_is_adjacently_tagged_…` test) — the
// implementer copies the EXACT #[serde(tag=…, content=…, …)] attributes that today's
// dual-codec `*Shape` structs carry, so the generated types.ts is unchanged.
```

**Contract (do NOT guess the attributes):** copy the exact `serde` tag/content/rename attributes
from the CURRENT `SessionLifecycleShape` / `TerminalEventShape` intermediary structs (the shapes
the dual-codec already serializes) onto the real enums, then delete the `*Shape` structs and the
hand-written impls. The `ts_export` parity test is the gate — any `src/ipc/types.ts` diff fails the
task, so the shape is locked by test, not by this snippet. `serde_json` stays in `crates/protocol`
ONLY if still used elsewhere (broker Channel path); otherwise remove it. Delete the never-re-derive
contract box from spec §3, CONTRIBUTING, and architecture.md in the SAME cycle (false once CBOR lands).

### 3.3 Call-site sweep

All 7 `bincode::` call sites (`crates/sessiond/src/socket_server.rs`,
`src-tauri/src/{socket_client,commands}.rs`, protocol crate + its tests) move to ciborium via the
`encode_frame`/`FrameDecoder` helpers (tests that hand-rolled `bincode::serialize` switch to
`encode_frame`). `bincode` is removed from both crates' `Cargo.toml` and the workspace deps.

---

## 4. Handshake preamble + version negotiation (§1 goal 2)

### 4.1 The problem this fixes

v1's handshake was itself a bincode-encoded `Hello` frame. A peer on a different codec cannot even
decode it, so it can never reply `Incompatible` — negotiation is impossible across a codec change.
Fix: the FIRST bytes on every connection are a **fixed, codec-independent preamble**; only after a
version is agreed does the CBOR frame stream begin.

### 4.2 Wire format (raw, little-endian, never changes)

Client → daemon, immediately on connect:
```
magic:        u32 LE   = 0x4250_4141  ("BPAA" — protocol-Agnostic preAmble; distinct from the
                                        old per-frame MAGIC so a v1 daemon can't misread it as a frame)
client_min:   u16 LE            // lowest protocol version the client speaks
client_max:   u16 LE            // highest
build_len:    u16 LE            // length of the build string that follows (<= 256)
build:        [u8; build_len]   // client build string, UTF-8
```
Daemon → client reply:
```
magic:        u32 LE   = 0x4250_4141
result:       u8       // 1 = Accepted, 0 = Incompatible
// if Accepted:
chosen:       u16 LE            // the negotiated protocol version (highest common)
build_len:    u16 LE; build: [u8; build_len]   // daemon build string
// if Incompatible:
daemon_min:   u16 LE
daemon_max:   u16 LE
```
Useful property (clean v1→v2 detection): `PREAMBLE_MAGIC` stored LE is bytes `41 41 50 42`; a v1
daemon reads those 4 bytes as its `u32`-LE frame-length prefix = `0x4250_4141` ≈ 1.06 GB, which
exceeds `MAX_FRAME_LEN` (16 MiB) → the v1 `FrameDecoder` rejects it as `Oversized` and closes the
connection. So a v2 client hitting a stale v1 daemon sees a clean connection close (→
`IncompatibleDaemon`, §4.5), never a hang or a misparse — the codec break detects itself.

Negotiation rule: `chosen = min(client_max, daemon_max)`; Accepted iff
`max(client_min, daemon_min) <= chosen`, else Incompatible. After Accepted, both sides use the
codec + frame schema bound to `chosen` (today `chosen == 2` ⇒ CBOR). The preamble reader enforces
`build_len <= 256` and a total-preamble byte cap (fail closed on a garbage/oversized preamble,
mirroring `MAX_FRAME_LEN`). All preamble I/O has a bounded read timeout so a silent/stuck peer
can't hang a connection.

### 4.3 Protocol constants (`crates/protocol/src/lib.rs`)

```rust
pub const PREAMBLE_MAGIC: u32 = 0x4250_4141; // "BPAA"
pub const CLIENT_MIN_VERSION: u16 = 2;
pub const CLIENT_MAX_VERSION: u16 = 2;
pub const DAEMON_MIN_VERSION: u16 = 2;
pub const DAEMON_MAX_VERSION: u16 = 2;
```
The old `Request::Hello` / `Response::Welcome` / `Response::Incompatible` frame variants are
REMOVED (superseded by the preamble); the old per-frame `MAGIC`/`PROTO_VERSION` constants are
removed. `encode_preamble_*` / `decode_preamble_*` helpers live in a new
`crates/protocol/src/preamble.rs` (own module, own tests), keeping `framing.rs` focused on frames.

### 4.4 Daemon side (`crates/sessiond/src/socket_server.rs`)

`handle_client` first reads + validates the client preamble (magic, range, build cap, timeout),
computes negotiation, writes the daemon reply. On Incompatible → write reply, close. On Accepted →
proceed to the CBOR frame dispatch loop (the rest of `handle_client` is unchanged except the codec).

### 4.5 Core side (`src-tauri/src/socket_client.rs`)

The client writes its preamble, reads the daemon reply. On Accepted → normal operation. On
Incompatible OR (garbage reply / connection closed during handshake / timeout) → surface a typed
`ClientError::IncompatibleDaemon { daemon_min, daemon_max }` (unknown ranges when the daemon just
closed) — this is what triggers the upgrade flow (§6). Bounded backoff reconnect still applies for
plain "daemon not up yet".

---

## 5. Multi-subscriber attach (§1 goal 4; D5)

### 5.1 Supervisor fan-out (`crates/sessiond/src/pty_supervisor.rs`)

Today `Shared.sink: Mutex<Option<Sender<Vec<u8>>>>` holds ONE live consumer. v2:
`Shared.sinks: Mutex<Vec<(u64 /* sub_id */, Sender<Vec<u8>>)>>`. The reader thread, on each chunk,
sends to EVERY sink and prunes any whose `send` fails (receiver dropped). `subscribe_output` gains
a `sub_id` and PUSHES a sink (does not replace); a new `unsubscribe_output(session_id, sub_id)`
removes one. The `!is_active` guard (a session whose reader already exited) still refuses new
subscriptions (returns `NoSuchSession`), preserving the TOCTOU fix — the check + push happen under
the `sinks` lock (same lock discipline as the current fix).

### 5.2 Attach registry (`crates/sessiond/src/attach.rs`)

`entries: StdMutex<HashMap<SessionId, AttachEntry>>` → keyed by **`(SessionId, u64 conn_id)`**
(a session now has 0..N entries, one per attached connection). Each entry still owns its forwarder
`JoinHandle` + cancel flag + `sub_id`.
- `attach(conn_id, session_id, sink)`: allocate a `sub_id`, `subscribe_output` (push), snapshot
  scrollback, send that subscriber's OWN `Replay`, spawn its forwarder, insert `(session_id,
  conn_id)`. **No supersede** — attaching from another connection no longer stops existing ones.
- `detach(conn_id, session_id)`: remove `(session_id, conn_id)` only; `unsubscribe_output`; cancel
  that forwarder. Immediate (discard, like today's detach).
- `detach_all_for_conn(conn_id)`: remove every entry whose key's conn_id matches (that client
  disconnected) — already the shape today, now naturally multi-session.
- `remove_session(session_id)`: session ENDED — GRACEFULLY drop ALL its subscribers (the reader
  drops all sinks on exit; each forwarder drains to `Disconnected` then terminates — the graceful
  path from the truncation fix, applied per-subscriber).
- `detach_all()`: shutdown drain — all entries, all sessions.
- `attachment_count()` stays (now counts subscriber entries).

### 5.3 Dispatch (`socket_server.rs`)

`AttachSession`/`DetachSession` already carry `conn_id` (from the multi-client fix). Semantics
change from single-attach-supersede to additive multi-subscriber. `Push::Replay`/`Push::Output`
already fan out per-subscriber via each forwarder's own sink — no wire-shape change; the multiplicity
is a daemon behavior. Document in the spec that attach is now many-to-one (N connections : 1 session).

### 5.4 Tests (the D5 proof)

Two real socket clients A+B both `AttachSession` the same live session: BOTH receive their own
`Replay` then live `Output`; A `DetachSession` → B keeps streaming; A disconnects → B unaffected;
`KillSession` → both drained, both get `ChildExited`, both forwarders terminate (no leak). Plus the
existing single-client attach tests stay green (one subscriber is the N=1 case).

---

## 6. Real drain + upgrade choreography (§1 goal 3; D4)

### 6.1 `DaemonShutdown{drain}` real semantics (`socket_server.rs` + `boot.rs`)

Today: no-op `Ack`. v2: on `DaemonShutdown{drain:true}` the daemon (a) stops accepting new
connections, (b) flushes scrollback for all live sessions to SQLite (best-effort), (c) flushes any
pending `command_events`, (d) sends a final `Push` / the `Ack`, then (e) triggers a graceful
process exit (same path as SIGTERM). `drain:false` → immediate exit, no flush. Because the daemon
is launchd-managed with `KeepAlive{Crashed}`, a clean self-exit is NOT a crash → launchd does not
auto-restart; the upgrade flow explicitly `kickstart`s the replacement.

### 6.2 Core upgrade flow (`src-tauri/src/`)

New `#[tauri::command] upgrade_daemon()` and the detection→consent→act sequence:
1. **Detect:** `socket_client` raises `IncompatibleDaemon` (§4.5) when it meets a stale/older
   daemon after an app update. The core does NOT silently kill it.
2. **Consent:** the webview shows a dialog — *"Update background service — N live sessions will
   end. Their records and scrollback are saved and will reappear as inactive."* where N = the last
   known live-session count (from the last successful `list_sessions`, else 0). The user confirms
   or cancels (cancel → stay disconnected, banner persists honestly).
3. **Act (on confirm):** best-effort `DaemonShutdown{drain:true}` over whatever connection exists
   (may fail if the old daemon can't parse the v2 frame — that's fine); then
   `launchctl kickstart -k gui/<uid>/ai.builderpro.desktop.sessiond` (force-restart with the new
   bundled binary); then reconnect (now negotiates v2) and `list_sessions` → rehydrated inactive
   sessions appear.
4. **Honest failure:** if kickstart fails (TCC/permission), surface the actionable banner from §8.3
   — never a fake "connected".

### 6.3 Frontend (`src/`)

A minimal dialog component + a store flag (`daemonIncompatible: bool`) + wiring
`daemon://incompatible` (new core event) → dialog. No terminal-render changes. Reuses the existing
error-surfacing pattern (the contract from the docs cycle, spec §13).

---

## 7. Schema v2 + `command_events` (§1 goal 5)

### 7.1 Migration (`crates/sessiond/src/persistence.rs`)

`SCHEMA_VERSION 1 → 2`. `migrate(from_version)` already: fail-closed, single transaction,
`from_version > SCHEMA_VERSION` rejected (newer-db + older-daemon → actionable error, which the
§6 upgrade flow resolves). Add the v1→v2 step (runs when `from_version == 1`):

```sql
CREATE TABLE command_events (
  session_id TEXT NOT NULL REFERENCES session(id),
  seq        INTEGER NOT NULL,          -- monotonic per session
  ts         INTEGER NOT NULL,          -- unix seconds
  kind       TEXT NOT NULL,             -- 'started' | 'finished'
  exit_code  INTEGER,                   -- present iff kind='finished' and known
  PRIMARY KEY (session_id, seq)
);
```
Forward-migrate-only policy is documented (already the built behavior). A fresh v2 db creates the
table directly; a v1 db upgrades in-place preserving existing rows.

### 7.2 Writer (best-effort, from OSC-133)

The OSC parser already extracts command lifecycle (C = command started, D = finished + exit code —
`osc_parser.rs`). On each such event the daemon appends a `command_events` row (monotonic `seq` per
session), best-effort like scrollback: a DB failure is logged and swallowed, never stalls the PTY.
Retention: `command_events` rows are deleted with their session (cascade) under the BL-4 retention
work; for this cycle, purge-on-session-delete is wired if session delete exists, else the rows
follow the session row's lifetime (documented).

### 7.3 Tests

Migration: a v1 db (built with only session/scrollback/workspace) opened by a v2 daemon gains
`command_events` and keeps its rows; a v2 db opened again is a no-op; a db with `user_version=3`
fails closed. Writer: a session that runs one command produces a `started` then a `finished` row
with the right exit code (drive a real `/bin/sh -c` with a known exit).

---

## 8. Error handling & honest degradation

- **Handshake:** bounded read timeout on the preamble both sides; garbage/oversized preamble →
  fail closed + disconnect (no allocation of a bogus build string). Incompatible → typed error,
  never a misparse.
- **Codec:** a body that fails CBOR decode → `FrameError::Decode` → disconnect that client (never
  a partial-apply). CBOR's self-describing nature means a type mismatch is a clean decode error,
  not silent corruption.
- **Multi-subscriber:** each subscriber has its own bounded outq; a slow/dead subscriber is dropped
  independently and never stalls another subscriber or the PTY (extends the existing per-client
  isolation).
- **Upgrade:** every step is best-effort with an honest fallback (drain may fail → still kickstart;
  kickstart may fail → actionable banner). Never a fake connected/updated state.
- **Migration:** fail-closed in a transaction; newer-db → actionable error routed to the upgrade
  flow.
- **Logging:** structured, no secrets (unchanged rules); the preamble build strings are non-secret.

---

## 9. Testing (TDD) & Definition of Done

New/changed test coverage (all TDD, RED first):
1. **Codec roundtrip:** every `Frame`/`Request`/`Response`/`Push` variant + every `SessionLifecycle`
   and `TerminalEvent` variant round-trips through ciborium (replaces the bincode roundtrip tests);
   the `ts_export` parity test proves the TS shape is byte-identical to v1 (no `types.ts` diff).
2. **Preamble/negotiation:** accepted (equal ranges), chosen = min(maxes), incompatible (disjoint
   ranges), oversized/garbage preamble rejected, timeout on a stuck peer.
3. **Cross-version decode:** a v2 daemon fed a v1-style (bincode) preamble/frame rejects cleanly
   (no panic, no misparse) — the audit's A3 requirement.
4. **Multi-subscriber:** §5.4 two-client scenarios.
5. **Real drain:** `DaemonShutdown{drain:true}` flushes scrollback + command_events then exits;
   `drain:false` exits without flush.
6. **Migration:** §7.3.
7. **Upgrade flow:** core-side unit tests for detection→typed error→command surface (the Tauri
   command's request-building/`kickstart` invocation shape, against a stub — mirroring the existing
   `commands_over_stub_daemon` pattern); the launchctl call uses the existing mockable runner.
8. **E2E:** `e2e:survive` updated to the v2 preamble/codec (the harness speaks the wire); it must
   stay green. A new e2e phase for daemon-restart rehydration (closes BL-7) is IN SCOPE here since
   the drain path is being built.

DoD: all 8 `final-suite.sh` stages green (incl. clippy `-D warnings`, fmt, tsc, coverage ≥80%,
e2e); CI green; docs updated in the same cycle (spec §3/§5/§15 codec box DELETED; overview protocol
section marks v2 as shipped; README/architecture/traceability trued up; backlog BL-7 → done, BL-11
"escaped-descendant" re-evaluated under multi-subscriber; CHANGELOG `[0.2.0]`).

---

## 10. Execution shape (input to writing-plans)

- Isolated worktree branch off `main`.
- Suggested task groups (file-ownership = parallel-safety):
  - **G1 (sequential, contracts first):** protocol crate — preamble module + constants + codec
    swap + tagged-enum derives + roundtrip/preamble tests. Everything downstream depends on the
    locked wire.
  - **G2 (parallel after G1):** daemon handshake+negotiation+drain (socket_server/boot); daemon
    multi-subscriber (pty_supervisor + attach); daemon schema v2 + command_events (persistence +
    osc wiring). Disjoint files.
  - **G3 (sequential):** core socket_client preamble+negotiation+incompatible detection; then core
    upgrade command + frontend dialog/store (depends on the core error type).
  - **G4:** e2e harness → v2 wire + new daemon-restart phase; docs truth pass (delete codec box,
    mark v2 shipped, close BL-7); CHANGELOG.
  - **G5:** whole-branch review (adversarial: cross-version safety, multi-subscriber races, drain
    correctness, migration fail-closed) → merge.
- Every task: TDD, verifiable DoD, two-stage review. Context7 re-checked for ciborium API at plan
  time (into_writer/from_reader signatures, no_std features we don't need, MSRV vs 1.92).

## 11. Risks & mitigations

- **CBOR frame-size growth** (field names in every map): negligible on a local UDS at S1
  throughput; if profiling ever shows it, CBOR supports integer keys — a future, additive
  optimization, not now.
- **ciborium MSRV / API drift:** pinned + Context7-verified at plan time; a roundtrip test guards
  behavior.
- **Multi-subscriber reader-thread cost:** fan-out is O(subscribers) per chunk under the sinks
  lock; subscribers are few (GUI = 1, agents later). Prune-on-send keeps it bounded.
- **v1→v2 in the wild:** only affects an installed v0.1 with live sessions updated to v0.2; the
  consent dialog makes the session loss explicit and records survive. Documented in the survival
  truth table already.

---

## Vision v2–v4 amendments (2026-07-06)

The vision-alignment pass introduced `bpa-orchd` (ADR-HOST) and the workflow/agent/MCP layers.
Three additive adjustments keep Pv2 from foreclosing them — none changes the codec/negotiation/
attach/drain design above; all are append-only.

1. **Pv2.1 additive-request batch (reserved, NOT built now).** The workflow engine and agents
   will need terminal capabilities beyond S1's set. Reserve them in the append-only `Request`
   variant order so they slot in later without another wire break: `command+argv spawn` (no shell
   wrapping), `typed exit-status wait`, `ReadOutput { since_seq }` (cursor read of scrollback),
   and a rendered-text snapshot (grid as text). This spec only NAMES and orders them; they are
   implemented in the slice that needs them (SW1/S6b). Recorded so the variant indices are not
   reused.

2. **`command_events` attribution hook.** The schema-v2 `command_events` table (§7) gains an
   attribution column now — which actor / workflow-run / step a command belongs to
   (nullable `origin` text, e.g. `gui`, `run:<uuid>#<step>`). Additive; lets S7/S8 roll up
   per-run cost and history later without a second migration. The daemon writes `gui` by default;
   `bpa-orchd`-driven sessions pass their run/step id at `CreateSession` (additive optional field
   on the create request — reserved with the Pv2.1 batch).

3. **Second core-side client = `bpa-orchd`.** The codec-agnostic handshake preamble + version
   negotiation (§4) must not assume a single client identity: both the GUI and `bpa-orchd`
   connect concurrently (multi-subscriber attach, §5, already supports co-attach). No wire change
   — a note that the preamble carries no "there is exactly one client" assumption, and peer-cred
   (§8) admits any same-uid client.
