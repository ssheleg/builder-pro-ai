# Full-project audit — 2026-07-24

**Target:** HEAD `f49c06d` (release `0.10.0`), audited in an isolated git worktree (`/tmp/bpa-audit`).
**Trigger:** owner request — full audit of project state, business logic, UX behavior, bugs,
architecture, and documentation, with adversarial testing.
**Status of this document:** final, with two sections (§2 baseline tail, §4.7 Rust pin-probes)
completed after the background runs settled.

## 1. Scope & method

Audited state: the committed tree at `f49c06d`. The main working tree was **not** touched: it held
uncommitted SW1 work mid-flight (since committed as `45c9836`; further BL-108 work followed). All
findings cite `f49c06d` paths/lines.

Method, in waves:

1. **Recon (3 agents):** backend map, frontend/UX map, QA-infrastructure inventory.
2. **Domain study (7 agents):** sessiond, workspaces/files, orchd domain, graph+research,
   extensions/trust, reliability/stats, Rust cross-cutting — each produced an invariant list and
   falsifiable hypotheses. Plus a pre-commit review of the SW1 WIP diff.
3. **Execution (7 agents, ~60 probes):** Node harness probes against isolated daemon instances
   (`spawnDaemon` with fresh `mkdtemp` `HOME`/`XDG_RUNTIME_DIR` per instance — the live install,
   its sockets `/tmp/bpa-501/*`, Keychain, and launchd labels were never touched; a launchd probe
   used a throwaway label `ai.builderpro.auditprobe.<pid>` and verified cleanup), vitest probes for
   store/component races, Rust pin-tests for the filesystem domain, and a real launchd
   bootstrap/bootout experiment.
4. **Baseline:** `scripts/final-suite.sh` (all 10 stages) in the worktree.
5. **Docs:** README/architecture/traceability/runbooks/backlog cross-checked against code.

Honest limitations:

- **No live GUI click-through.** UX verification is code-traced (same method as the project's own
  `docs/ux/audits/*`) plus component-level vitest probes.
- **Keychain-dependent paths** (bearer exfil end-to-end, OAuth token refresh race) were reviewed
  statically only — no Keychain access by design of the probes.
- The MCP `>16 MiB artifact` connection-drop and the `kill()`-path tail-race variant are
  static-only findings (not dynamically probed).
- Two test-hermeticity patches were applied **in the worktree only** to make the baseline runnable
  on a machine with the live app installed (see §10.3) — they are an upstream fix proposal, not a
  change to the repo.

Severity scale (project convention): **P1** security/correctness, **P2** robustness/ops,
**P3** polish. Every finding is tagged **[empirical]** (reproduced by a probe) or **[static]**
(code-traced). Duplicates of known `docs/backlog.md` rows are marked with their BL-id; everything
else is **NEW**.

## 2. Baseline: `final-suite.sh` on 0.10.0

See §2.1 for the run; §2.2 for what it took to get a clean run.

### 2.1 Result

**ALL 10 GATES PASSED** on `f49c06d` (+ the §10.3 hermetic-test patch), measured in the worktree:

| Stage | Result |
|---|---|
| 1 English-only | OK (307 files) |
| 2 `cargo test --workspace` | **1170 passed, 0 failed** |
| 3 clippy `-D warnings` | OK |
| 4 rustfmt | OK |
| 5 `vitest run` | **1130 passed, 63 files** |
| 6 `tsc --noEmit` | OK |
| 7 ts-rs parity (both bindings) | OK |
| 8 coverage gate | **bpa-sessiond ≥ 80%, bpa-orchd ≥ 80%** |
| 9 e2e survive-restart | ALL PHASES PASSED |
| 10 e2e orchd survive + round-trip | ALL PHASES PASSED |

Note: README's "1153 Rust / 1070 TS" is stale — actual 1170 / 1130 (DOC-2). Stages 1–7 ran in
one pass; stages 8–10 were re-run after an environment incident (§10.2) and passed cleanly.

### 2.2 Environment friction found while baselining

- A fresh checkout cannot run `cargo test --workspace` until the two sidecar daemons are built and
  staged into `src-tauri/binaries/` (Tauri `build.rs` hard-requires both `externalBin` entries —
  the same property BL-59 relies on). README documents this; CI stages them; a first-time local
  run fails with a raw build-script error instead of a hint. **P3 [empirical].**
- Two `src-tauri` unit tests are **non-hermetic on any machine running the installed app**
  (details and the fix in §10.3). **P2 [empirical] — NEW, related to BL-16.**

## 3. P1 findings

### 3.1 REL-1 — Every GUI launch kills both daemons (and every live terminal) via `bootstrap()`'s bootout branch — **NEW, P1 [empirical ×3]**

- **What:** `ensure_daemon_running` (`src-tauri/src/lib.rs:263-268`) runs
  `install_agent() → bootstrap() → kickstart()` on **every** app start. `launchctl bootstrap` on an
  already-loaded service exits **5**, which `is_already_signal` (`src-tauri/src/launchd.rs:106-109`)
  treats as *drift*: the code then runs **`bootout`** — a SIGTERM to the healthy running daemon —
  re-bootstraps and kickstarts a fresh one (`launchd.rs:204-214`). The doc comment says
  *"already bootstrapped == success"*, the code does the opposite.
- **Blast radius:** every GUI restart (or second app instance, e.g. dev `.app` next to the
  installed one) kills `bpa-sessiond` — **all live PTYs and their agent processes die** — and
  `bpa-orchd`, flipping in-flight research runs to `failed{interrupted}` and dropping the pending
  OAuth map. This silently voids the app's core promise ("live shells survive the GUI closing") on
  the most routine action there is. Side effects: the `IncompatibleDaemon` consent/upgrade flow is
  bypassed in the launchd-managed case (the old daemon is replaced *before* any handshake), which
  also makes the premise of **BL-34** ("a stale-but-compatible daemon is never restarted")
  outdated — the daemon is force-replaced on every boot instead.
