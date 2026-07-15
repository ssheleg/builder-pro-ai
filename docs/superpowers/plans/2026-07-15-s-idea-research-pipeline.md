# S-IDEA — Ideas + Research Pipeline — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The end-to-end loop **idea → research (MCP tool) → durable artifact → owner-evaluated insight → backlog task**, without the S6 agent org — hosted in `bpa-orchd`, reusing the shipped Idea/Insight/Task entities, the S-EXT MCP invoke path, and the S4 graph.

**Architecture:** Net-new is a single `research_run` entity (schema v4) + an async background-run driver that calls the shipped `mcp::invoke::call_tool` and records the resulting durable `mcp_artifact` as the ResearchArtifact. Every other step (spawn-project, insight/task formation, fit-context) reuses shipped verbs. Owner-driven fit-verdict (LLM deferred to S6a).

**Tech Stack:** Rust (tokio, rusqlite, `bpa_daemon_core::migrate`, the shipped `mcp::invoke`/`trust`/`graph` modules), React 19 / Zustand, Node e2e harness. **No new external deps.**

**Spec:** `docs/superpowers/specs/2026-07-15-s-idea-research-pipeline-design.md` — §4 (DDL) and §5 (wire) are the verbatim contracts; tasks reference them (DRY).

## Global Constraints (every task implicitly includes these)

- **Host = `bpa-orchd`**; the research run CALLS `mcp::invoke::call_tool` (no new egress, no new crate). Reuse verbs for everything but the 3 research verbs (spec D6).
- **Wire layering** (spec §5): frame enums `OrchdRequest/Response/Push` stay plain snake_case Hop-B-only (NO ts-rs), variants appended at END; the new `ResearchRun` entity + `ResearchStatus` enum get `#[serde(rename_all="camelCase")]` + `ts_rs::TS` + `#[ts(export_to="orchd-types.ts")]`, i64 timestamps `#[ts(type="number")]` (mirror `McpArtifact`).
- **Migration**: additive `Migration{upto:4}` mirroring `migrate_v3`; `SCHEMA_VERSION` 3→4; whole-chain single-tx, forward-only.
- **Async-run safety (spec D11/D12)**: the background run task is a detached `tokio::spawn` holding `Arc<Mutex<Db>>` + the broadcaster (NOT a `&Db`), 3-phase-locked (never holds the DB lock across the network await); **boot-reconcile** on orchd boot marks stale `pending`/`running` runs → `failed{interrupted}`; `call_tool`'s connect handshake is timeout-bounded.
- **Honest degradation (Q8)**: failed run → `failed{error_kind}` + the UI's «без ресёрча» path; no fake success, no silent retry. `error_kind` is a fixed vocabulary — never args/secrets/tool-output.
- **Transition atomicity**: each status transition is ONE `UPDATE` (the `(status='done')=(artifact_id NOT NULL)` CHECK); `start_research_run` is one transaction.
- **Production-grade**: TDD; honest error handling; structured logs w/o secrets; docs same slice.
- **Gate**: `bash scripts/final-suite.sh` → `ALL GATES PASSED` (9 stages); orchd coverage ≥80%; ts-rs parity; `RUST_TEST_THREADS=4`; retry-once ONLY the known BL-40 attach flake; no env-fragile wall-clock asserts (the run tests use the fake `connect_fn`/`ToolCaller` seam, not real timing). Commit trailer `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`.
- **PATH**: `export PATH="$HOME/.cargo/bin:$HOME/.rustup/toolchains/1.92-aarch64-apple-darwin/bin:/opt/homebrew/bin:$PATH"`; frontend `npx vitest run`/`npx tsc --noEmit`. Gate needs staged sidecars (`cargo build -p bpa-sessiond -p bpa-orchd` → `src-tauri/binaries/bpa-{sessiond,orchd}-aarch64-apple-darwin`, gitignored).

## Dependency graph & parallel groups

