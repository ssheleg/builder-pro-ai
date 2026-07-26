# Comprehensive QA report — 2026-07-24

> Branch `qa/comprehensive-audit-2026-07-24` (off `nightbuild` @ `f49c06d`). This is the master
> living document for the audit-and-test pass; Phase 0 results are final, later phases append.
> Companion to the existing `2026-07-19-code-audit-test-plan.md` + `ux-test-results.md` — it
> **verifies a fresh slice and fills gaps**, not redoes the 181-scenario / 101-investigation work.

## Baseline (measured this pass, quiet dev load, with the installed app's live daemons running)

| Gate | Result |
|------|--------|
| English-only (`check-english.sh`) | PASS (307 files, 24 allowlisted) |
| `cargo fmt --check` | PASS (after the Phase-0 edits were rustfmt'd) |
| clippy `--workspace --all-targets -- -D warnings` | PASS |
| `npx tsc --noEmit` | PASS |
| `npx vitest run` | PASS — **1130 tests / 63 files** |
| `cargo test --workspace` | **PASS — 0 failed** (after Phase-0 fixes; was RED before — see below) |

> **README/test-count drift (doc finding, non-blocking):** README claims Rust **1153** / TS **1070**
> tests (`0.10.0`); this pass measures vitest **1130** (TS grew, README undercounts) and the Rust
> count is ~1150 across binaries. Traceability.md is also frozen at `[0.8.0]` counts. Re-measure +
> refresh in the doc pass.

## Phase 0 — determinism stabilization (DONE, the prerequisite for any reliable test loop)

Before these fixes `cargo test --workspace` was **non-deterministic / hung on this machine** — it
could not be used to gate work. Three root causes, all fixed with RED→GREEN verification.

### BL-102 — connect-retry tests hit a LIVE daemon (test isolation) — FIXED
- **Symptom:** `connect_with_retry_gives_up_*`, `connect_with_retry_clamps_zero_attempts_*`,
  `connect_orchd_with_retry_gives_up_*` (`src-tauri/src/lib.rs`) failed because they resolved the
  socket path from the ambient env and connected to the **installed app's live daemons** bound at
  `/tmp/bpa-501/{d,orchd}.sock` — `connect()` succeeded, so the `result.is_err()` assertion failed.
  The tests' own comment ("no daemon is listening anywhere near this path") was false.
- **Fix:** added a `with_dead_xdg` helper that repoints `XDG_RUNTIME_DIR` at a guaranteed-dead
  tempdir under the shared `ENV_TEST_LOCK` (mirroring the sibling `*_does_not_retry_incompatible`
  tests), and wrapped all three give-up/clamp tests in it. Verified: 7/7 `with_retry` tests pass and
  the full `builder-pro-ai` lib (279) is green with the live daemons still running.

### BL-108 — trailing terminal output LOST on natural shell exit (real data-loss race) — FIXED
- **Symptom:** `attach::tests::natural_exit_final_output_reaches_attached_client_and_entry_is_reaped`
  (`crates/sessiond/src/attach.rs`) failed intermittently under full-workspace parallel load. Root
  cause was NOT test timing — it was a **production drain race**: `AttachRegistry::remove_session`
  (called from the supervisor's WAIT thread via `on_exited`, `socket_server.rs:348`) called
  `supervisor.unsubscribe_output(...)`, which yanks the reader's `std_tx` sink **before** the reader
  thread had read+broadcast the session's final chunk. The reader's subsequent broadcast of that
  chunk found the sink gone and silently dropped it — truncating trailing output to every attached
  client (a real, user-visible loss, exactly the "natural exit loses FINAL_MARKER" failure mode).
- **Fix:** removed the `unsubscribe_output` call from `remove_session`. The reader thread's own
  `sinks.clear()` on EOF/exit (`pty_supervisor.rs:454`) is the authoritative, race-free graceful
  drain — it runs strictly AFTER the read loop has broadcast every byte (loop sends each chunk via
  `retain(...send...)`, THEN breaks on EOF and clears), so every queued byte reaches every forwarder
  before it observes `Disconnected`. For the KillSession path this is moot anyway: `Supervisor::kill`
  joins the reader thread, so the reader has already cleared sinks before `remove_session` returns.
  Updated the doc comments (attach.rs `remove_session`, explaining the deliberate no-unsubscribe)
  to make the intent match the implementation. Verified: 167/167 `bpa-sessiond` lib tests pass;
  full workspace green.

### BL-107 — Keychain prompt hangs the whole test binary — FIXED
- **Symptom:** `cargo test --workspace` hung >15 min: `connectors::accounts::tests::*` (and the
  `bpa-secrets` own round-trip) blocked at 0% CPU on a macOS Keychain **authorization GUI prompt**
  that a non-interactive shell never answers. Each crate's inline `set→get→delete` skip-guard
  (`accounts.rs`, `adapter.rs`, `no_secrets_in_logs_mcp.rs`, `dispatch_integration.rs` ×2, and
  `bpa-secrets`'s own) called `security_framework` directly with no bound.
- **Fix:** added a shared, hang-proof `bpa_secrets::keychain_available(timeout)` that runs the
  round-trip on a worker thread bounded by `recv_timeout(timeout)`; a timeout is one more loud SKIP
  reason, never a hang. All six call sites now delegate to it. Verified: `bpa-orchd` (477 lib + 54
  integration) green; `bpa-secrets` round-trip dropped from **132 s → 1.67 s** (probe now SKIPs in
  ≤5 s on this binary instead of blocking on the prompt); full workspace green.

### BL-106 — e2e phase7 SKIP silently passes (CI honesty) — FIXED
- **Symptom:** `tests/e2e/orchd-survive.mjs` phase7 (connector/keychain survival) had several
  `return conn` SKIP branches that let the run exit 0 — so a Keychain-provisioning regression in CI
  would pass vacuously (CI provisions an unlocked search-list keychain, so phase7 MUST run there).
- **Fix:** the harness now tracks a `phase7Ran` flag set only on phase7's full happy-path completion;
  at the end of `main()` a SKIP fails the run when `BPA_REQUIRE_KEYCHAIN=1` is set, otherwise logs an
  allowed WARNING. `.github/workflows/ci.yml` sets `BPA_REQUIRE_KEYCHAIN=1` on the e2e orchd step.
  Verified: phase7 runs fully on an authorized binary; the gate is wired; local runs WARN, not fail.

### REL-1 — every GUI launch killed both daemons + all live terminals (P1, contradicts core promise) — FIXED
- **Symptom:** `launchd.rs::bootstrap()` treated launchctl exit 5 ("already bootstrapped") as *drift*
  and ran `bootout` (SIGTERM to the healthy running daemon) + re-bootstrap on EVERY app start. But
  `launchctl bootstrap` returns exit 5 on an already-loaded label regardless of plist drift (it does
  not diff), so the bootout fired on every routine launch — killing `bpa-sessiond` (all live PTYs +
  agent processes) and `bpa-orchd` (in-flight research runs → `failed{interrupted}`, pending OAuth
  map dropped). This voided the product's headline survival guarantee ("live shells survive the GUI
  closing") on the most common action, and bypassed the consent-gated upgrade flow. Found by the
  parallel full-audit (`docs/qa/2026-07-24-full-audit-report.md` §3.1, empirical ×3), then confirmed
  and fixed here. (New backlog row BL-109.)
- **Fix:** `bootstrap()` now treats exit 0 OR exit 5/`is_already_signal` as SUCCESS with NO bootout
  ("already loaded" is the goal; the service may hold live sessions). A genuine plist/binary reload
  goes through the consent-gated upgrade flow (`kickstart_force`, T10b dialog), not a blind bootout
  — consistent with BL-34. The misnamed pinning test `bootstrap_already_bootstrapped_is_success`
  (which asserted bootout WAS invoked) is flipped to `…_is_success_without_bootout`. Verified:
  12/12 `launchd` tests + full `cargo test --workspace` green; fmt + clippy clean.

### Net Phase-0 result
`cargo test --workspace` is now **deterministic and green** on a dev machine with live daemons +
an un-approved keychain — i.e. the project's own gate (`scripts/final-suite.sh` stage 2) is usable
again as a loop. Five backlog rows (BL-102, BL-106, BL-107, BL-108, **BL-109**) move to `done`,
including one **new P1** (REL-1) that restored the core survival promise.

### Note — phase9 e2e flake (BL-40 class, pre-existing, not a regression)
The first e2e:orchd run failed phase9 (`researchBootReconcilePhase`: expected `interrupted`, got
`transport`) under concurrent load — the blocking stub's tool call got a transport error instead of
hanging. A clean re-run passed ALL PHASES. This is the documented BL-40 family ("real-PTY / network
tests use fixed wall-clock deadlines that flake under parallel oversubscription"), not a new defect,
and is unrelated to any change here.

## Phase 0 — 2026-07-19 fix-plan verification (static, against current code)
| Item | Area | Status | Evidence |
|------|------|--------|----------|
| A1 | openpty retry | done (BL-40) | `pty_supervisor.rs` `Supervisor::create` retries on EAGAIN/ENFILE/EMFILE |
| B1 | connector body cap | done | `adapter.rs:47` `MAX_CONNECTOR_BODY=8MiB`, streaming `OversizedBody` at :194 |
| **B2** | timeout_ms floor | **mitigated** | `mod.rs:421` `effective_timeout` floors `≤0` → default (test `…defaults_nonpositive…` names B2). NOT floored in `validate_new_server` as the audit suggested — by design: small-positive values (5 s/50 ms) must pass through for the timeout tests; an explicit tiny value is the owner's choice, a 0/negative gets the safe default |
| B3 | expires_in saturating | done | `accounts.rs:497` `expires_at_from` clamps huge `expires_in` (i64 wrap fix, doc'd) |
| B4 | LIKE escape | done | `graph.rs:866` `replace('%',"\\%")` + `ESCAPE '\\'` |
| B5 | AUTH_MARKERS | done | `mcp/error.rs` `contains_status_code` standalone match (not bare substring), `AUTH_CODE_MARKERS`, tests |
| **B6** | CBOR nesting-depth | **open (P3)** | `framing.rs` has only the 16 MiB SIZE cap (`MAX_FRAME_LEN`); no depth guard. Low severity — socket is 0700 same-user; add a depth pre-check + negative test when convenient |
| C1 | reportError scrub message | done | `store.ts` `reportError` scrubs via `scrubSecrets`; `diag.test.ts` has the planted-secret test |
| C2 | startWorkspaceWatch catch | done | `App.tsx:578-582` `.catch(...)`, `FilesRail.tsx` watch-paused path |
| C3 | FormInsight created-id guard | done | `FormInsightDialog.tsx` `setCreatedInsightId` + `useSubmitGuard` (resume-from-failed-step) |
| C4 | setLifecycle exited no-op | done | `store.ts:797` "Exited always wins (C4)" |
| D1/D2 | diag scrub tests | done | `diag.test.ts` `scrubSecrets` + planted secret |
| D3 | onDaemonIncompatible test | done | `App.test.tsx:33/433` |
| D4 | theme tests | done | `ui/theme.test.ts` (matchMedia stub, setThemePref) |
| D5 | TerminalPane test | done | `components/TerminalPane.test.tsx` exists |
| **D6** | ConnectDialog test | **open (test gap)** | `ext/ConnectDialog.test.tsx` does NOT exist — the security-relevant consent-before-connect dialog is the one untested component |
| E1 | phase7 SKIP marker | done (BL-106, this pass) | see above |
| **E2** | CSP regression test | **open (pairs with F1/BL-2)** | `smoke.test.ts` has no CSP assertion |
| **F1** | tauri.conf CSP | **open (BL-2, P1, deliberately deferred)** | `tauri.conf.json:23` still `csp: null`; the audit deferred it — a wrong `connect-src` bricks Tauri v2 IPC and jsdom can't catch it, so it needs a real-app GUI smoke, not a drive-by |

**Net:** every P1/P2 correctness item (A1, B1, B3, B4, B5, C1–C4) landed; B2 is mitigated at the
call site. The genuinely-open residuals are B6 (P3 hardening), D6 (one test gap), E2 + F1/BL-2 (CSP —
the one outstanding P1, deferred by design), all already tracked.

## Relationship to the parallel full-audit + remaining open findings
A second, deeper **read-only** audit (`docs/qa/2026-07-24-full-audit-report.md`, 7 agents / ~60
probes in an isolated worktree) exists in this tree. This pass **confirmed and fixed its single
most severe P1 (REL-1)** above, and verified the prior 2026-07-19 fix plan. The remaining NEW
findings from that report are **open and prioritized** — each needs its own TDD fix slice; they are
NOT yet fixed. In priority order (severity × user-impact × fix-scope):

**P1 — security / core-promise (fix before any external test):**
- **SEC-1** — `McpCallTool` not re-gated by consent after `McpUpdateServer{url}`: a cached tool call
  reaches a NEW url with the resolved bearer, audited `tool_call/allow`. Fix: invalidate consent +
  tool cache on any security-relevant server mutation (url/command/args/env).
- **SEC-2** — stdio consent fingerprint covers only `sha256(binary)`, not `args`/`env`:
  `McpUpdateServer{args:["-c","<payload>"]}` re-uses a stale grant → arbitrary code execution; the
  env denylist only strips `DYLD_*`/`LD_*` (`NODE_OPTIONS`/`PYTHONPATH`/`BASH_ENV`… pass). Fix:
  fingerprint `command+args+env+sha256(binary)`, re-prompt on change, widen the denylist.
- **SES-1** — `RemoveWorkspace` not serialized with `CreateSession`: a create storm during removal
  leaves orphan live sessions/PTYs on the "deleted" workspace (vanish silently on next restart).
  Fix: per-workspace create/remove serialization.
- **GRAPH-1** — no ownership on graph mutations: ghost nodes are draggable/deletable and fire
  cross-project move/delete verbs. Fix: UI marks ghosts non-draggable/non-selectable; daemon rejects
  cross-project mutations.

**P2 — data integrity (high-value, mostly small):**
- **DOM-1** — export is NOT a full snapshot: the v6 `doc` family + the entire graph (nodes/edges)
  are missing from the bundle → `export→wipe→import` silently loses both.
- **DOM-3** — `SetTaskRank(±Infinity)` is accepted; the next `ExportAll` serializes `rank:null` and
  the store **cannot re-import its own export**. One Infinity rank = un-backupable store.
- **DOM-10** — `InMemoryFallback` (and `VersionTooNew`) accepts EVERY mutation silently (lost on
  restart); rules/doc files still write to disk → phantom files w/o DB rows. No mutation verb checks
  `storage_status`.
- **SES-4** — `CreateSession` does not validate `workspace_id`: a bogus id runs, every persist fails
  (log-only), vanishes on restart with no client-visible error.
- **FS-3** — `delete_entry(root, "")`/`(root, ".")` moves the **entire workspace root to Trash**
  (command-layer unprotected; UI-unreachable today). Plus FS-4 (FIFO read blocks the UI thread),
  FS-5 (`validate_path_within("/", …)` accepts `/etc/passwd` — same class as BL-103).
- **SEC-6** — `McpConnect` silently RE-ENABLES per-tool allowlist entries (`upsert_mcp_tools`
  inserts `enabled=1`): a "disable this dangerous tool" is erased on every reconnect.

**P3+** — UX-1 (false "empty" flash, no `loaded` flags), REL-3/4 (diagnostics scrub gaps + raw
ErrorBoundary copy), FE-1 (refresh epoch guards), DOM-9 (push gaps), plus 19 documentation findings
(DOC-1..19) — see the full-audit report §4–§9.

> These map cleanly onto the audit's own §11 dedup vs `docs/backlog.md` and §12 recommendations.
> Recommend sequencing: SEC-1/SEC-2/SES-1 (security + core-promise) → DOM-1/DOM-3 (backup
> integrity) → the rest. Each is a self-contained TDD slice on this branch.

## Phases 1–4 (deeper scenario re-audit)
Not separately re-run here: the project already carries a **181-scenario UX audit (169 ✅ / 0 🔴)**,
a **101-scenario investigation (remediated in 0.8.0)**, and the parallel full-audit's per-domain
probes (§4.1–4.6, all empirical). Re-auditing the same green scenarios would duplicate that work;
the higher-value path is fixing the open findings above. A fresh scenario pass against the current
code can be run on request via the `ux-audit` skill.

---

## Addendum — 2026-07-25 fix wave (release 0.10.1)

A second pass fixed the high-severity findings above with full TDD, bumped to `0.10.1`, and re-ran
the whole gate. New backlog rows BL-110..BL-122 (8 done / 5 open).

### Fixed this wave (all TDD-green, each names its regression test above)
- **SEC-1 (BL-110, P1):** `McpCallTool` now re-gates connect consent on EVERY call (both transports),
  so an HTTP url change after a grant re-prompts instead of routing the bearer to a new endpoint.
- **SEC-2 (BL-111, P1):** the stdio consent fingerprint now covers `command+args+env+sha256(binary)`,
  closing arbitrary-code-execution under a stale grant via an `args`/`env` swap.
- **DOM-3 (BL-112, P2):** `SetTaskRank` rejects non-finite ranks — the store stays re-importable.
- **SES-4 (BL-113, P2):** `CreateSession` validates `workspace_id` up front (`NoSuchWorkspace`).
- **SES-6 (BL-114, P2):** `RemoveWorkspaceRoot` canonicalizes the path so the `LastRoot` guard fires.
- **SEC-6 (BL-115, P2):** `upsert_mcp_tools` preserves a disabled tool's `enabled` flag across reconnect.
- **FS-3 (BL-116, P2):** `delete`/`rename`/`move` reject the workspace root itself (data-loss guard).
- **GRAPH-1 (BL-117, P1):** ghost graph nodes are non-draggable/non-selectable + delete-filtered.

### Deferred (filed, open): SES-1 (BL-118, RemoveWorkspace/create serialization), UX-1 (BL-119,
per-slice loaded flags), FE-6 (BL-120, toast tone), SEC-2 env-denylist (BL-121), GRAPH-1
daemon-side (BL-122). Rationale: each is a larger/cross-cutting slice better done as its own TDD
change than rushed before a release.

### Release gate (final, 2026-07-25) — ALL GREEN
| Gate | Result |
|---|---|
| English-only | PASS (320 files) |
| `cargo test --workspace` | PASS — 0 failed |
| clippy `-D warnings` | PASS |
| `cargo fmt --check` | PASS |
| `npx tsc --noEmit` | PASS |
| `npx vitest run` | PASS — **1188 tests** |
| ts-rs parity (`types.ts`/`orchd-types.ts`) | IN-SYNC |
| daemon coverage gate (sessiond+orchd ≥80%) | PASS (orchd 87.5%) |
| e2e `survive-restart` | ALL PHASES |
| e2e `orchd survive+roundtrip` | ALL PHASES (phase7 keychain ran; phase9 reconciled) |
| `cargo build --release` (aarch64) | PASS (2m53s) |

Version manifests bumped `0.10.0 → 0.10.1` (`package.json`, `tauri.conf.json`, `src-tauri/Cargo.toml`);
CHANGELOG `[0.10.1]` added; `runbook-orchd.md` DOC-1 honesty fix (orchd DOES carry live research-run
state — a restart interrupts it, boot-reconcile flips it to `failed{interrupted}`).

### Deploy status
Codebase is release-ready and compiles green in release mode. The **universal notarized artifact**
could not be produced in this environment: it needs `rustup` (to add the x86_64 target for the lipo
universal build — only the bare aarch64 toolchain is present here) AND Apple Developer-ID signing +
notarization credentials AND a `main`-branch release.yml run. Human/env steps for the actual publish:
1. Commit this branch (`qa/comprehensive-audit-2026-07-24`) and merge to `main` (release.yml is main-only).
2. Trigger `release.yml` (workflow_dispatch) with `version=0.10.1`, with the Apple creds in scope
   (`APPLE_SIGNING_IDENTITY` + `APPLE_API_*` or `APPLE_ID`/`APPLE_PASSWORD`/`APPLE_TEAM_ID`).
3. Publish the resulting draft/prerelease Release after a `sign-verify.sh` + clean-VM smoke.