- **Evidence:** (a) code read; (b) the bug is pinned by the unit test
  `launchd::tests::bootstrap_already_bootstrapped_is_success` (`launchd.rs:508-523`), which asserts
  bootout *is* invoked; (c) live probe on this machine: `bootstrap` on a loaded throwaway service →
  exit 5 → bootout → **PID 79975 → 80017** (`/tmp/bpa-probes/reliability/p1-launchd-rel1.sh`).
  Note the bitter irony in `kickstart()`'s own test (`launchd.rs:537-541`): it forbids `-k`
  because force-killing on every launch *"would destroy every live session with zero consent"* —
  `bootstrap()` does exactly that one call earlier.
- **Fix:** treat "already bootstrapped" as success without bootout; real drift handling belongs to
  the existing upgrade flow. Flip the pinning test to assert **no** bootout.

### 3.2 GRAPH-1 — No ownership on graph mutations: any socket client can move/delete any project's nodes — **NEW, P1 [empirical]**

- **What:** `GraphMoveNode`/`GraphDeleteNode` (and edge verbs) carry no `project_id`
  (`crates/orchd-proto/src/lib.rs`), and the daemon guards only the *archived* status of the node's
  own project (`crates/orchd/src/graph.rs:468-500`). The UI makes it worse: ghost nodes are meant
  to be read-only (`graphMapping.ts:32-34` says so) but `toFlowNodes` never sets
  `draggable:false`/`selectable:false` (`graphMapping.ts:89-108`), and `flushMoves` /
  `handleDeleteSelected` happily fire the verbs at foreign ids (`GraphCanvas.tsx:450-464,574-591`).
- **Evidence:** probe — `GraphMoveNode` on another project's node → **success**, coordinates
  persisted; `GraphDeleteNode` on it → **Ack**, its edges die by CASCADE, the ghost disappears from
  the victim project (`/tmp/bpa-probes/graph/probe-1-ghost-write.mjs`).
- **Impact:** an accidental drag on the canvas silently rewrites another project's layout (no
  success toast, the other project isn't even open); Delete removes a foreign node with all its
  edges. Server-side scoping is defense-in-depth (the socket is same-UID), but the UI-side hole is
  a real user-facing footgun *today*.
- **Fix:** UI — mark ghosts non-draggable/non-selectable and filter them in move/delete handlers;
  daemon — reject cross-project mutations unless explicitly requested (append-only wire: new verb
  or an ownership precheck where feasible).

### 3.3 SEC-1 — Tool calls are not re-gated by consent after a server URL change (bearer-exfil path) — **NEW, P1 [empirical]**

- **What:** per-call consent gating in `mcp::invoke::call_tool` exists only for stdio
  (`crates/orchd/src/mcp/invoke.rs:89-103`). For HTTP servers, consent is checked at
  `McpConnect` time against the URL fingerprint (`trust.rs`) — but `McpUpdateServer{url}` after the
  grant repoints the server, and subsequent `McpCallTool` (and the research driver,
  `research/mod.rs:454`) goes to the **new** URL with the resolved bearer, audited as
  `tool_call/allow`. `trust.rs:51-59` describes this exact attack as mitigated — only for
  `Action::Connect`.
- **Evidence:** probe — grant consent on stub A, connect, `McpUpdateServer{url: B}`, call a cached
  tool: the request **reaches B** (instrumented stub log shows `initialize` POST), error is
  `Error{io}`, never `Error{consent}`; audit records `tool_call/allow`
  (`/tmp/bpa-probes/security/probe1-gate-bypass.mjs`). Bearer itself was not attached (Keychain
  kept out of scope) — the network-path proof stands.
- **Fix:** invalidate consent + tool cache on any security-relevant server mutation
  (url/command/args/env), or re-evaluate the fingerprint on every call. Also: there is no wire verb
  to *revoke* a consent grant at all.

### 3.4 SEC-2 — stdio consent fingerprint covers neither `args` nor `env`: arbitrary code execution under a stale grant — **NEW, P1 [empirical]**

- **What:** the `bin:` fingerprint is `sha256(binary bytes)` only (`trust.rs:406-420`); `args`
  participate solely in the weaker `cmd:` fallback, and `env` is not covered at all.
  `McpUpdateServer` can rewrite both (`socket_server.rs:1629-1634`). The env denylist only strips
  `DYLD_*`/`LD_*` (`daemon-core/env_filter.rs`) — language-level injections (`NODE_OPTIONS`,
  `PYTHONPATH`, `BASH_ENV`, `PERL5OPT`, `RUBYLIB`…) pass through.
- **Evidence:** consent granted on `/bin/sh` with `args:[]` → `McpUpdateServer{args:["-c","touch …/SEC2-SPAWN-PROOF"]}` → `McpConnect` → audit `stdio_spawn/allow`, **file created**
  (`/tmp/bpa-probes/security/probe2-args-env-swap.mjs`). Env-swap variant equally allowed (env var
  demonstrably reached the child). Control: changing `command` → correctly denied.
- **Fix:** fingerprint `command + args + env + sha256(binary)`; re-prompt on any change; extend the
  env denylist beyond `DYLD_*`/`LD_*`.

### 3.5 SES-1 — `RemoveWorkspace` is not serialized with `CreateSession`: orphan sessions/PTYs, removal starvation — **NEW, P1 [empirical]**

- **What:** removal collects victims, kills them (up to ~2 s each), then deletes rows
  (`crates/sessiond/src/socket_server.rs:956-1002`). A `CreateSession` landing in that window is
  neither killed nor persisted (FK insert fails, swallowed as a log line).
- **Evidence:** 50 creates/50 ms storm → removal **starved >13 s** and left **49 live sessions with
  49 live shells** on the "deleted" workspace; short storm → removal reports `Ack`, yet 10 window
  sessions survive with live shells (`/tmp/bpa-probes/sessiond/p5-removews-race.mjs`,
  `p5b-followup.mjs`). They vanish silently at the next daemon restart.
- **Fix:** serialize create/remove per workspace (per-workspace mutex or a "closing" flag that
  fails `CreateSession` immediately).

### 3.6 UX-1 — False "empty" flash on every domain list before the first fetch lands — **NEW, P1-candidate [empirical]**

- **What:** no store slice has a `loaded` flag (`store.ts:711-726` initializes `[]`), so
  "not fetched yet" is indistinguishable from "genuinely empty": `GoalTree.tsx:513`,
  `TasksList.tsx:592`, `IdeasList.tsx:468`, `InsightsList.tsx:341`, `ServersTab.tsx:253`,
  `ArtifactsTab.tsx:151`, `ResearchPane.tsx:150`. In an app whose core tenet is honesty about
  state, a user with data sees "The goal tree is empty." on every navigation for the full
  list-latency window.
