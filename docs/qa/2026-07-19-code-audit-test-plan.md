# Code audit + test plan — 2026-07-19

Full-tree QA audit of Builder Pro AI (Tauri + React/TS + Rust workspace) and the plan for the
fix loop. Produced by a five-surface parallel audit (frontend logic, frontend components, Rust
daemons, Rust core/security, Tauri bridge + test-infra), each verified against the working tree.

## 1. Baseline (measured, quiet load)

| Gate | Result |
|------|--------|
| English-only (`check-english.sh`) | PASS |
| `cargo test --workspace` | 805 ok / **1 flaky-fail** (openpty resource exhaustion under parallel oversubscription) — passes isolated |
| clippy `-D warnings` | PASS |
| `cargo fmt --check` | PASS |
| vitest | **925 pass** / 57 files |
| `tsc --noEmit` | PASS |
| ts-rs parity (`types.ts`, `orchd-types.ts`) | IN-SYNC |
| coverage gate (sessiond+orchd ≥80%) | flaked under load (same openpty cause); passes when not oversubscribed |
| e2e survive-restart | ALL PHASES |
| e2e orchd survive+roundtrip | ALL PHASES |

**The one thing making the suite non-deterministic is the openpty flake** — it breaks
`cargo test --workspace` and the coverage gate whenever the machine is under load (BL-40 class).
Fixing it is prerequisite #1 for a reliable "run tests in a loop" workflow.

## 2. Findings (by disposition)

Severity: P1 correctness/security/data-loss · P2 robustness/ops · P3 polish.
Disposition: **FIX** (this loop, TDD) · **BACKLOG** (honest deferral, new/existing BL row).

### 2.1 FIX — test-infra reliability
- **A1** [P2→gate-blocking] `crates/sessiond/src/pty_supervisor.rs:259` `Supervisor::create` — `openpty`
  has no retry on transient OS resource exhaustion (EAGAIN/ENFILE/EMFILE-class, macOS bounded pty
  count). Panics the PTY-spawning tests under load; also a real hard-failure for a user spawning
  many terminals. FIX: bounded `retry_transient(max, backoff, op)` helper (lock released between
  attempts) + deterministic unit test.

### 2.2 FIX — backend correctness/security
- **B1** [P2] `crates/orchd/src/connectors/adapter.rs:176` — `invoke` buffers the full upstream
  response (`response.json::<Value>()`) with no size cap → OOM DoS of the whole orchd process from
  an untrusted-class connector body. FIX: stream with a byte cap → typed `ConnectorError` + test.
- **B2** [P2] `crates/orchd/src/mcp/registry.rs:104` — `validate_new_server` never floors
  `timeout_ms`; `timeout_ms:0` → `timeout(ZERO)` → every connect/tool-call instantly fails and the
  server is silently bricked. FIX: floor to ≥1000ms in validate + apply_patch + test.
- **B3** [P3] `crates/orchd/src/connectors/accounts.rs:348,463` — `expires_at = now + duration`
  can overflow i64 on an absurd `expires_in`. FIX: `saturating_add` + test.
- **B4** [P3] `crates/orchd/src/graph.rs:861` — `GraphSearch` binds `%{query}%` to LIKE without
  escaping `%`/`_` → wrong substring matches. FIX: escape + `ESCAPE '\'` + test.
- **B5** [P3] `crates/mcp/src/error.rs:46` — `AUTH_MARKERS` matches bare `401`/`403` substrings
  anywhere → benign transport errors misclassified as auth. FIX: tighten to status context + test.
- **B6** [P3-hardening] `crates/protocol/src/framing.rs:92` — CBOR decoder has no nesting-depth
  guard; a crafted ≤16 MiB frame could recurse (ignored-any skip path). Socket is 0700/0600
  (same-user only) so real severity is low, but add a negative test; add a depth pre-check if clean.

### 2.3 FIX — frontend correctness/security
- **C1** [P1] `src/store/store.ts:646` `reportError` — stores `describeOrchdError()` output as
  `event.message` **unscrubbed** (only `detail` is scrubbed; `recordRenderCrash` scrubs both), so a
  raw daemon message with a `/Users/<name>` path or embedded key leaks into the copyable support
  bundle (`toSupportBundle` assumes events are pre-scrubbed). FIX: `scrubSecrets(message)` + test.