```
T1 (connect-timeout fix in shipped mcp::invoke) — small, isolated, do FIRST (research inherits it)
T2 (schema v4 research_run + persistence CRUD + boot-reconcile) depends T1(none, but sequence)
T3 (orchd-proto research verbs+entity+push + ts + temp dispatch arm) — SEQUENTIAL contract
T4 (research module: start_run + async driver + graph-ingest-on-accept) depends T2,T3,T1
T5 (dispatch P + core commands + broker + register) depends T3,T4
T6 (frontend ipc+store+ResearchRunDialog/ResearchPane/FormInsightDialog/spawn-project + IdeasList) depends T5
T7 (e2e survival + boot-reconcile phases + gate) depends T5   [T6 ‖ T7 — disjoint files]
T8 (docs truth + CHANGELOG [0.7.0] + backlog + gate) depends T6,T7
T9 (whole-branch review + merge + CI green)
```

---

### Task 1: bound the connect/initialize handshake in `mcp::invoke::call_tool` (spec D12)

**Files:** Modify `crates/orchd/src/mcp/invoke.rs` (wrap the `connect_fn(...).await` in a timeout); its `#[cfg(test)]` module (add the never-resolving-connect test).

**Interfaces — Produces:** `call_tool` now bounds BOTH the connect handshake AND the `tools/call` RPC by `server.timeout_ms`; a connect that never completes → `McpError::Timeout` (mapped to the run's `timeout` error_kind), not a hang.

- [ ] **Step 1: RED.** In `invoke.rs`'s test module, add `call_tool_connect_that_never_resolves_times_out_not_hangs`: a `connect_fn` returning a future that never resolves (`std::future::pending()`); a server row with a short `timeout_ms` (e.g. 50); assert `call_tool(...).await` returns `Err(OrchdMcpError::…Timeout…)` within a bounded wall-clock (use a real sub-second timeout OR `tokio::time` pause/advance), and does NOT hang. Run `cargo test -p bpa-orchd mcp::invoke -- --nocapture` → FAIL (currently the connect await is unbounded).
- [ ] **Step 2: GREEN.** In `call_tool` (the phase-2 network section), wrap the `connect_fn(server, bearer).await` in `tokio::time::timeout(Duration::from_millis(server.timeout_ms), …)`; on elapse map to the same `McpError::Timeout`/`OrchdMcpError` the `tools/call` timeout produces (reuse `classify_error_kind`→`timeout`). Keep the existing `tools/call` timeout untouched. No DB lock is held here (phase 2), so no lock-across-await concern.
- [ ] **Step 3:** `cargo test -p bpa-orchd` (full crate — the existing mcp/connector/trust tests must stay green; the connect-happy-path tests must still pass), clippy `-D warnings`, fmt. Commit `fix(orchd): bound the MCP connect/initialize handshake by server.timeout_ms — no hang-forever (S-IDEA D12)`.

### Task 2: `orchd.db` schema v4 (`research_run`) + persistence CRUD + boot-reconcile

**Files:** Modify `crates/orchd/src/persistence.rs` (SCHEMA_VERSION 3→4; `Migration{upto:4}`; research_run CRUD; the boot-reconcile query), `crates/orchd/src/boot.rs` (call boot-reconcile after `open_db`); Create `crates/orchd/src/research/mod.rs` (row structs + status enum); Modify `crates/orchd/src/lib.rs` (`pub mod research;`).

**Interfaces — Consumes:** T1. **Produces:** on `persistence::Db`:
- `start_research_run(NewResearchRun{idea_id, server_id, tool_name, args_json}) -> ResearchRunRow` — ONE `unchecked_transaction()`: validate idea+server exist, insert `research_run{status:pending}`, and flip `idea.lifecycle` `captured→researching` (only if currently `captured`). Returns the pending row.
- `set_research_run_running(id)`, `set_research_run_done(id, invocation_id, artifact_id)`, `set_research_run_failed(id, error_kind)` — each ONE `UPDATE` satisfying the §4 CHECK.
- `list_research_runs(idea_id) -> Vec<ResearchRunRow>` (newest first), `get_research_run(id) -> Option<ResearchRunRow>`.
- `reconcile_interrupted_research_runs() -> usize` — `UPDATE research_run SET status='failed', error_kind='interrupted', updated_at=? WHERE status IN ('pending','running')`; returns the count. Called from `boot::run` right after the DB opens (mirror `ensure_global_ruleset`).
- Row struct `ResearchRunRow` + `ResearchStatus` enum (Pending/Running/Done/Failed) with TEXT⇄enum helpers (`pending`/`running`/`done`/`failed`), in `research/mod.rs`.

- [ ] **Step 1: RED.** `crates/orchd/src/research/mod.rs` `#[cfg(test)]` (in-memory Db, seed an idea + an mcp_server): after migration schema_version==4; `start_research_run` inserts `pending` + flips the idea `captured→researching` (assert both, one call); a `specced` idea's lifecycle is NOT changed by a new run; `set_research_run_done` sets `done`+artifact_id (CHECK holds); `set_research_run_failed` sets `failed`+error_kind (artifact_id stays NULL); the CHECK rejects a `done` row with NULL artifact_id (a persistence-level test); `list`/`get`; `reconcile_interrupted_research_runs` flips a seeded `running`+`pending` to `failed{interrupted}` and leaves a `done`/`failed` row untouched; `idea_id` FK CASCADE (deleting the idea deletes its runs). Run `cargo test -p bpa-orchd research` → FAIL.
- [ ] **Step 2: GREEN.** Bump `SCHEMA_VERSION` to 4; append `Migration{upto:4}` with the §4 DDL VERBATIM (mirror `migrate_v3`'s `execute_batch` shape). Implement the CRUD + `reconcile_interrupted_research_runs` in persistence.rs (parameterized; `now_ms()`; enum⇄TEXT). Wire `research::run_boot_reconcile` (or call `db.reconcile_interrupted_research_runs()`) into `boot::run` after the DB opens. `pub mod research;` in lib.rs.
- [ ] **Step 3:** `cargo test -p bpa-orchd research` + full `cargo test -p bpa-orchd` green; clippy/fmt. Commit `feat(orchd): schema v4 research_run + persistence CRUD + boot-reconcile of interrupted runs (S-IDEA §4, D11)`.

### Task 3: `orchd-proto` — research verbs + entity + push

**Files:** Modify `crates/orchd-proto/src/lib.rs` (append at END), regenerate `src/ipc/orchd-types.ts`; add a temp dispatch arm in `crates/orchd/src/socket_server.rs`.

**Interfaces — Consumes:** T2. **Produces (spec §5):**
- Entity `ResearchRun { id, idea_id, server_id, tool_name, args_json, status: ResearchStatus, invocation_id: Option<String>, artifact_id: Option<String>, error_kind: Option<String>, created_at, updated_at }` (camelCase+ts-rs; i64 `#[ts(type="number")]`); enum `ResearchStatus{Pending,Running,Done,Failed}` (camelCase wire).
- Requests (append at END, snake_case): `ResearchStartRun{idea_id, server_id, tool_name, args_json}`, `ResearchListRuns{idea_id}`, `ResearchGetRun{id}`.
- Responses: `ResearchRun(ResearchRun)`, `ResearchRuns(Vec<ResearchRun>)`.
- Push: `ResearchRunsChanged{idea_id: Option<String>}`.

- [ ] **Step 1: RED.** In `crates/orchd-proto/src/lib.rs` `#[cfg(test)]`: frame round-trip `OrchdResponse::ResearchRuns(vec![sample])` (snake_case, lossless); a `ResearchRun` entity serializes camelCase (`ideaId`, `createdAt`, `toolName`) — assert exact keys; `ResearchStatus::Pending` serializes `"pending"`; the ts_export test regenerates `orchd-types.ts` containing `ResearchRun` + `ResearchStatus` (camelCase, `createdAt: number`, `invocationId: string | null`). Run `cargo test -p bpa-orchd-proto` → FAIL.
- [ ] **Step 2: GREEN.** Append the entity + enum (mirror `McpArtifact`'s derive block) and the frame variants at the END of each enum. Regenerate + commit `orchd-types.ts`. Add a TEMPORARY wildcard dispatch arm in `socket_server.rs` covering `ResearchStartRun|ResearchListRuns|ResearchGetRun` → `Error{Io,"research dispatch not yet implemented"}` so `cargo build -p bpa-orchd` succeeds (T5 replaces). Also add a temp broker arm for `ResearchRunsChanged` in `src-tauri/src/broker.rs` (emit `orchd://research-runs-changed` null payload — T5 finalizes) so `--workspace` stays green.
- [ ] **Step 3:** `cargo test -p bpa-orchd-proto`; `cargo build --workspace` green; clippy/fmt; confirm `orchd-types.ts` gained `ResearchRun`. Commit `feat(orchd-proto): S-IDEA research wire — ResearchRun entity + Start/List/Get verbs + ResearchRunsChanged (append-only) (S-IDEA §5)`.

### Task 4: research module — `start_run` + async background driver + graph-ingest-on-accept

**Files:** Modify `crates/orchd/src/research/mod.rs` (the orchestration + the background driver), `crates/orchd/src/persistence.rs`/`crates/orchd/src/graph.rs` wiring for insight-accept graph-ingest.

**Interfaces — Consumes:** T2 (CRUD), T3 (types), T1 (bounded call_tool). **Produces:**
- `research::start_run(db: &Arc<Mutex<Db>>, broadcaster, idea_id, server_id, tool_name, args_json, connect_fn) -> Result<ResearchRunRow, OrchdError>` — `db.start_research_run(...)` (pending + idea flip); `tokio::spawn` the driver (captures `Arc<Mutex<Db>>` + a cloned `Broadcaster` — NOT a `&Db`); return the pending row. The driver: (1) lock→`set_research_run_running`, broadcast `ResearchRunsChanged{idea_id}`→unlock; (2) `mcp::invoke::call_tool(db, server_id, tool_name, args_json, project_id, connect_fn)` (bounded per T1, 3-phase); (3) `Ok`→lock→`set_research_run_done(invocation_id, artifact_id)`, broadcast→unlock; `Err`→lock→`set_research_run_failed(classify)`, broadcast→unlock. `error_kind` from the shipped `classify_error_kind`/`PolicyCapExceeded→policy_cap_exceeded`.
- **graph-ingest-on-accept (D9):** modify the shipped `set_insight_status` path (`persistence.rs` / the `SetInsightStatus` dispatch): when status→`accepted`, ALSO `add_entity_ref_node(entity_type='insight', entity_id=insight_id, project_id, label=title)`; treat a `Conflict` (already ingested / re-accept after archive) as a benign no-op. (New wiring — the shipped path does not do this today.)

- [ ] **Step 1: RED.** `crates/orchd/src/research/mod.rs` `#[cfg(test)]` (in-memory `Arc<Mutex<Db>>` + a real `Broadcaster` + a fake `connect_fn` returning a `FakeSession` — reuse `crate::mcp::test_support::FakeSession` seam): `start_run` returns `pending` + the idea is `researching`; drive the spawned driver to completion (await the JoinHandle the test holds, OR call the driver fn directly with the fake) — a fake success → run `done`+artifact_id+invocation_id (a durable `mcp_artifact`, is_untrusted=1) + a `ResearchRunsChanged` was broadcast; a fake `PolicyCapExceeded` → `failed{policy_cap_exceeded}`, NO artifact; a fake transport error → `failed{transport}`, invocation_id NULL. graph-ingest: `set_insight_status(accepted)` on a research insight seeds exactly one `entity_ref` node; a second accept → `Conflict` handled, still one node. Run `cargo test -p bpa-orchd research` → FAIL.
- [ ] **Step 2: GREEN.** Implement `start_run` + the driver against the `connect_fn`/`ToolCaller` seam (prod = `mcp::connect_session`); the driver is a free async fn the test can call directly (so it's unit-testable without a real spawn) AND `start_run` spawns it in prod. Wire graph-ingest into `set_insight_status`. Structured `tracing` on run transitions — run id, status, error_kind — NEVER args/secret/tool-output.
- [ ] **Step 3:** `cargo test -p bpa-orchd research` + full crate green; clippy/fmt. Commit `feat(orchd): research run driver (async, 3-phase, boot-reconcile-safe) + insight-accept graph-ingest (S-IDEA §6, D9)`.

### Task 5: dispatch + core commands + broker + register

**Files:** Modify `crates/orchd/src/socket_server.rs` (replace the temp arm; thread `Arc<Mutex<Db>>` + broadcaster into the research dispatch so the spawned driver has them), `src-tauri/src/{commands.rs,broker.rs,lib.rs}`.

**Interfaces — Consumes:** T3, T4. **Produces:** dispatch for the 3 research verbs — `ResearchStartRun` → `research::start_run(&deps.db, deps.broadcaster.clone(), …)` → reply `ResearchRun{pending}` (the driver pushes `ResearchRunsChanged` on each transition); `ResearchListRuns`→`ResearchRuns` (no push); `ResearchGetRun`→`ResearchRun` (no push); Err→map_err. Core: `research_start_run`/`research_list_runs`/`research_get_run` commands (proxy, map Error→Daemon); broker `ResearchRunsChanged`→`EV_ORCHD_RESEARCH_RUNS_CHANGED="orchd://research-runs-changed"` (camelCase `{ideaId}`, exhaustive); register the 3 commands.

- [ ] **Step 1: RED.** `dispatch_integration.rs`: `ResearchStartRun` against a loopback stub MCP server (reuse the S-EXT stub with a `research`/`echo` tool) → reply `ResearchRun{status:pending}` + a listener observes `ResearchRunsChanged` and (polling `ResearchGetRun`) the run reaches `done` with an artifact; `ResearchListRuns` returns it, no extra push on reads; a stub that returns a tool error → the run reaches `failed`. commands: `research_start_run` happy + an `Error{...}`→`CommandError::Daemon`. broker: `ResearchRunsChanged`→`orchd://research-runs-changed` camelCase `{ideaId}`. Run `cargo test -p bpa-orchd --test dispatch_integration` + `cargo test -p builder-pro-ai` → FAIL.
- [ ] **Step 2: GREEN.** Replace the temp dispatch arm; thread the broadcaster+db into `start_run`. Finalize the broker arm; add the 3 core commands + register. 
- [ ] **Step 3:** `cargo test -p bpa-orchd` + `cargo test -p builder-pro-ai` (stage sidecars) + `cargo build --workspace` green; clippy/fmt. Commit `feat(orchd+core): research dispatch + research_* commands + orchd://research-runs-changed (S-IDEA §5/§6)`.

### Task 6: frontend — research flow UI

**Files:** Modify `src/ipc/orchd.ts` + `events.ts` + `store/store.ts` + `components/IdeasList.tsx`; Create `src/components/idea/{ResearchRunDialog,ResearchPane,FormInsightDialog,SpawnProjectFromIdea}.tsx` + tests.

**Interfaces — Consumes:** T5. **Produces:** wrappers `researchStartRun/researchListRuns/researchGetRun`; `onOrchdResearchRunsChanged`; store `researchRunsByIdea` + `refreshResearchRuns(ideaId)`; App binds the event (unconditional refresh). Per spec §7:
- **ResearchRunDialog** — pick connected+enabled MCP server (store `mcpServers`) → `mcpListTools` → pick a tool → args JSON (seed from idea title/body) → spend-preflight (show `trustListPolicies` for the scope + honest cost-unknown note) → «Запустить» → `researchStartRun`.
- **ResearchPane** (per idea) — list runs (status badge); a `done` run → the ResearchArtifact viewer (`mcpGetArtifact(artifact_id)` + «непроверенные данные» untrusted banner, reuse the ArtifactsTab viewer); a `failed` run → the error_kind + «сформировать insight без ресёрча» (Q8).
- **FormInsightDialog** — title/body prefilled from the artifact (owner edits), `source="research-run:<id>"`; fit-context side panel (project goals w/ `metric_refs` + the idea/insight `GraphNeighborhood`); owner sets `fit_verdict`+`fit_reasoning` → `orchdCreateInsight` + `orchdSetInsightFitVerdict`. «Принять»→`orchdSetInsightStatus(accepted)`; «В backlog»→`orchdCreateTask{source:Insight, source_id, projectId}` (flips idea `researching→specced` via `orchdSetIdeaLifecycle`).
- **SpawnProjectFromIdea** — `pickFolder`→`createWorkspace`→`orchdCreateProject{name:idea.title, workspace_ids}`→`orchdSetIdeaProject`.
- IdeasList: per-idea «Исследовать»/«Создать проект» buttons + a research-run status badge. ALL mutating controls `disabled={orchdDown}` (click-asserted, the T8 discipline).

- [ ] **Step 1: RED.** jsdom + @testing-library/react + user-event, mock ipc + store: ResearchRunDialog fires `researchStartRun` with the picked server/tool/args + the spend-preflight renders the policy; ResearchPane renders run status + (done) the untrusted artifact banner + (failed) the «без ресёрча» affordance; FormInsightDialog shows fit-context (goals + graph neighborhood) + fires `orchdCreateInsight`/`orchdSetInsightFitVerdict`, accept→`orchdSetInsightStatus`, «В backlog»→`orchdCreateTask`; SpawnProjectFromIdea calls the 3 wrappers in order; store `refreshResearchRuns` replaces the idea's slice + `orchd://research-runs-changed` re-fetches; ALL mutating controls disabled while `orchdDown` (click-assert-not-called). Run `npx vitest run` → FAIL.
- [ ] **Step 2: GREEN + Step 3:** implement; `npx vitest run` + `npx tsc --noEmit` PASS. Commit `feat(ui): idea research flow — run dialog + research pane + form-insight (fit-context) + spawn-project (S-IDEA §7)`.

### Task 7: e2e (survival + boot-reconcile phases) + gate  [parallel-safe with T6]

**Files:** Modify `tests/e2e/orchd-survive.mjs` (2 new phases + codec for the research verbs); reuse/extend `tests/e2e/lib/stub-mcp-server.mjs` (add a `research` tool + a BLOCKING variant for the boot-reconcile phase).

**Interfaces — Consumes:** T5. **Produces:** the DoD e2e.
- [ ] **Step 1.** Extend the harness codec (`encodeOrchdRequest`/`decodeOrchdResponse`/`decodeOrchdPush`) for `ResearchStartRun`/`ResearchListRuns`/`ResearchGetRun` + `ResearchRun`/`ResearchRuns` + `ResearchRunsChanged` (snake_case frame; `default:throw`). **Survival phase:** CreateIdea → register+connect the stub (a `research` tool returning canned findings) → `ResearchStartRun` → poll `ResearchGetRun` until `done` → assert the `mcp_artifact` exists → `CreateInsight`+`SetInsightFitVerdict{fit}` → `SetInsightStatus{accepted}` → `CreateTask{source:insight}` → `OrchdShutdown{drain}` → relaunch → assert idea `specced`, run `done`+artifact, insight fit+accepted, task all survive. Log `phaseN OK: idea→research→insight→task survives restart`. **Boot-reconcile phase (D11):** a stub whose `research` tool BLOCKS → `ResearchStartRun` → poll until `running` (NOT done) → `OrchdShutdown{drain}` → relaunch → assert the run is `failed{interrupted}`. Log `phaseN OK: interrupted research run reconciled on restart`. `npm run e2e:orchd` → `ALL PHASES PASSED`.
- [ ] **Step 2.** `bash scripts/final-suite.sh` (RUST_TEST_THREADS=4, staged sidecars) → `ALL GATES PASSED` (orchd coverage ≥80%; retry-once ONLY the BL-40 attach flake). Commit `test(e2e): idea→research→insight→task survives restart + interrupted-run boot-reconcile (S-IDEA DoD) + gate green`.

### Task 8: docs truth + CHANGELOG [0.7.0] + backlog + gate

**Files:** `docs/architecture.md` (research pipeline in orchd + the async-run/boot-reconcile note), overview roadmap §3 (S-IDEA row → SHIPPED `[0.7.0]`, current-slice pointer moves, note the Q10/Q5/streaming deltas like prior rows; entity-map ResearchArtifact clarification), `README.md` (features + measured test counts + research mention), `CHANGELOG.md` (`[0.7.0]`), `docs/traceability.md` (S-IDEA rows: survives-restart→e2e survival phase; boot-reconcile→e2e reconcile phase + unit; connect-timeout→the T1 test), `docs/backlog.md` (the spec §10 deltas: S6a LLM verdict, token-streaming pane [BL-70], agent decomposition, ResearchArtifact provenance viewer, run cancel/retry, S8 metrics, Q5 prowl session-id adapter, D11 JoinHandle-drain-tracking), `docs/frontend-conventions.md` (the idea research flow + untrusted banner reuse), `docs/runbook-daemon.md` (the research-run + real-prowl §9 Human step).
- [ ] `git grep -niE 'todo|tbd' docs/architecture.md` empty; README counts re-measured (`cargo test --workspace`/`npx vitest run`); `bash scripts/final-suite.sh` → `ALL GATES PASSED`. Commit `docs: S-IDEA shipped — ideas + research pipeline, CHANGELOG [0.7.0]`.

### Task 9: whole-branch review + merge + CI green
- [ ] `scripts/review-package MERGE_BASE HEAD` → final whole-branch review (most-capable model): the async-run correctness (3-phase locking, boot-reconcile covers every mid-flight loss, connect-timeout, no leaked task), the `(status='done')=(artifact_id NOT NULL)` CHECK holds across all transitions, no secret/arg in `error_kind`/logs, provenance semantics (invocation_id on done / NULL-on-Mcp-failure documented), graph-ingest Conflict-benign, honest degradation (Q8) + honest pending-not-streaming, reuse-only-verbs (no accidental net-new wire beyond the 3 research verbs), spec §§1–10 completeness. Fix Critical/Important (one fix subagent, full list). → finishing-a-development-branch: ff-merge → main, push, **watch CI green** (stage sidecars; retry-once BL-40; the S-EXT ci.yml keychain step already covers keychain paths — the research stub is no-auth).

---

## Self-review notes (author)

- **Spec coverage:** §4 DDL→T2; §5 wire→T3+T5; §6 run state machine→T4 (+ D12 connect-timeout T1, D11 boot-reconcile T2); §7 UI→T6; §8 tests→per-task TDD + T7 e2e (survival + boot-reconcile phases); §9 human steps→T8 docs; §10 backlog→T8. D1(orchd)✓ D2(reuse mcp_artifact)→T4 D3(async 3-phase)→T4 D4(owner verdict)→T6 D5(spend preflight)→T6 D6(reuse verbs)→T5/T6 D7(schema v4)→T2 D8(append-only)→T3 D9(graph-ingest)→T4 D10(Q8)→T4/T6 D11(boot-reconcile)→T2 D12(connect-timeout)→T1 D13(Q5)→doc/T6.
- **No placeholders:** each task names exact files, produces/consumes signatures, a concrete first failing test. Verbatim DDL/wire live in the spec (§4/§5), referenced (DRY).
- **Type consistency:** `ResearchRunRow`(persistence) ↔ `ResearchRun`(proto entity) ↔ `researchRunsByIdea`(store); `ResearchStatus` enum values pending/running/done/failed consistent across DDL CHECK, proto, TS; `start_run`/driver signature (`Arc<Mutex<Db>>`+broadcaster, `connect_fn` seam) matches the shipped `mcp::invoke::call_tool` it calls.
- **Parallel safety:** T1–T5 sequential (contracts+module+dispatch); T6 (frontend) ‖ T7 (e2e) are the only parallel group — disjoint files (src/ vs tests/e2e/); no two parallel tasks write the same file.