- **Evidence:** vitest probe — `GoalTree` with a 50 ms-deferred non-empty list shows the EmptyState
  before resolution (`/tmp/bpa-probes/fe/ux1-empty-flash.probe.test.tsx`). `DocsPanel.tsx:421-429`
  proves the fix pattern already exists (`docs-loading` vs `docs-empty`).
- **Fix:** per-slice `…Fetched` flags (or the keyed-slice "absence = loading" convention applied
  everywhere) and a loading row instead of EmptyState until the first response.

## 4. P2 findings (confirmed)

### 4.1 sessiond (terminal domain) — all [empirical] unless noted

| ID | Finding | Evidence |
|---|---|---|
| SES-2 | `kill -9` on the daemon loses up to ~1 s of scrollback that was **already displayed** to attached clients (flush tick is 1 s; no checkpoint on crash). 3/3 repros lost the post-tick marker. | `p2-kill9-scrollback.mjs` |
| SES-3 | After `kill -9`, a rehydrated session reports `lifecycle:"running"` with `isActive:false` forever — UI spins on a corpse; counters can mislead. | `p3-stale-lifecycle.mjs` |
| SES-4 | `CreateSession` does not validate `workspace_id`: the session runs, every persist attempt fails on FK (log-only), and it **vanishes on restart** without a single client-visible error. | `p4-bogus-workspace.mjs` |
| SES-5 | An unterminated recognized OSC (e.g. `ESC]133;C` without BEL/ST) makes the daemon **drop subsequent user output** up to the terminator — in the live stream *and* the persisted scrollback (`scrollback.rs:131-145`, `attach.rs:535-539`). A binary `cat` or a forging child hides real text. | `p6-osc-swallow.mjs` |
| SES-6 | `RemoveWorkspaceRoot` on the last root is a **silent no-op returned as success** (expected: the documented `LastRoot` error). Static hypothesis said `LastRoot`; the daemon instead ACKs without changing anything. | `p7c-roots.mjs` |
| SES-7 | Backpressure invariant "disconnect on outq overflow" did **not** reproduce in a 25 s flood window — the mute reader stayed connected (REFUTED as specified; actual overflow policy undetermined, no harm observed). | `p10-backpressure.mjs` |

### 4.2 orchd domain + export/import — all [empirical]