- **C2** [P1/P2] `src/App.tsx:448` + `src/components/FilesRail.tsx:88` — `startWorkspaceWatch()`
  fire-and-forget with no `.catch`; a rejected watch-start is an unhandled rejection and leaves
  `watchPaused=false` (UI falsely reads "live"). FIX: `.catch` → `setWatchPaused(true)` + tests.
- **C3** [P1] `src/components/idea/FormInsightDialog.tsx:248` — `handleCreate` creates the insight
  then sets the fit-verdict with no created-id guard; a verdict-failure retry duplicates the
  insight (sibling flows guard this). FIX: hold created id, skip re-create on retry + test.
- **C4** [P2] `src/store/store.ts:515` `setLifecycle` — a late `state-changed` after `exited`
  resurrects a dead session (can re-set waiting). FIX: no-op when existing lifecycle is exited + test.

### 2.4 FIX — test-gap fills (no prod change / paired with above)
- **D1** toSupportBundle / reportError no-secret test (pairs with C1).
- **D2** `recordRenderCrash` scrub + no-toast test.
- **D3** `onDaemonIncompatible` event subscription test (its orchd twin is tested; this fatal one isn't).
- **D4** `setTheme` store action + `initTheme` system-listener (matchMedia change) tests.
- **D5** `TerminalPane` component test (component has no test file).
- **D6** `ConnectDialog` component test (no test file; MCP trust choke-point — consent-before-connect).

### 2.5 FIX — CI/e2e honesty
- **E1** [P2] `tests/e2e/orchd-survive.mjs` phase7 has SKIP branches that still exit 0; CI never
  asserts it ran (unlike BL-16 shell tests). FIX: assert a "phase7 OK" marker / fail on SKIP.
- **E2** [P2] No regression guard that `tauri.conf.json` CSP stays non-null. FIX: capabilities test
  asserting `app.security.csp` is a non-null object locking `script-src 'self'` (pairs with F1).

### 2.6 FIX (higher-risk, runtime-verified) — security posture
- **F1** [P1] `src-tauri/tauri.conf.json:23` `csp: null` (BL-2) — no CSP; the root mitigation for
  injected-content confused-deputy. Feasible: **no `dangerouslySetInnerHTML`/`innerHTML` anywhere**,
  index.html has only an external module script; only inline *styles* need `'unsafe-inline'`. FIX:
  restrictive CSP + E2 regression test + **real-app smoke** (verify the window loads and IPC works).

### 2.7 BACKLOG — honest deferral (new BL rows + rationale)
- **fs_explorer webview-supplied `root` allowlist** (`fs_explorer.rs:478`, agent P1) — real
  confused-deputy in theory, but **no HTML-injection sink exists** and F1 (CSP) removes the
  execution vector; a registered-roots allowlist needs State injection into today's stateless
  pure-local commands (architectural). Add negative tests documenting current behavior; backlog the
  allowlist as defense-in-depth to land with the fs-command-hardening slice. → **BL-102**
- **a11y focus-trap + APG keyboard model** across all modals/tabs/tree (FE-components agent, ~10
  findings) — a cross-cutting UX epic, not a drive-by; extends BL-29. → **BL-103**
- **Coverage gate widening** to the Tauri bridge crate (commands.rs/fs_explorer.rs) — infra. → **BL-104**
- **`sign-verify.sh` wired into `release.yml`** (artifact signature/notarization verify). → **BL-105**
- Existing rows already covering audit items: BL-2 (CSP — being taken by F1), BL-3 (db 0600),
  BL-10 (reader→forwarder bounded bridge), BL-34 (stale-daemon build compare), BL-40 (PTY test
  determinism — A1 addresses the openpty half), BL-50 (symlink no-follow delete/rename).

## 3. Loop protocol

For each FIX task: **RED** (write a test that fails for the right reason, run it, confirm the
failure) → **GREEN** (minimal implementation, run, confirm pass) → re-run the affected gate
(vitest / `cargo test -p <crate>` / clippy) → conventional commit. After each group, re-run the
broader suite. At the end: full `scripts/final-suite.sh` must print `ALL GATES PASSED`, plus a
loaded-condition re-run of `cargo test --workspace` to prove the flake is gone. Work lands on a
branch (`qa/audit-fixes-2026-07-19`), not directly on main.

Order: A1 (unblock the gate) → B1–B6 → C1–C4 → D1–D6 → E1–E2 → F1 (runtime-verified) → backlog rows.
