# Deep Audit Findings — 2026-07-24

> **Validity re-check 2026-07-27 (post-0.10.4).** This report was written against the 0.10.0 tree.
> Since then THREE remediation waves landed: **`0.10.1`** (audit sweep, reports
> `docs/qa/2026-07-24-full-audit-report.md` + `2026-07-24-comprehensive-qa-report.md`), **`0.10.2`**
> P1-hardening (`BL-123/124/125/134/138/139`, schema bump to **v8**), and **`0.10.4`**
> (`fix(BL-3)` — owner-only DB file mode). Every row re-verified against the current 0.10.4 tree:
>
> - **STILL VALID & OPEN (re-confirmed 0.10.4):** C1, H1, H3, H6, M1, M2, M3, M4, M5, M7, M8, L1,
>   L2, L3, L4, L5, L6, L9, L12, L13, L15. Each re-checked against current code:
>   - C1 `csp:null` still (`tauri.conf.json:23`); H1 stdio stderr still inherited (no `cmd.stderr`
>     override); H6 orchd-proto `verb_name` still zero pinning tests;
>   - M1 `Broadcaster::broadcast` still `let _ = tx.try_send(...)` silent-skip
>     (`broadcast.rs:66`); M2 GenericRest still follows redirects with bearer;
>   - M7 sessiond `migrate_v3` backfill still non-idempotent (`INSERT … SELECT` w/o `WHERE NOT
>     EXISTS`, `persistence.rs:299`); M8 sessiond still `wal_checkpoint TRUNCATE` (`:159`);
>   - L1 `McpServerRow`/`NewMcpServer`/`McpServerPatch` still `#[derive(Debug)]` carrying `env`
>     (`mcp/mod.rs:56,82,109`).
> - **CLOSED by 0.10.1:** H4, H5, L8 (UX-2 archived-filtered-from-pickers), L11, E1/E2
>   (BL-102 non-hermetic tests take `ENV_TEST_LOCK`+isolated XDG; BL-107 keychain probe bounded),
>   L10 (coverage cites swept + `docs/ux/lint.py` now a blocking gate stage).
> - **CLOSED by 0.10.2:** **M6 / BL-103** (fs commands no longer trust webview-supplied roots —
>   BL-123 `WorkspaceRootsCache` + `ensure_fs_root`/`ensure_registered_root`, fail-closed).
> - **CLOSED by 0.10.4:** **H2 / BL-3** — `bpa.db` + `orchd.db` (+`-wal`/`-shm` sidecars) now
>   `set_permissions(0o600)`, parent dir `0o700`, in both `sessiond/persistence.rs:174-183` and
>   `orchd/persistence.rs:239`.
> - **CORRECTIONS:** **L7** — protocol `verb_name` pinning regressed from 4/15 to **1/15** (only
>   `ListWorkspaces` asserted now); finding is stronger, not weaker. **L14** — sessiond
>   `CreateWorkspace` push-reach deviation is a **documented-accepted** spec §7 carve-out → info.
>   **L16** — `McpDisconnect` no-op is by Phase-1 connect-per-call design → info.
> - **Coverage gap in THIS audit (honest):** 0.10.1 found+fixed severe bugs this pass did NOT flag —
>   **REL-1** (every GUI launch ran `bootout`, killing both daemons + all live terminals — a
>   data-loss class), **SEC-1/SEC-2** (MCP consent bypass: HTTP tool calls not re-gated per-call;
>   stdio fingerprint didn't cover args+env), **GRAPH-1** (cross-project graph damage),
>   **SES-1** (`RemoveWorkspace` × `CreateSession` race). The launchd-bootstrap and consent-recheck
>   surfaces deserve a dedicated re-audit.
>
> Read the per-row "Tracked?" column below together with this banner; rows marked **NEW** are still
> actionable unless listed as fixed above.

A full-pass static + dynamic audit (Phases 0–3): behavioral matrix, contract registry,
business-rules map, security/concurrency/migration/degradation/wire-parity audits, UX-scenario
compliance trace, and a live gate run. **Research-only** — no code was modified in this pass.

## Baseline / state of the tree

- **HEAD `f49c06d` = v0.10.0**, CI-green per history. Two-daemon Tauri app (sessiond + orchd),
  CBOR wire, append-only versioning (`[3,3]` sessiond, `[1,1]` orchd). 87 orchd verbs + 15 sessiond
  verbs + 133 Tauri commands; ~69.8k Rust LOC + ~40.1k TS/TSX LOC.
- **Working tree carries uncommitted WIP** = the SW1/workflow feature (SCN-060..066): modified
  `crates/orchd-proto/src/lib.rs`, `crates/orchd/src/persistence.rs` (SCHEMA_VERSION 6→7 + `workflow`
  table), `src/App.tsx`, `src/store/store.ts`, `src/ipc/orchd.ts`, `src/components/workflows/`
  (untracked). Docs for it (scenarios SCN-060..066 draft, foundation JTBD-12, screens, plan) are
  **committed at HEAD** and consistent with the WIP code shape.
- **Gate re-run on the live tree:** clippy ✓, `cargo fmt --check` ✓, `npx tsc --noEmit` ✓,
  `npx vitest run` ✓ (**70 files / 1187 tests** — exceeds README's 0.10.0 "63/1070" because the WIP
  added workflow tests), Rust workspace tests ✓ except:
  - **2 non-hermetic `builder-pro-ai` lib tests** (`connect_with_retry_clamps_zero_attempts…`,
    `connect_orchd_with_retry_gives_up…`) — fail **only because the installed app's launchd daemons
    are live** on this machine (`/Applications/Builder Pro AI.app/…/bpa-{sessiond,orchd}`), so the
    retry-budget tests see a real handshake instead of the transient refusal they assume.
    Pre-documented in CHANGELOG `[0.10.0]`. Environmental, not a defect.
  - **`bpa-secrets::set_get_update_delete_roundtrip`** — fails on a canceled macOS Keychain auth
    prompt in this non-interactive shell; CI runs with the login keychain unlocked. Environmental.
- **ts-rs parity:** `src/ipc/orchd-types.ts` regenerates differently from committed (the WIP proto
  adds `Workflow`/`Stage`/`Gate`/`ContextScope`); `types.ts` in sync. Expected for uncommitted WIP
  — `final-suite.sh` stage 7 will fail until the regenerated file is committed (normal last step).
- **e2e (`npm run e2e:survive`, `e2e:orchd`)** not re-run this pass (would exercise the WIP tree;
  HEAD was CI-green). `e2e:orchd` phase 7 skips keychain loudly on a locked login keychain.

> The tree was **actively edited during the audit** (vitest file/test counts grew between two runs),
> so an earlier snapshot showed transient fmt/tsc/vitest failures that have since cleared. Findings
> below are re-verified against the current tree.

---

## Findings — prioritized

Severities: **CRIT / HIGH / MED / LOW / ENV**. `BL-NN` = already-tracked backlog row. `NEW` = not
previously filed. Every item carries `file:line`.

### CRITICAL

| # | Finding | Where | Tracked? |
|---|---|---|---|
| C1 | **CSP is `null`** — webview ships with no Content-Security-Policy. No defense-in-depth against a future HTML-injection sink, no egress lock-down: an injected script could `fetch()` arbitrary origins and bypass the entire trust/consent/audit machinery. Today there is no live `dangerouslySetInnerHTML` sink (regression-guarded by `src/__tests__/smoke.test.ts`), so this is latent — but it is the one control that protects the *whole* surface if the renderer is ever partially compromised. | `src-tauri/tauri.conf.json:23` | **BL-2 (P1, open)** — confirmed open. Fix needs a real-app GUI smoke (a wrong `connect-src` bricks Tauri v2 IPC; jsdom can't catch it). |

### HIGH

| # | Finding | Where | Tracked? |
|---|---|---|---|
| H1 | **stdio MCP child's stderr is inherited unredacted** into orchd's own log stream — an untrusted stdio server can forge/inject log lines or smuggle content into operator logs. DYLD/LD stripping + `stdio_exec` consent are bypassable-by-channel once the child runs under inherited stderr. | `crates/mcp/src/transport.rs:97-108` (no `cmd.stderr(...)`) | **BL-69 (P2, open)** — confirmed open. Pipe+size-bound+scrub, or `/dev/null`, before stdio servers reach users. |
| H2 | **`orchd.db` (and its `-wal`/`-shm` sidecars) created with default umask** (≈0644), not 0600. The socket/dir/log are correctly hardened (singleton.rs 0o700/0600, logging.rs 0o700) — the DB is the one hole. Carries ruleset markdown content (proven secret-bearing by `no_secrets_in_logs.rs`), invocation request_hashes, account refs. | `crates/orchd/src/persistence.rs:230-237` (no `set_permissions`); WAL sidecars inherit at `:237` | **BL-3 (P1, open)** — confirmed open. `set_permissions(0o600)` after `Connection::open`, plus sidecars; purge-on-project-delete. |
| H3 | **No active prompt-injection mediation** — `mcp_artifact.is_untrusted=1` is set on every MCP/connector result but nothing reads/rewrites/gates it; the artifact content returns verbatim to the UI. Correctly deferred to S6b (no agent to feed it to yet), but the tag is documentation, not a control, today. | `crates/orchd/src/mcp/invoke.rs:200-209`, `connectors/adapter.rs:378-389` | **BL-74 (P1, open)** — confirmed open, owner S6b. |
| H4 | **README claims features shipped in 0.10.0 that CHANGELOG `[0.10.0]` never documents** — keep-awake (SCN-045), task priority (SCN-051/schema v5), and the Docs tab (SCN-054/schema v6) are in the README 0.10.0 row but have **no Added entry** in any of the four `[0.10.0]` CHANGELOG sections (only the Docs *inline-markdown preview* and CEO/stats are logged). Violates the "update CHANGELOG in the same change" tenet. | `README.md:243` ↔ `CHANGELOG.md:5-107` | **NEW (docs).** Add Added entries, or correct the README row if these shipped under a different version. |
| H5 | **`docs/traceability.md` "Test totals — current" stuck at `[0.9.2]`** (Rust 1072 / TS 949/59) while README is 0.10.0 (1153/1070/63) and CHANGELOG `[0.10.0]` records **no** totals (unlike `[0.8.0]`). Two docs disagree on both version and counts; nothing corroborates README's 1153/1070. | `docs/traceability.md:272` ↔ `README.md:180-181` | **NEW (docs).** Bump the "current" block to 0.10.0 + restore the CHANGELOG test-total note. |
| H6 | **`orchd-proto` has NO test pinning any `verb_name` string.** `verb_name()` is exhaustive with no wildcard (92 variants named — good), but none of the 92 returned `&str` literals is asserted anywhere (no `#[cfg(test)]` mod in the proto crate; roundtrip/ts_export don't touch it). A typo (`"CreateProjct"`) compiles fine and silently mislabels every completion-trace/audit row keyed on that string. sessiond pins 4 strings; orchd pins none. | `crates/orchd-proto/src/lib.rs:1573-1667` (def); no test | **NEW.** Add a string-pinning test mirroring `protocol/src/lib.rs:341-366`, at minimum for every tail-appended verb (Workflow*, UpsertDoc, SetTaskPriority, GraphUpdateEdge, UnarchiveProject, ConnectorListProviders). |

### MEDIUM

| # | Finding | Where | Tracked? |
|---|---|---|---|
| M1 | **orchd broadcaster silently drops pushes on a full client queue, with no overflow-notify.** sessiond closed this exact gap as "D4" — `make_push_sink`'s `overflow_notify` tears down the slow connection → client reconnects → BL-92 rehydrate. orchd has **no** equivalent arm; a slow-but-responsive GUI whose reply-path stays fast but falls behind on broadcasts would **silently lose** `ProjectsChanged`/`GoalsChanged`/… with no disconnect, no rehydrate. The frontend's reconnect-rehydrate only fires on a disconnect. | `crates/orchd/src/socket_server.rs:270-296` (no `overflow_notify`); `crates/daemon-core/src/broadcast.rs:62-67` (silent skip) | **NEW (latent).** Port sessiond's `overflow_notify` discipline to orchd dispatch. |
| M2 | **`GenericRestAdapter` follows redirects with the account bearer.** Unlike the OAuth token-exchange client (`accounts.rs:540-560`, `redirect::Policy::none()`), the generic-rest client uses reqwest's *default* (following) policy. reqwest 0.13 strips `Authorization` on cross-host redirects, but **same-host** redirects keep the bearer — and the code relies on reqwest's implicit behavior rather than enforcing it. A compromised owner-chosen URL that 302s within the same host receives the live bearer at a target the owner didn't pick. | `crates/orchd/src/connectors/adapter.rs:98-122,116-122` | **NEW.** `redirect::Policy::custom` that drops `Authorization` on any redirect, or `Policy::none()` + surface non-2xx/3xx explicitly. |
| M3 | **`stdio_exec` consent fingerprint is a TOCTOU window.** Bytes are hashed at *authorize* time; the spawn happens moments later. A local attacker rewriting the binary at the exact path in that sub-second window gets the old hash's approval for new bytes. Not remotely reachable. | `crates/orchd/src/trust.rs:406-436` | **BL-68 (P2, open)** — confirmed open. open-then-hash-then-`fexecve`, or spawn-then-verify. |
| M4 | **Project deletion orphans MCP bearer Keychain entries.** `McpDeleteServer` correctly deletes the bearer (Keychain-first, fail-closed); a *project* delete cascades `mcp_server` rows via FK but can't reach Keychain from SQL → orphaned live tokens. | `crates/orchd/src/socket_server.rs:1692` (good) vs project-cascade path | **BL-81 (P2, open)** — confirmed open. Pre-cascade bearer sweep or Keychain GC keyed on `MCP_SERVICE`. |
| M5 | **Skills + ruleset markdown reads have no size cap.** Plain `read_to_string` to completion (unbounded memory/latency); `fs_explorer::read_file_preview` already establishes the stat-first / 1 MiB-cap pattern these don't adopt. A hostile/corrupted file pointed via `md_path` (which IS symlink-escape-guarded) is a DoS vector. | `crates/orchd/src/skills/registry.rs:137,199`; `ruleset_files.rs:72` | **BL-77 (P2, open)** — confirmed open. Adopt the stat-before-read cap. |
| M6 | **fs_explorer `root` is webview-supplied, not allowlisted.** Every fs command takes `root: String` from the webview and only containment-checks `rel` — nothing ties `root` to a registered workspace root. Defense-in-depth today (no live HTML sink + CSP-null), but becomes the primary FS-exfiltration/destruction vector the instant a sink appears. | `src-tauri/src/fs_explorer.rs:478-520` | **BL-103 (P2, open)** — confirmed open. Inject a registered-roots `State`; reject `root="/etc"`. |
| M7 | **sessiond `migrate_v3` backfill INSERT is non-idempotent.** `INSERT INTO workspace_root … SELECT id,0,root_path FROM workspace` has no `WHERE NOT EXISTS`/`ON CONFLICT` (unlike orchd v2's backfill) and the table has `PRIMARY KEY(workspace_id,ord)`. Gated by `user_version` so the normal path is safe, but paired with `CREATE TABLE IF NOT EXISTS` it would fail-closed on a hand-edited/inconsistent DB rolled back to ≤2. | `crates/sessiond/src/persistence.rs:274-275` | **NEW (edge).** Mirror orchd's `WHERE NOT EXISTS` or `INSERT OR IGNORE`. |
| M8 | **sessiond checkpoint uses `TRUNCATE`** — the exact blocking mode orchd downgraded to `PASSIVE` after a CI hang ("TRUNCATE waited for all readers and could block a best-effort shutdown ack"). sessiond still carries the latent hang-risk on its shutdown path; correctness is unaffected (WAL replays on next open). | `crates/sessiond/src/persistence.rs:159` vs `crates/orchd/src/persistence.rs:215-220` | **NEW (latent).** Align sessiond to `PASSIVE` for parity with orchd's documented rationale. |

### LOW

| # | Finding | Where | Tracked? |
|---|---|---|---|
| L1 | **`McpServerRow`/`NewMcpServer`/`McpServerPatch` derive `Debug` carrying `env: BTreeMap`.** For a stdio server `env` is the channel secrets travel through; today no call site `{:?}`-formats a server row (verified), but the next `tracing::debug!(?server)` or `#[instrument]` leaks every secret env var. | `crates/orchd/src/mcp/mod.rs:56,82,109` | **NEW (latent).** Hand-write redacting `Debug` (mirror `AccountToken`/`OAuthProviderConfig`). |
| L2 | **fs_explorer delete/rename/move act on a symlink's *resolved* target** (canonicalize follows the final component) — deleting `link.txt → real/data.txt` trashes the target + leaves a dangling link. Within-root is enforced; delete is Trash-reversible. | `src-tauri/src/fs_explorer.rs:387-432` | **BL-50 (P3, open).** `lstat`/no-follow on the final component. |
| L3 | **Keychain access while the screen is locked** unresolved — only bites once S6b/SW2 introduce unattended scheduling. | `crates/secrets/src/lib.rs` | **BL-27 (P1, design-gated).** Resolve before the first unattended credential-backed call. |
| L4 | **orchd log is unbounded** (no rotation), paired with BL-69's inherited stderr → disk-exhaustion + secret-accumulation surface. | orchd tracing appender | **BL-21 (P2, open).** Daily/size-capped rotation. |
| L5 | **Store `refresh*` actions don't gate on `orchdDown`** (unlike `ConnectorsTab`'s explicit `if (orchdDown) return`). During an outage every mount/reconnect-attempt toasts a failure — noise the ConnectorsTab pattern explicitly avoids. Rejections ARE caught + routed through `reportError` (never silent). | `src/store/store.ts:1004-1242` vs `src/components/ext/ConnectorsTab.tsx:220` | **NEW (polish).** Gate store refreshes on `orchdDown`, or document why ConnectorsTab diverges. |
| L6 | **`refreshDoc` silently drops the view on `NotFound`** — honest for a genuine delete-by-another-client, but a transient orchd `NotFound` removes the editor's local view with no breadcrumb. | `src/store/store.ts:1094-1103` | **NEW (polish).** Emit a diag-only event (no toast) so the drop is reconstructable. |
| L7 | **protocol `verb_name` test samples only 4/15 verbs** — a typo on an unpinned verb (e.g. `CreateSession`, `WriteStdin`) slips past. | `crates/protocol/src/lib.rs:341-366` | **NEW.** Assert all 15, or every trace/audit consumer. |
| L8 | **Inbox orphan-idea "link to project" select offers archived projects too**, inconsistent with the sidebar workspace-attach (which filters to `activeProjects`). Lets an archived (read-only per SCN-012) project be targeted from one surface. Only the idea's back-reference changes, not archived data. | `src/components/IdeasList.tsx:337,289-293` vs `WorkspaceSidebar.tsx:581` | Open UX finding (AUD2-2026-07-19-03). Filter to active like the sidebar. |
| L9 | **Terminal-area placeholders ("No terminals yet…"/"Select a terminal tab.") are inline literals** in App.tsx, outside `strings.terminal` — central-strings doctrine violation. | `src/App.tsx:682-684` | Open (AUD2-2026-07-19-04). Move into `strings.ts`. |
| L10 | **Stale UX-scenario `Coverage:` cites** (re-confirmed, not fixed since the 07-23 audit): SCN-013 (`strings.ts:198,201`→files strings; terminal strings at 246-249), SCN-045 (8 of 10 cites drifted: store/App/strings/WorkspaceSidebar/lib.rs cluster), SCN-051 (`commands.rs:1627-1655,1709-1720`→insight cmds, not task cmds at 1704/1786), SCN-054 (`orchd-proto:792-849`→ResearchRun, Doc is at 862), SCN-056 (`WorkspaceSidebar:119-152`→SCN-059 root-detection, fast-path at 224-244). Behavior matches each scenario; only the cites are wrong. | `docs/ux/scenarios.md` SCN-013/045/051/054/056 | **NEW/known.** Mechanical refresh restores audit re-runnability. |
| L11 | **`traceability.md` S-UXR row cites the legacy `docs/qa/ux-scenarios.md`**, not the canonical `docs/ux/scenarios.md` (per `CLAUDE.md`). | `docs/traceability.md:240` | **NEW (docs).** Repoint. |
| L12 | **README "Documentation index" omits `docs/ux/scenarios.md`** (the UX source of truth) and foundation/flows/screens. | `README.md:322-333` | **NEW (docs).** Add the UX docs to the index. |
| L13 | **`McpSetServerBearer` existence-check→relock window** vs a concurrent `McpDeleteServer` could orphan a bearer; low-severity under the single-connection app, latent under multi-connection/agent use. | `crates/orchd/src/socket_server.rs:1722-1752` | **NEW (latent, watch).** |
| L14 | **`sessiond CreateWorkspace` push reach ≠ siblings** — emits `WorkspaceCreated` via `push_sink` (this connection only); `Add/RemoveWorkspaceRoot`/`RemoveWorkspace` use the `broadcaster` (all connections). A second window won't learn about a new workspace without a refetch. Source acknowledges the deviation. | `crates/sessiond/src/socket_server.rs:879` (push_sink) vs `:909,926,995` (broadcaster) | **NEW (latent).** Verify/document multi-window sync expectations. |
| L15 | **`SetInsightFitVerdict.fit_reasoning` is REQUIRED (`String`) while `SetInsightStatus.resolution_reasoning` is OPTIONAL** — sibling insight verbs with divergent optionality. | `crates/orchd-proto/src/lib.rs:1108` vs `:1113` | **NEW (contract asymmetry).** Document or align. |
| L16 | **`McpDisconnect` is a no-op** (replies `Ack`, no DB, no push) by Phase-1 connect-per-call design — a client expecting side-effects gets none. | `crates/orchd/src/socket_server.rs:1775` | Documented. Optional: surface "no persistent session" honestly. |

### ENV (environmental, not defects)

| # | Item |
|---|---|
| E1 | 2 non-hermetic `builder-pro-ai` lib tests fail only when the installed app's launchd daemons are live on the host (retry-budget tests see a real handshake). Pre-documented in CHANGELOG `[0.10.0]`. |
| E2 | `bpa-secrets::set_get_update_delete_roundtrip` needs an interactive macOS Keychain auth; fails on a canceled/non-interactive prompt. CI runs with the login keychain unlocked. |
| E3 | `e2e:orchd` phase 7 (connector/Keychain) skips loudly on a locked login keychain — by design, never a silent pass. |

---

## Coverage gaps (test-thinness vs complexity)

From the frontend/backend maps, components/paths whose test count looks thin relative to their
logic — candidates to strengthen before hardening:

- **`GoalTree.tsx`** — dual-`moveGoal` ord swap + metric-chip editor; only ~3 component tests.
- **`UpgradeDialog.tsx`** — dual-daemon precedence + `hydrated` gating; 1 test.
- **`ResearchRunDialog.tsx`** — policy preflight + tool fetching; 1 test.
- **`GraphCanvas.tsx`** — optimistic edge rollback + debounced move-flush + search-epoch guard across 762 lines; 11 tests.
- **`FormInsightDialog.tsx`** — resume-from-failed-step state machine (Create→Accept→To-backlog); 7 tests.
- **`mcp/cache.rs`** — 2 tests; `connectors/adapter.rs` — 11; `skills/registry.rs` — 17; `orchd/boot.rs` — 5; `sessiond/singleton.rs` — 1.
- **e2e `.mjs` harnesses** — cross-hop restart semantics; phase-7 keychain skip is the only known soft spot.

---

## Positive confirmations (controls verified sound — absence of a finding is explicit)

- **3-phase locking** ("never hold the `Db` guard across an `.await`") is rigorously and consistently
  applied across `mcp::lifecycle`, `mcp::invoke`, `connectors::adapter`, `connectors::accounts`,
  `research::run_research`, and every dispatch arm — no `Send` violation, no deadlock, no
  guard-across-suspension. The `try_lock`→`lock().await` change in research is correct and cannot
  deadlock (single async mutex, no nested re-acquisition).
- **HoL blocking is timeout-bounded on every orchd network path** (BL-89 done) — connect_fn, list_tools,
  tools/call, OAuth exchange, generic-rest, and the research driver all bounded.
- **Migrations** are whole-chain atomic (one tx, rollback on any failure); `VersionTooNew`
  fail-closed without touching the DB; corrupt-DB quarantine + WAL/SHM sidecar cleanup; the
  `research_run` CHECK cannot be left half-applied (single-UPDATE transitions).
- **Honest degradation** is solid: every mutating control disabled while `orchdDown` (verified across
  16 surfaces); storage banner covers both non-persistent modes; "—" never a styled zero in stats;
  `partitionSessions` is exhaustive & disjoint (the 0.10.0 invisible-session cluster is closed);
  `useSubmitGuard`'s synchronous re-entry lock wraps every mutation.
- **Security controls sound:** DYLD/LD env stripping (both spawn paths); Keychain wrapper is the only
  caller, `SecretRef`/`SecretError` structurally can't hold bytes; OAuth token-exchange is
  SSRF-guarded (`redirect::Policy::none()` + 30s); `Debug` redaction on `AccountToken`/`OAuthProviderConfig`;
  8 MiB connector body cap (streamed, no OOM); import path-traversal guards; skills `md_path`
  symlink-escape guard; consent re-prompt-on-change (URL/binary/command) with namespaced kinds.
- **UX-scenario compliance:** 50 PASS / 1 GAP (SCN-028, L8) / 2 DRIFT (SCN-013/056 cites, L10) over the
  53 implemented/validated scenarios. No behavioral defects; the only substantive item is the archived-
  project link-select consistency gap.
- **Wire discipline:** both `verb_name` matches exhaustive with no wildcard; CBOR `MAX_FRAME_LEN`
  enforced on encode + decode; export pre-send frame-cap guard; append-only variant order asserted
  (sessiond) — orchd's only gap is the missing *string* pinning (H6).
- **No-secrets-in-logs:** all 5 orchd `no_secrets_in_logs_*` suites green (ruleset, connectors,
  graph, mcp bearer, dispatch-trace).

---

## Recommended remediation order (impact × effort)

1. **H6** (orchd-proto `verb_name` string-pinning test) — cheapest, closes a silent-trace-corruption
   risk across all 92 verbs. ~30 min.
2. **H4 / H5** (CHANGELOG + traceability test-total/coverage sync) — pure docs; restores the
   "numbers are measured, not guessed" discipline. L10/L11/L12 in the same docs pass.
3. **C1 (BL-2 CSP)** + **H2 (BL-3 db mode)** — the two cheapest controls that protect the entire
   surface if the renderer/host is partially compromised. Needs a real GUI smoke.
4. **M1** (orchd overflow-notify) — port sessiond's D4 fix; closes a silent-lost-push class.
5. **M7/M8** (sessiond migration idempotency + PASSIVE checkpoint) — small, removes two latent edges.
6. **H1 (BL-69 stderr)** before any stdio MCP server reaches real users; **M2** (redirect bearer)
   alongside.
7. Everything else (BL-68, BL-74, BL-77, BL-81, BL-103, L1–L16) is independently actionable; most P1
   security items are correctly sequenced to S6b/hardening already.

## Human steps (only what genuinely needs a person)

- Run the **signed/notarized universal build + clean-VM Gatekeeper smoke** to close the CSP/egress
  verification (a real-app GUI smoke a jsdom run can't provide) — `docs/build-macos.md`.
- Decide SCN-028's intended contract (should archived projects be link targets?) — a product call.
- The SW1/workflow WIP: commit the regenerated `orchd-types.ts` (ts-rs parity) + the new files when
  the slice is ready; flip SCN-060..066 `draft`→`validated` per the plan's task 14.