| ID | Finding | Evidence |
|---|---|---|
| DOM-1 | **Export is not a full snapshot:** the v6 `doc` family and the entire graph (nodes/edges) are missing from the bundle — `export → wipe → import` silently loses both. Core 6 families round-trip field-exact (verified). | `p1-export-gap.mjs` |
| DOM-2 | Import does not re-seed graph entityRefs (`insert_project_raw` bypasses `create_project`'s seeding): an imported project's graph is empty — the "project graph is never empty" invariant (S4 D6) breaks. | `p1-export-gap.mjs` |
| DOM-3 | `SetTaskRank(±Infinity)` is accepted and stored; the next `ExportAll` serializes it as `"rank":null` and the daemon **cannot re-import its own export** (`invalid type: null, expected f64`). One Infinity rank = un-backupable store. NaN → raw `NOT NULL constraint failed` as `Error{Io}`. | `p8-rank-ord.mjs`, `p8b-infinity-export.mjs` |
| DOM-4 | Import "success-failure": a post-commit ruleset-file write failure returns `Error{Io}` **after the tx committed** and emits **zero pushes** — client sees an error, data is in the store, UI never learns (pushes fire only on `Ok`, `socket_server.rs:1397-1411`). Retry then hits `Conflict`. | `p7-import-success-failure.mjs` |
| DOM-5 | `ResearchStartRun` bypasses the archived guard (no `ensure_optional_project_active` in `research/mod.rs:202-258`): a run starts and flips `captured→researching` on an **archived** project — the only child mutation escaping the (otherwise total — 27/27 verified) archived read-only. | `p2-archived-sweep.mjs`, `probe-6-research-archived.mjs` |
| DOM-6 | MCP/Skill/Trust CRUD has **no archived guard and no project-existence precheck**: `McpAddServer scope=project` on an archived project succeeds; with a bogus `project_id` it fails as raw `Error{Io}: FOREIGN KEY constraint failed` instead of `NotFound`. Same class: `update_mcp_server`, `set_mcp_server_enabled`, `grant_consent`, `add_skill`, `upsert_policy` (`ref_id` isn't even an FK). | `p2-archived-sweep.mjs` |
| DOM-7 | `mcp_server.enabled=0` blocks nothing server-side — neither `connect` nor `call_tool` reads it: research and direct calls run to `done` on a "disabled" server. (Per-tool `enabled=0` *does* block — control verified.) | `probe-7-disabled-server.mjs` |
| DOM-8 | Import accepts a goal cycle (A↔B) — rows become **invisible** to `list_goals` (anchor is `parent_id IS NULL`; REFUTED the hypothesized DoS hang: returns in 1 ms) — and an `additional` goal with `parent_id NULL` creates a **second root** (D5 broken), deletable unlike the strategic one. Related to BL-60 (cycles) — the rogue-root variant is NEW. | `p6-broken-bundles.mjs` |
| DOM-9 | Push gaps: `SetInsightStatus(accepted)` creates a graph node but emits no `GraphChanged`; `ResearchStartRun` flips idea lifecycle but emits no `IdeasChanged` — other clients/views drift until an unrelated push. | `p4-push-map.mjs` |
| DOM-10 | `InMemoryFallback` (also the `VersionTooNew`/downgrade path): **every** mutation is accepted and silently lost on restart; rules/doc files are still written to disk → phantom files without DB rows. No mutation verb checks `storage_status`; the only defense is the static banner. | `p2-degraded.mjs`, `p4-version-too-new.mjs` |
| DOM-11 | Malformed bundle errors leak raw SQLite text as `Error{Io}` (dangling FK, NaN rank) instead of typed `Validation`. | `p5-import-conflicts.mjs` |

### 4.3 Trust/security — all [empirical]

| ID | Finding | Evidence |
|---|---|---|
| SEC-3 | Rate cap races: 5 parallel `McpCallTool` at `ratePerMin=1` → **5/5 succeed** (check-then-act; only *completed* calls count). Sequential control correctly denies call #2. | `probe3-rate-race.mjs`, `probe-10-rate-race.mjs` |
| SEC-4 | Spend cap is dead on the MCP path: `cost_usd` is always NULL (no cost reporting), so the cap never binds — a UI control that does nothing (code documents it as honest degradation; the UI doesn't). | `probe7-dead-spend-cap.mjs` |
| SEC-5 | `TrustGrantConsent` writes **no** audit row — "who granted this trust, when" is unanswerable from the append-only audit that exists for everything else. | `probe6-consent-audit.mjs` |
| SEC-6 | `McpConnect` silently **re-enables** per-tool allowlist entries (`upsert_mcp_tools` inserts `enabled=1`): an owner's "disable this dangerous tool" is erased on every reconnect. | `probe4-allowlist-reset.mjs` |
| SEC-7 | Generic-REST connector: arbitrary per-call `args["url"]` + bearer, default reqwest redirect policy (Authorization kept on same-host redirects), no per-account allowlist — accepted-by-design per code comment, but it is a same-UID bearer-exfil surface worth an explicit owner decision. **[static]** | `adapter.rs:93-107` |
| SEC-8 | Positive controls hold: call timeout enforced (2002 ms at `timeoutMs:2000`, one `timeout` invocation row); deny path audited with no junk rows; per-tool disable blocks. | `probe8-timeout-bound.mjs`, `probe9-audit-control.mjs` |

### 4.4 Reliability/platform — [empirical] unless noted

| ID | Finding | Evidence |
|---|---|---|
| REL-2 | No pull-based orchd status command (sessiond has `daemon_status`; orchd relies on push only): if the `orchd://incompatible` event is lost in a cold-boot race, the upgrade UI is unreachable. The code itself defers this (`commands.rs:88-90` "out of this task's scope"). **[static]** |
| REL-3 | ErrorBoundary "Copy details" copies the **raw** `error.message + componentStack` — paths/tokens leak into the same support bundle that `recordRenderCrash` carefully scrubs. **[static]** |
| REL-4 | Diagnostics scrubber misses: JSON-quoted keys (`"access_token": "…"`), JWT, `gho_`/`ghu_`/`ghs_`/`ghr_`/`github_pat_`, `glpat-`, `AKIA…`, `AIza…`, `npm_`/`pypi-`, Slack webhooks, URL userinfo, `Cookie:` headers, PEM blocks — and a multi-word password (`password: "two words"`) passes through **whole**. | `rel4-scrub-differential.probe.test.ts` |
| REL-5 | Push loss across a reconnect has no replay — a mutation in the disconnect window never reaches the client except via the `onOrchdUp` rehydrate (which is therefore load-bearing, and was broadened in BL-92). Verified characterization; no mutation during the window arrived. | `p5-push-loss.mjs` |
| REL-6 | `request()` TOCTOU: a non-idempotent verb checked-in before a drop can be written into the **new** connection after reconnect — "executed after a micro-outage" semantics for `Create*` is undefined. **[static]** `socket_client.rs:447-469` |
| REL-7 | `launchctl` invocations have no timeout (a hung XPC wedges `bring_up_*` forever); the plist is written non-atomically. **[static]** `launchd.rs:38-47,184-191` |
| REL-8 | Boot-connect worst case ≈ 8×(0.5 s+5 s preamble timeout) ≈ 44 s vs documented "~4 s" when the daemon accepts but never handshakes. **[static]** `socket_client.rs:320-354` |

### 4.5 Frontend store/components — [empirical] (vitest probes, 28/28 green)

| ID | Finding | Evidence |
|---|---|---|
| FE-1 | 19 of 20 `refresh*` actions have **no epoch/in-flight guard** (stats got one in 0.10.0; the rest didn't): out-of-order stale writes land permanently (probe: stale `refreshGoals` response overwrites a fresher one), and bursts duplicate identical fetches (3× `refreshInvocations` = 3 in-flight calls; `onOrchdMcpInvocationLogged` refetches the **whole** log per tool call). | `fe1-refresh-race.probe.test.ts` |
| FE-2 | `refreshStats` passes an error **string** into `describeOrchdError` → toast says "unknown orchestrator error" while the inline note shows the real reason — contradicting UI in the same view. Verified on the real bundle. | wave-1 empirical |
| FE-3 | Unhandled promise rejections on hot paths: `writeStdin`/`resize` reject per keystroke against a dead daemon; `orchdGraphNeighborhood` in `FormInsightDialog` has no `.catch`. Benign today (banner is honest), hostile to any future global handler. **[static]** |
| FE-4 | Missing double-submit guards on side-effecting invokes: `ToolsBrowser` tool **Invoke** (double spend/duplicate artifacts), `ConnectorsTab` connector **Invoke**, "+ New terminal" (verified: 2 clicks → 2 sessions), `DocsPanel` mutations, export/import buttons. BL-95's `useSubmitGuard` covers the older dialogs only. | `fe8-double-submit.probe.test.tsx` |
| FE-5 | The HTML-sink smoke guard misses `outerHTML`, `insertAdjacentHTML`, `document.write`, `innerHTML+=` — and `capabilities/default.json` still authorizes unused `fs:default` + `fs:scope $APPDATA/**` and `store:default` (plugins registered, never used). With `csp: null` (BL-2), one missed sink is $APPDATA write. **[static]** |
| FS-7 | "show ignored" toggle re-lists but never restarts the watcher → ignored-file changes give no `fs://changed` until manual refresh/reactivation. | `sus7-show-ignored-watch.probe.test.tsx` |
| FS-8 | FileTree lost-invalidation race: `fs://changed` during an in-flight `listDir` is dropped (fetch guard), the stale response then repopulates the cache — phantom tree until the next event. | `sus8-lost-invalidation.probe.test.tsx` |

### 4.6 Filesystem domain (fs_explorer / fs_watcher / paths) — all confirmed by Rust pin-tests

11 pin-tests (added to `#[cfg(test)]` modules only, production code untouched) — **11/11 green**,
no regressions in the surrounding suites (`fs_*` 76/76, `bpa-paths` 20/20).

| ID | Finding | Evidence (pin-test) |
|---|---|---|
| FS-1 | Watched-root **deleted or renamed mid-session** produces no event at all — no `fs://changed`, no `fs://watch-error`, not even the `["*"]` refresh sentinel. The tree goes silently stale until a manual refresh; the first click into the dead root then fails with a misleading `outsideRoot`. | `pin_sus1_watched_root_deletion_mid_session_produces_no_event`, `…_rename_…` |
| FS-2 | FSEvents overflow/rescan collapses to the root path itself, which the filter drops (empty rel) — **no full refresh** on overflow; silent desync (kind/`need_rescan` discarded before the filter). | `pin_sus2_root_path_itself_yields_nothing_not_even_refresh_sentinel` |
| FS-3 | **Data-loss class:** `delete_entry(root, "")` or `(root, ".")` moves the **entire workspace root to Trash** — validation passes (`validate_path_within(root, root)` is Ok by design). Currently unreachable from the UI (root rows hide Delete), but the command layer itself is unprotected; one crafted invoke destroys a workspace (mitigated only by Trash recoverability). `rename/move` with `rel=""` escape only via kernel `EINVAL` surfacing as a confusing `FsError::Io`. | `pin_sus3_delete_entry_inner_empty_rel_trashes_the_whole_root`, `…_dot_rel_…`, `…_rename_…`, `…_move_…` |
| FS-4 | `read_file_preview` on a FIFO/special file **blocks the Tauri command worker indefinitely** (`File::open` waits for a writer; no `is_file()` guard) — and these commands run synchronously, so this freezes the UI thread. Agents create FIFOs routinely in build scripts. | `pin_sus4_read_file_preview_inner_on_fifo_blocks_indefinitely` |
| FS-5 | The fs security boundary is lexical only: `validate_path_within("/", "/etc/passwd")` → **Ok** — `root` is never checked against registered workspaces; a compromised/misbehaving webview gets arbitrary FS read/write within process rights. (Control: symlink-escape inside a real root still fails closed.) | `pin_sus5_validate_path_within_with_root_slash_accepts_etc_passwd` + control |
| FS-6 | Nested roots (`/a` + `/a/b`) route watch events by **first match**, not longest prefix — events under `/a/b` land in `/a`'s bucket, so the `/a/b` root node in the tree never invalidates (permanently stale), and the routing depends on Vec order. | `pin_sus6_nested_roots_route_to_first_match_not_longest_prefix` |

## 5. P3 findings (selection — full detail in agent worklogs)

| ID | Finding | Status |
|---|---|---|
| SES-8 | Attach under load duplicates ~1 line at the Replay/live seam (subscribe-before-snapshot window). | [empirical] |
| SES-9 | `atPrompt` StateChanged pushed ×3 after each command (duplicate pushes). | [empirical] |
| SES-10 | Killing a live session leaves a persistent tombstone row (`exited{signal:"Killed: 9"}` + scrollback) — visible after restart; accumulates until killed again. By design per probe, but retention (BL-4) is open. | [empirical] |
| SES-11 | `kill()` blocks a tokio worker up to ~2 s per session (no `spawn_blocking`); `killpg` on a stale pgid; `live_sessions` set grows unboundedly; startup `finished` event without `started` (zsh precmd); flush-cadence comment says "~500 ms" vs actual 1 s. | [static] |
| FE-6 | Success toasts render with the danger accent (`Toast.tsx:46`) — success looks like an error. | [static] |
| FE-7 | Restored (no-live-shell) terminals accept keystrokes that silently die (`void writeStdin` swallow); no restored marker on the tab, no "respawn shell" affordance. | [static] |
| FE-8 | No arrow-key navigation / roving tabindex in any tablist/tree (TerminalTabs, ProjectPanel, ExtPanel, FileTree); no terminal search (dead `@xterm/addon-search` dep); ⌘K undiscoverable (no visible hint); dialogs lack focus-trap. | [static] |
| FE-9 | Dead IPC surface (15 wrappers never called from UI — incl. `removeWorkspaceRoot` with no UI, `moveEntry`, `orchdUpdateProject/Task/Insight`, `orchdExportAll/ImportBundle`, `detachSession`, `getSessionState`, `powerStatus`, `researchGetRun`, `mcpUpdateServer`, `orchdDeleteInsight`); unused deps (`@xterm/addon-search/serialize/web-links`; JS sides of plugin-store/plugin-fs); dead strings key; hardcoded user-facing strings bypassing `strings.ts` (ErrorBoundary, App.tsx chips). | [static] |
| FE-10 | WebGL never enabled if `open()` happened at zero container size (stays on DOM renderer until hide/open cycle); backpressure watermark is a bare `console.warn` invisible to the owner. | [static] |
| UX-2 | QuickCapture and IdeasList attach-select offer **archived** projects the backend will always reject with a toast; WorkspaceSidebar filters to active. | [static] |
| UX-9 | `ResearchPane` polls every 2 s **per mounted pane** (N panes = N intervals); hydrate retry is infinite with no escalation/hint after N failures; `ProjectPanel` counters read 0 before fetch; selected project in dialogs isn't reset when the project list changes (submit can target a deleted id). | [static] |
| GRP-2 | After a failed/interrupted run the idea stays `researching` forever (no auto-revert; manual select exists); entityRef rename allowed server-side but silently overridden by the read-time resolver; `flushMoves` drops buffered moves on `orchdDown` without local rollback; the fit-context "related graph" panel in FormInsightDialog is dead by construction (no idea entityRef ever exists). | [static] |
| GRP-3 | Cross-edge with an archived endpoint can't be deleted/updated from the live side (escape hatch = delete your own node) — UI doesn't explain the `Invariant` toast. | [empirical] |
| GRP-4 | Orphan entityRef label = accept-time snapshot, can lag the last rename (stored label not updated by `UpdateInsight`). | [empirical] |
| LNK-1 | Terminal links: a path with a space/non-ASCII under a root links to the **root itself with `rel=""`** (wrong target, not "no link") — systematic for any char outside `[\w.@+-]`. | [empirical] |
| FS-9 | Symlink→directory renders as a file (unexpandable, "cannot preview a directory" toast); create-on-existing-name → generic `io` instead of typed `alreadyExists`; root rows have no `title` tooltip (same-basename roots indistinguishable); `dropWorkspace` doesn't prune `expanded`/`treeCache`/`selectedFile`; preview not live (no re-read on `fs://changed`). | [static] |
| STATS-1 | Stats cancel is frontend-only (the blocking scan runs on); no dedup of concurrent scans; attribution falls to "other" on symlink/case path differences; shared-root attribution is list-order-dependent. | [static] |

## 6. Refuted hypotheses (negative results worth recording)

- **Tail-loss race on natural child exit** — REFUTED: 20/20 runs, the final marker survived
  exit → drain-shutdown → restart → Replay (`p1-tail-race.mjs`). The `kill()`-path variant
  (join-wait before join-reader) remains **unprobed**.
- **Goal-cycle import DoS** — REFUTED as a hang: `list_goals` anchors on `parent_id IS NULL`, so a
  cycle is unreachable/invisible (data-integrity issue instead — see DOM-8).
- **Backpressure disconnect** — not reproduced in 25 s (SES-7).
- **"Kill live session deletes its rows"** (static guess) — REFUTED: tombstone persists
  (SES-10); "remove last root → `LastRoot` error" — REFUTED: silent no-op success (SES-6).
- **"161 vs 128 Tauri commands"** (recon suspicion) — REFUTED: grep artifact; 128 attributes =
  128 registrations, and all 127 `invoke()` names used by the frontend have handlers.
- **Frontend recon worries** — REFUTED/clean: every `Push` variant (9 sessiond + 16 orchd) is
  mapped and subscribed; all listeners/intervals/observers have cleanup; zero HTML-injection sinks;
  no broad-store subscriptions.

## 7. SW1 (workflow engine) pre-commit review

Reviewed the WIP diff as of ~15:59 (since committed as `45c9836`). The reported build-breaker
(`OrchdPush::WorkflowsChanged` unhandled) was fixed before commit; wire discipline (append-only,
`[1,1]` unchanged), schema v7 migration (additive, single-tx, fixture test), dispatch/push
patterns, ts-rs sync and test coverage are all clean. Remaining at commit time:

- **WIP-2 (P2):** daemon accepts a workflow with **zero stages**, contradicting SCN-060 and the
  project's own "server is the authoritative guard" doctrine (`validate_workflow`,
  `crates/orchd/src/persistence.rs:3199`; the client blocks it, the wire doesn't).
- **WIP-3 (P2):** same-change rule unmet at commit: SCN-060..066 still `draft`, SCR-01..04 still
  `designed`, SCN-007's nav row/icon (`⚙` vs actual `⛓`, position) stale.
- **WIP-4 (P3):** no project-level "Run workflow" entry; per-row "Run →" doesn't preselect that
  workflow (picker always defaults to the first global one).
- **WIP-5 (P3):** `stage.id` not validated (empty/duplicates allowed) — future run-journal
  addressing depends on it.

## 8. Architecture assessment

Solid overall: zero TODO/FIXME in the backend, disciplined error taxonomy, bounded external calls,
append-only wire with parity tests, strong test culture. Watch-items:

- **ARCH-1 (P2):** lock-poisoning cascade — 61× `.lock().unwrap()` in `pty_supervisor.rs` (plus
  clients/commands), while the codebase already knows the `into_inner()` recovery pattern
  (`socket_server.rs:164,177,195`) — one panicking reader thread kills the whole PTY supervisor
  (and a panicking flusher kills daemon-wide persistence). Unify on a poison-tolerant lock helper.
- **ARCH-2 (P2):** `socket_client.rs` vs `orchd_client.rs` drift has begun: per-request completion
  tracing exists only in the orchd one; reconnect-escalation tests (7) exist only in the sessiond
  one. Extract a generic frame-work or port the tests/trace before the next copy-paste divergence.
- **ARCH-3 (P2):** orchd error codes cross three layers as `format!("{code:?}")` strings matched
  by the frontend (`"Invariant"` etc.) while ts-rs exports the same enum in camelCase — two
  spellings, one rename away from silent breakage. Pin with a test or serde-serialize.
- **ARCH-4 (P3):** monoliths — `persistence.rs` 8159 lines (75 pub methods, tests interleaved),
  `commands.rs` 5692 (111 commands + 50 `expect_*` wrappers — macro-able), sessiond
  `socket_server.rs` 4922 (mostly tests). Mechanical to split when it starts hurting.
- **ARCH-5 (P3):** `unreachable!()` in reconnect paths (4 sites) — today safe by construction,
  one new `HandshakeError` variant away from a silently dead client. Type-narrow `RetryDecision`.
- **ARCH-6 (P3):** two reqwests (0.12 via `oauth2`, 0.13 elsewhere) — documented decision; track
  an `oauth2` bump. `thiserror 1` vs transitive 2, `rustix 0.38` vs 1.1 — candidate bumps.
- **ARCH-7 (P3):** boot-time `panic!`s (tracing init, no-usable-DB) bypass the honest `ExitCode`
  path.
- **ARCH-8 (P2, design):** fs_explorer/fs_watcher security boundary holds only if the frontend
  sends daemon-validated roots — nothing checks `root` against registered workspaces, so a
  compromised renderer gets arbitrary FS read/write within process rights (`/` passes validation).
  Keep a workspace-root snapshot in core State and validate per call.
- **Task/thread lifecycle:** clean — every spawn has a shutdown path; the single deliberate
  exception (detached research driver) has the boot-reconcile backstop.

## 9. Documentation findings (19 total — DOC-1..19)

Systemic pattern: docs are disciplined but **lag one release** (0.10.0 not fully reflected).
Highlights:

- **DOC-1 (P1):** `runbook-orchd.md` claims "no live runtime state to lose" and "restarting
  bpa-orchd does not end any live work" — false since 0.7.0: research runs are live tokio tasks;
  a restart interrupts them (the same runbook documents it 300 lines later). Operator-misleading.
- **DOC-9 (P2):** CHANGELOG says `RemoveWorkspace` kept the wire at `[1,1]` — it's a **sessiond**
  verb; sessiond wire is `[3,3]` (the `[1,1]` space is orchd's).
- **DOC-2/3/19 (P2/P3):** stale measured numbers — README "1153 Rust / 1070 TS tests" vs actual
  ~1178 / ≥1123; `traceability.md` "current" totals frozen at 0.9.2; architecture.md "~925 vitest".
- **DOC-4/5/6 (P2):** `architecture.md` ends at 0.9.2 — no 0.10.0 (stats/power modules missing
  from the module map, schema v5/v6 undocumented, says "no v5 migration" while v6 ships).
- **DOC-7 (P2):** circular broken reference — architecture.md points to README's "survival truth
  table", README points back to architecture.md; neither has it (it lives in the platform-overview
  spec §2).
- **DOC-8/12/13/15/16 (P2/P3):** runbook says recovery recreates "schema-v4" (actually v6);
  traceability names a test by its old name and mis-numbers final-suite stages; BL-102 line refs
  stale; build-macos describes sign-verify/smoke as sessiond-only (both scripts cover both daemons).
- **DOC-10/11 (P2):** governance drift — `CONTRIBUTING.md` + `check-ux-scenarios.sh` still watch
  the **superseded** `docs/qa/ux-scenarios.md` catalog while the source of truth moved to
  `docs/ux/scenarios.md`; the sync gate is effectively dead.
- **DOC-14/17/18 (P3):** BL-17 marked open though the CI coverage job exists and blocks; four
  separate `## [0.10.0]` changelog headings (keepachangelog wants one); SCN-058/059 statuses
  (`validated` vs `implemented`) inconsistent with neighbors.

## 10. QA-infrastructure assessment

Strong: a real 10-stage gate shared by local + CI, daemon coverage ≥80% enforced, ts-rs parity,
e2e survival harnesses with their own CBOR codec, anti-skip guards, English-only gate.
Gaps found:

### 10.1 Process/gates

- `check-ux-scenarios.sh` is advisory-only **and** watches the superseded catalog (DOC-10) — the
  scenario-sync rule currently has no teeth. `docs/ux/lint.py` (integrity linter) is wired nowhere
  (not CI, not hooks, not final-suite) though it passes today.
- No frontend coverage gate/tooling; no ESLint/Prettier (only `tsc` strict); GUI-level e2e and the
  clean-VM smoke are manual runbooks; pre-push hook is per-clone manual; `nightbuild` isn't
  CI-covered (by design — runner cost); `sign-verify.sh`/`smoke-clean-vm.sh` are orphaned from
  release.yml (BL-105).

### 10.2 Flaky/non-hermetic tests

- BL-108's two load-sensitive sessiond flakes are being fixed concurrently (the BL-108 attach
  drain fix was landing in the main tree during this audit).
- **NEW (this audit):** `tests::connect_with_retry_clamps_zero_attempts…` (lib.rs:1111) and
  `tests::connect_orchd_with_retry_gives_up…` (lib.rs:1390) — and, timing-dependent,
  `tests::connect_with_retry_gives_up…` (lib.rs:1089) — expect "nothing is listening" but resolve
  the socket via the `/tmp/bpa-<uid>` fallback (other tests `remove_var("XDG_RUNTIME_DIR")` instead
  of restoring it, lib.rs:1188,1274,1361,1466,1535), so on any machine running the installed app
  the **live daemons answer** and the tests fail. Reproduced twice on this machine; on CI they
  pass only because no daemon is installed. Related to BL-16's env-race class.
- **Environment incident (worth a runbook line):** the coverage run deadlocked for hours inside
  Apple's Security framework — the parallel keychain tests (`connectors::accounts/adapter`)
  piled onto the framework's process-global mutex while a **11-day-wedged `coreautha`
  (LocalAuthentication agent)** held an unresolved auth session. Diagnosed via `sample` (all
  threads in `SecItemDelete → __psynch_mutexwait`); remedy: `kill -9 <coreautha pid>`, then the
  run goes green with zero code changes. If it ever recurs without a wedged agent, run the
  keychain suites with `--test-threads=1`. Keychain verified clean afterwards (no orphaned test
  entries).

### 10.3 The applied fix (worktree-only — upstream as-is)

Each of the three tests now takes `ENV_TEST_LOCK`, points `XDG_RUNTIME_DIR` at a fresh
`tempfile::tempdir()`, and removes the var afterwards — the exact discipline the file already uses
for its other env-mutating tests. Diff lives in `/tmp/bpa-audit/src-tauri/src/lib.rs` (three test
bodies); `cargo fmt --check` + `clippy -D warnings` pass on it.

## 11. Findings vs `docs/backlog.md` (dedup)

Already tracked (do not refile): BL-2 (CSP), BL-3 (db file mode), BL-4 (retention — SES-10
relates), BL-5 (stale-pane — FE-7 relates), BL-6 (fire-and-forget toasts — FE-3 relates), BL-11
(escaped-descendant orphans), BL-16 (env-race class — §10.2 extends), BL-30 (boot RAM), BL-34
(see REL-1 §3.1 — premise now outdated), BL-45 (treeCache orphan), BL-47 (native confirm —
extended to 12 sites), BL-49 (link scope — LNK-1 is a worse variant), BL-52 (watcher gitignore),
BL-60 (import cycles — DOM-8 adds the rogue-root variant), BL-66 (stored labels in neighborhood —
GRP-4 relates), BL-70 (connect-per-call), BL-76 (connector invocation push), BL-81 (keychain
orphans), BL-95 (double-submit — FE-4 adds new surfaces), BL-105 (orphaned scripts), BL-108
(flakes — being fixed concurrently).

**NEW (40+):** REL-1..REL-8 (§3.1, §4.4), GRAPH-1 (§3.2), SEC-1..SEC-7 (§3.3-3.4, §4.3),
SES-1..SES-6 (§3.5, §4.1), DOM-1..DOM-11 (§4.2), FE-1..FE-5 (§4.5), FS-1..FS-8 (§4.5-4.6),
UX-1 (§3.6), UX-2, LNK-1, GRP-2..4, STATS-1, WIP-2..5, DOC-1..19, §10 gaps.

## 12. Recommendations (priority order)

1. **REL-1** — one-line-class fix + flip the pinning test; every launch currently kills all live
   terminals. Re-evaluate BL-34 and the upgrade-flow reachability after.
2. **SEC-1 + SEC-2** — re-gate on any server mutation; fingerprint `command+args+env+binary`;
   add a consent-revoke verb.
3. **GRAPH-1** — UI read-only ghosts (two-line change) now; server-side scoping as a wire
   follow-up.
4. **SEC-3** — count in-flight invocations (or a per-policy mutex across check+record).
5. **UX-1** — per-slice loaded flags (pattern exists in `DocsPanel`).
6. **DOM-1/DOM-2** — include docs+graph in the bundle, or label the export honestly as partial;
   re-seed entityRefs on import.
7. **DOM-3/DOM-4/DOM-8** — validate rank finiteness, single-root/kind-parent coherence on import;
   emit pushes (or roll the response forward) on post-commit failure paths.
8. **DOM-5/DOM-6/DOM-7** — extend `ensure_optional_project_active` to research + MCP/skill/trust
   CRUD; enforce `mcp_server.enabled` server-side; typed `NotFound` for FK misses.
9. **FE-1** — generalize the stats epoch/in-flight guard across all `refresh*` actions; debounce
   `refreshInvocations`.
10. **REL-3/REL-4** — scrub ErrorBoundary's copy; extend the scrubber corpus (JSON-quoted keys,
    JWT, provider token prefixes, userinfo, cookies, PEM).
11. **SES-1/SES-4/SES-5/SES-6** — per-workspace create/remove serialization; validate
    `workspace_id` on create; fail-open the OSC stripper on unterminated sequences; return
    `LastRoot` honestly.
12. **FS-1..FS-6** — reject destructive verbs whose `rel` canonicalizes to the root itself;
    guard preview with `is_file()`; map root-loss/overflow to `fs://watch-error` (or the `["*"]`
    sentinel); route nested roots by longest prefix (or reject nesting at `AddWorkspaceRoot`);
    keep a workspace-root snapshot in core State and validate `root` on every fs call.
13. **§10** — wire `docs/ux/lint.py` into CI/final-suite; repoint `check-ux-scenarios.sh` +
    CONTRIBUTING to `docs/ux/scenarios.md`; upstream the hermetic-test patch (§10.3); frontend
    coverage gate; prune dead IPC/deps/capabilities (FE-9/FE-5).
14. **DOC** — one release-lag pass: architecture.md + traceability.md to 0.10.0, runbook-orchd
    honesty fix (DOC-1), CHANGELOG merge + wire-version fix (DOC-9/DOC-17), measured numbers rule
    re-applied (DOC-2).

## 13. Appendix

### 13.1 Baseline final result

`scripts/final-suite.sh` on `f49c06d` + the §10.3 patch: **ALL GATES PASSED** (see §2.1 for the
stage table). The isolated worktree used for the run was removed after the gate; the two
upstream-ready patches were kept:

- `/tmp/bpa-probes/upstream-patches/hermetic-connect-tests.patch` — the §10.3 fix (3 tests;
  gate-checked: fmt + clippy clean).
- `/tmp/bpa-probes/upstream-patches/fs-pin-tests.patch` — the 11 §4.6 pin-tests (added after the
  gate stages; converts to regression tests alongside the FS fixes).

### 13.2 Probe inventory (reproducible)

All probes target isolated daemon instances (fresh `mkdtemp` `HOME`/`XDG_RUNTIME_DIR`); none touch
the live install. Re-run with `node <script>` (binaries overridable via `BPA_ORCHD`):

- sessiond: `/tmp/bpa-probes/sessiond/p1…p10*.mjs`, `p5b/p7b/p7c` follow-ups (`common.mjs`,
  `lib/daemon-harness.mjs`).
- orchd domain: `/tmp/bpa-probes/orchd/p1…p9*.mjs` (`lib-orchd.mjs`).
- graph+research: `/tmp/bpa-probes/graph/probe-1…probe-10-*.mjs` (`lib/orchd-lib.mjs`).
- security: `/tmp/bpa-probes/security/probe1…probe9-*.mjs` (`orchd-codec.mjs`, `rest-stub.mjs`).
- reliability: `/tmp/bpa-probes/reliability/p1-launchd-rel1.sh`, `p2…p5-*.mjs`.
- frontend: `/tmp/bpa-probes/fe/*.probe.test.{ts,tsx}` (28/28 green;
  `vitest run --config vitest.probe.config.ts`).
- Rust pins: §13.3.

### 13.3 Rust pin-probes

11 pin-tests against real production code (only `#[cfg(test)]` additions, +333 lines in the
scratch worktree — `/tmp/bpa-audit`): `crates/paths/src/lib.rs` (+34),
`src-tauri/src/fs_explorer.rs` (+132), `src-tauri/src/fs_watcher.rs` (+167). All 11 green; suites
around them regression-free (`cargo test -p builder-pro-ai --lib fs_` → 76/76, `cargo test -p
bpa-paths --lib` → 20/20). Names in §4.6; each pins *current* behavior with a `desired:` note, so
they can be lifted into the repo as failing-then-passing regression tests alongside the fixes.

> Probe hygiene note: the two FS-3 pin-tests necessarily exercise the real Trash path (that's
> what `delete_entry` does) — `~/.Trash` was verified empty after the run; nothing was left behind.

> Note: `/tmp/bpa-probes/**` is scratch space (survives until reboot). Ask before migrating any of
> it into the repo as regression tests — each confirmed finding above names its probe for exactly
> that purpose.

---

## Remediation outcome (2026-07-24, branch `audit-remediation-2026-07-24`)

Every actionable finding in this report was remediated on the branch; the rest is filed in
`docs/backlog.md` (BL-123..BL-170). Validation: `scripts/final-suite.sh` (11 stages — now with the
blocking `docs/ux/lint.py`) → **ALL GATES PASSED**; 1260 Rust tests / 1270 vitest (71 files), both
daemons ≥80% coverage, all e2e phases green. Plan and per-wave detail:
`docs/superpowers/plans/2026-07-24-audit-remediation.md`; groupings: `CHANGELOG.md` [Unreleased].
BL-143 (server-side graph ownership) landed as a follow-up: scoped graph mutations are typed
`NotFound`, wire unchanged. Concurrent main-tree work independently fixed REL-1 and BL-107/BL-108 —
either side can be taken at merge.
