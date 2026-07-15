# S-IDEA — Ideas + Research Pipeline — Design Spec

> Slice: **S-IDEA** (roadmap §3, "first user-visible v2 value"). Host: **`bpa-orchd`** (domain
> orchestration; reuses the S-EXT MCP invoke path — no new egress). The end-to-end loop
> **idea → research → evaluated insight → task in backlog**, WITHOUT the S6 agent org. Terminal
> artifact of the brainstorming cycle. Date: 2026-07-15. Deps S3+S4+S-EXT all SHIPPED.

## 1. Goal & scope

Turn a captured idea into a researched, owner-evaluated insight and a backlog task — end to end,
driven by the owner (no agent runtime). Research runs by invoking an owner-connected MCP research
tool (e.g. prowl.chat) through the S-EXT MCP client; the durable result is reviewed, distilled into
an insight the owner judges for fit against the project's goals/metrics, and (if accepted) formed
into a task. This is the first slice that stitches the shipped primitives (Idea/Insight/Task,
MCP tool calls + durable artifacts, the knowledge graph, projects/workspaces) into one loop.

**In scope (full S-IDEA row):**
- **Idea inbox + lifecycle** — reuse the shipped `idea` entity (`lifecycle` captured→researching→specced→in_dev→shipped→archived), ⌘K QuickCapture, IdeasList/«Идеи» tab. Add the surfaces that drive the loop.
- **Research run** — NET-NEW `research_run` (schema v4): an idea + a chosen MCP server/tool + args → an asynchronous run (pending→running→done|failed) that invokes the tool via the existing `mcp::invoke` path and records the resulting **durable `mcp_artifact`** as the ResearchArtifact (no blob duplication). Provenance: run links idea↔invocation↔artifact.
- **Inline spend-approval preflight** — before a run, the owner sees a confirm dialog with the effective spend/rate policy state (reuse `TrustListPolicies`) and an honest note that the external tool's cost is usually unknown until after the call; the owner confirms. The trust layer still hard-enforces caps at invoke time (a cap breach → run `failed{PolicyCapExceeded}`).
- **Research pane + ResearchArtifact viewer** — a pending/running/done pane (NOT token-streaming — MCP `tools/call` is request/response in the connect-per-call model; honest degradation) + the durable artifact viewer (reuse the S-EXT artifact viewer + the «непроверенные данные» untrusted banner).
- **Insight formation + owner fit-verdict** — from a done run, the owner forms an `Insight` (reuse `CreateInsight`, `source` = a `research-run:<id>` provenance string), sees **fit-context** (the project's goals with `metric_refs` + the idea/insight graph neighborhood via `GraphNeighborhood`) beside it, and sets `fit_verdict`(fit|no_fit|unknown) + `fit_reasoning` (reuse `SetInsightFitVerdict`) and `status`(accepted|archived) + `resolution_reasoning` (reuse `SetInsightStatus`). **Owner-driven; NO LLM.** Auto-archive of a clear non-fit keeps reasoning (owner action).
- **Task formation** — an accepted insight → a backlog `Task` (reuse `CreateTask{source:Insight, source_id:insight_id}`). Owner task decomposition = create subtasks (reuse `CreateTask{parent_id}`).
- **Spawn-project-from-idea** — for a project-less idea, a flow that creates a workspace (sessiond `CreateWorkspace`, owner picks a folder), a project (`CreateProject{workspace_ids}`), and links the idea (`SetIdeaProject`). Pure frontend orchestration over EXISTING verbs — no new orchd verb.
- **Graph-ingest** — a research-formed insight is seeded as a graph `entity_ref` node (reuse `add_entity_ref_node`) so it appears in the knowledge graph (overview §7 "ResearchArtifact graph-ingested"), the modest honest interpretation.
- **Lifecycle orchestration** — starting a run flips the idea `captured→researching`; forming a task from its insight flips `researching→specced`. Honest, owner-visible transitions.

**Non-goals / deferred (explicit):**
- **LLM-computed fit-verdict** — deferred to S6a (native LLM provider layer, not built). v1 verdict is owner-set; the schema field already exists. Filed as the S6a follow-up.
- **Token-streaming research pane** — MCP `tools/call` is request/response; v1 shows run status (pending/running/done), not streamed tokens. (A live-streaming pane needs a streaming tool + persistent session — backlog, aligns with the S-EXT list_changed/persistent-session item.)
- **Agent task-decomposition** — owner creates subtasks manually; automated decomposition is S6b.
- **A new research-tool contract** — the research tool is whatever MCP tool the owner's connected server exposes (discovered via the shipped `tools/list`); v1 does not hardcode a prowl-specific schema. The owner picks server+tool+args.
- **Metrics ingestion** — `goal.metric_refs` is shown as-is (owner-declared strings) for fit-context; real metric timeseries is S8.

## 2. Locked decisions

| # | Decision |
|---|---|
| **D1** | **Host = `bpa-orchd`.** Research orchestration is domain logic that CALLS the shipped `mcp::invoke::call_tool` path — no new egress, no new crate. |
| **D2** | **Thin `research_run`, reuse `mcp_artifact`.** The ResearchArtifact is the existing durable `mcp_artifact` produced by the run's tool call; the `research_run` row is the provenance link (idea↔invocation↔artifact) + status. No blob duplication, one source of truth. |
| **D3** | **Async run with 3-phase locking.** `ResearchStartRun` creates the run `pending`, spawns a background task (orchd's first background job), returns immediately. The task calls `mcp::invoke::call_tool` (already lock-safe — never holds the DB lock across the network await, S-EXT T6 lesson), then updates the run `done`/`failed` and pushes `ResearchRunsChanged`. The frontend shows pending→done via the push (honest "research pane"). |
| **D4** | **Owner-driven fit-verdict; NO LLM** (**overrides Q10's "agent-computed" default** — S6a, the native LLM provider layer, is not built/in-scope, and the DoD requires the loop to work "WITHOUT the S6 agent org", of which S6a is a member). Reuse `insight.fit_verdict`/`fit_reasoning`/`status`/`resolution_reasoning`. Fit-context (goals+metric_refs + graph neighborhood) is shown beside the insight; the owner decides. LLM auto-scoring → S6a follow-up (filed §10). |
| **D5** | **Spend-approval is a UI preflight + confirm, not a new backend gate.** The dialog shows the effective policy (`TrustListPolicies`) and an honest "cost unknown until after the call" note; the owner confirms. The trust layer's existing hard cap enforcement is unchanged — a breach at invoke time surfaces as run `failed{PolicyCapExceeded}` (Q8 honest degradation: the owner may still form an insight/task without research). |
| **D6** | **Reuse verbs for everything but the run.** spawn-project (`CreateWorkspace`+`CreateProject`+`SetIdeaProject`), insight formation (`CreateInsight`+`SetInsightFitVerdict`+`SetInsightStatus`), task formation (`CreateTask{project_id, source:Insight, source_id:insight_id}` — `task.project_id` is NOT NULL, always required), decomposition (`CreateTask{parent_id}`) all reuse SHIPPED verbs. Read-side spend-preflight reuses `TrustListPolicies` (a SHIPPED S-EXT verb — added in T18, `orchd-proto` + dispatch present; NOT net-new). The ONLY net-new wire is the three research verbs. |
| **D7** | **Additive schema v4.** `SCHEMA_VERSION` 3→4, one additive `Migration{upto:4}` (whole-chain single-tx, forward-only — the established `bpa_daemon_core::migrate` contract). New `research_run` table only. |
| **D8** | **orchd-proto append-only.** New `Research*` request/response variants + `ResearchRunsChanged` push appended at the END of the frozen enums. orchd version space stays `[1,1]` (additive). |
| **D9** | **Graph-ingest the insight, not the raw artifact.** On insight-accept from a research run, seed one `entity_ref` node (`entity_type='insight'`) via the existing internal `add_entity_ref_node` (dedup by the partial-unique index). The raw untrusted `mcp_artifact` is NOT graph-ingested (it's untrusted external data; the owner-curated insight is). |
| **D10** | **Honest degradation (Q8).** If the research server is down / the tool errors / a cap denies → the run ends `failed` with the reason; the UI offers «сформировать insight/задачу без ресёрча» (the owner path stays open). No fake success, no silent retry (the owner re-runs). |
| **D11** | **Boot-reconcile (crash/shutdown safety — the async-run's #1 risk).** The background run task is a detached `tokio::spawn` NOT tracked by the shutdown drain (`socket_server`'s `conns` JoinSet), so a restart/crash/drain mid-run would leave the row stuck `running`/`pending` forever. Fix: a boot-reconcile step (same "ensured at every boot" pattern as `ensure_global_ruleset` in `boot::run`, right after `open_db`) runs `UPDATE research_run SET status='failed', error_kind='interrupted', updated_at=? WHERE status IN ('pending','running')`. Any run not terminal (`done`/`failed`) at boot is stale → `failed{interrupted}`; the owner re-runs. This is the authoritative backstop for every mid-flight loss. (Additionally, track the spawned `JoinHandle` so `OrchdShutdown{drain}` best-effort awaits it with a short timeout — nice-to-have; boot-reconcile is the correctness guarantee.) |
| **D12** | **Bound the connect handshake (hang-forever fix in the shipped path).** `mcp::invoke::call_tool` today wraps only the `tools/call` RPC in `timeout(server.timeout_ms)`, NOT the preceding `connect_fn(...).await` (the MCP `initialize` round-trip). A research peer that accepts the connection but never completes `initialize` (dead peer / silent firewall drop / overloaded stdio child) hangs the background task forever. Fix (small, localized to the shipped S-EXT `invoke.rs`): wrap `connect_fn(...).await` in `timeout(server.timeout_ms)` → `McpError::Timeout` on elapse. Benefits every MCP call, not just research. |
| **D13** | **Q5 (prowl `session_id` per run) — v1 override.** Each `research_run` IS one tool call = one run-scoped invocation, so run-level isolation holds. The prowl-specific `session_id` lifecycle is entirely tool-defined and owner-supplied via `args_json` (v1 does not hardcode a prowl schema, per Non-goals); BPA does NOT generate or enforce a session id. Filed to backlog: a prowl-aware convenience adapter that auto-seeds a fresh `session_id` when the picked tool's schema exposes that key. |

## 3. Architecture & module layout

```
crates/orchd/
  src/research/mod.rs        NEW — ResearchRun row + status enum + CRUD (start/get/list) + the
                             background-run driver (spawn → mcp::invoke::call_tool → update+push)
  src/persistence.rs         SCHEMA_VERSION 3→4 + Migration{upto:4} (§4 DDL); research_run CRUD
  src/socket_server.rs       dispatch arms for the Research* verbs + ResearchRunsChanged fan-out;
                             the background task needs the broadcaster + Arc<Mutex<Db>> (thread them in)
  src/lib.rs                 pub mod research;
crates/orchd-proto/src/lib.rs  append-only: ResearchStartRun/ResearchListRuns/ResearchGetRun verbs
                             + ResearchRun entity + ResearchRunsChanged push; ts-rs → orchd-types.ts
src-tauri/src/
  commands.rs                research_start_run / research_list_runs / research_get_run (proxy)
  broker.rs                  ResearchRunsChanged → orchd://research-runs-changed {ideaId?}
  lib.rs                     register commands
src/
  ipc/orchd.ts + events.ts   researchStartRun/ListRuns/GetRun wrappers + onOrchdResearchRunsChanged
  store/store.ts             researchRunsByIdea slice + refreshResearchRuns + coarse-invalidation bind
  components/idea/           NEW: ResearchRunDialog (pick server+tool+args + spend-approval preflight),
                             ResearchPane (pending/running/done + artifact viewer + «непроверенные данные»),
                             FormInsightDialog (title/body from artifact + fit-context: goals+graph neighborhood
                             + owner fit_verdict/reasoning + accept→CreateTask), SpawnProjectFromIdea flow
  components/IdeasList.tsx    extend: per-idea «Исследовать» / «Создать проект» / lifecycle + research-run badge
```

**Boundaries.** `research/mod.rs` orchestrates; it does NOT open egress (calls `mcp::invoke`). The background run task is the only long-lived job — bounded (one per run, updates one row on completion, 3-phase-locked). The frontend flow reuses the S-EXT artifact viewer + the S4 graph neighborhood component for fit-context.

## 4. Data model — `orchd.db` schema v4 (additive)

`SCHEMA_VERSION` 3→4; one `Migration{upto:4}`, additive-only:

```sql
CREATE TABLE research_run (
  id             TEXT PRIMARY KEY,             -- uuid v4
  idea_id        TEXT NOT NULL,                -- FK -> idea(id) ON DELETE CASCADE
  server_id      TEXT NOT NULL,                -- FK -> mcp_server(id) ON DELETE CASCADE (the research MCP server)
  tool_name      TEXT NOT NULL,               -- the owner-chosen tool on that server
  args_json      TEXT NOT NULL DEFAULT '{}',   -- the invocation args (owner-supplied; NOT a secret)
  status         TEXT NOT NULL DEFAULT 'pending', -- 'pending'|'running'|'done'|'failed'
  invocation_id  TEXT,                          -- FK -> mcp_invocation(id); set on 'done'. Best-effort on 'failed':
                                                --   NULL for policy_cap_exceeded/consent (no invocation row written),
                                                --   and NULL for the Mcp(_) failure family too — call_tool records an
                                                --   mcp_invocation for those but does NOT return its id (shipped error
                                                --   type carries no id; see §6). Accepted partial-provenance: a failed
                                                --   run's record is its error_kind, not an invocation link.
  artifact_id    TEXT,                          -- FK -> mcp_artifact(id); set on 'done' ONLY
  error_kind     TEXT,                          -- set on 'failed' (e.g. 'policy_cap_exceeded','tool_error','transport','timeout','interrupted'); NEVER a secret/arg/tool-output
  created_at     INTEGER NOT NULL,
  updated_at     INTEGER NOT NULL,
  FOREIGN KEY(idea_id)   REFERENCES idea(id)       ON DELETE CASCADE,
  FOREIGN KEY(server_id) REFERENCES mcp_server(id) ON DELETE CASCADE,
  CHECK ( status IN ('pending','running','done','failed') ),
  CHECK ( (status='done') = (artifact_id IS NOT NULL) )   -- a done run has an artifact; others don't
);
CREATE INDEX research_run_by_idea ON research_run(idea_id, created_at);
```

Idempotent-migration note: additive table only; a v3→v4 upgrade of a live `orchd.db` creates the table, seeds nothing.

**Transition atomicity (CHECK footgun):** each status transition MUST be a single `UPDATE` that sets `status` together with `artifact_id`/`invocation_id`/`error_kind` — never two statements (setting `artifact_id` then `status='done'` in separate writes would momentarily violate `(status='done')=(artifact_id IS NOT NULL)`). `start_research_run` (insert `pending` + the `captured→researching` idea flip) is ONE `unchecked_transaction()` per the codebase idiom (`create_project`/`set_idea_lifecycle`), so a concurrent `DeleteIdea` (FK `ON DELETE CASCADE` on `idea_id`) can't interleave a half-completed reply. **Concurrent runs per idea are allowed** (each run is its own row; `research_run_by_idea` is non-unique) — a deliberate decision, not an oversight.

## 5. Wire protocol — `orchd-proto` (append-only)

New `OrchdRequest` variants (appended at END; snake_case Hop-B frame fields — NOT ts-rs):
```
ResearchStartRun { idea_id, server_id, tool_name, args_json } -> ResearchRun(ResearchRun)   // creates run{pending}, spawns the bg task; the push delivers the terminal state
ResearchListRuns { idea_id } -> ResearchRuns(Vec<ResearchRun>)                              // runs for an idea, newest first
ResearchGetRun { id } -> ResearchRun(ResearchRun)
```
New ENTITY struct (camelCase + ts-rs, i64→`#[ts(type="number")]`, mirroring `McpArtifact`'s derive):
`ResearchRun { id, idea_id, server_id, tool_name, args_json, status: ResearchStatus, invocation_id?, artifact_id?, error_kind?, created_at, updated_at }`, enum `ResearchStatus{Pending,Running,Done,Failed}` (camelCase wire `pending`/`running`/`done`/`failed`).
New `OrchdResponse` variants: `ResearchRun(ResearchRun)`, `ResearchRuns(Vec<ResearchRun>)`.
New `OrchdPush`: `ResearchRunsChanged { idea_id: Option<String> }` → frontend event `orchd://research-runs-changed`.

Everything else reuses shipped verbs: `CreateWorkspace`(sessiond), `CreateProject`/`SetIdeaProject`/`SetIdeaLifecycle`/`CreateInsight`/`SetInsightFitVerdict`/`SetInsightStatus`/`CreateTask`, `McpListTools`(to pick the research tool), `McpGetArtifact`(view the artifact), `GraphNeighborhood`(fit-context), `TrustListPolicies`(spend preflight).

## 6. Orchestration — the research-run state machine

`research::start_run(deps, idea_id, server_id, tool_name, args_json) -> ResearchRun`:
1. Validate the idea exists + the server exists+enabled; insert `research_run{status:pending}`.
2. Flip `idea.lifecycle`: `captured`→`researching` (only if currently `captured`; leave later states).
3. `tokio::spawn` the run task (holds `Arc<Mutex<Db>>` + the broadcaster, NOT a `&Db` — 3-phase discipline):
   a. lock→set `status:running`, push `ResearchRunsChanged`→unlock.
   b. call `mcp::invoke::call_tool(db, server_id, tool_name, args_json, project_id = idea.project_id)` — this is the SHIPPED path: it authorizes (consent + spend/rate caps), invokes with retry/timeout, records `mcp_invocation` + (on success) the durable `mcp_artifact` (is_untrusted=1). NO DB lock held across its network await (it's already 3-phase). **Per D12, `call_tool` is amended so the `connect_fn`/`initialize` handshake is ALSO wrapped in `timeout(server.timeout_ms)`** — so a research peer that stalls at connect can't hang this task forever (and D11's boot-reconcile is the cross-restart backstop). Each transition (c/d) is a SINGLE `UPDATE` (§4 CHECK atomicity).
   c. on `Ok(McpCallResult)`: lock→set `status:done, invocation_id, artifact_id` (one UPDATE), push→unlock.
   d. on `Err`: lock→set `status:failed, error_kind` (map the typed error: `PolicyCapExceeded`→`policy_cap_exceeded`, `Timeout`→`timeout`, tool/transport→their kinds via the shipped `classify_error_kind`), push→unlock. `invocation_id` stays NULL on `Mcp(_)`-family failures (the shipped error carries no id — §4, accepted partial-provenance). NEVER store args/secret/tool-output in `error_kind`.
4. Return the `pending` run immediately (the frontend subscribes to the push for the terminal state).

`ResearchStartRun` reply is the `pending` run; `ResearchRunsChanged{idea_id}` fires on each transition. `ResearchListRuns`/`ResearchGetRun` are plain reads (no push).

**Graph-ingest (D9)** is NOT in the run path — it happens when the owner ACCEPTS the derived insight: the insight-accept UI calls the existing `SetInsightStatus{accepted}`; orchd's `set_insight_status` handler additionally seeds an `entity_ref` node for the insight. **This is NEW wiring** — code-truth check: `add_entity_ref_node` exists (`graph.rs`, with `GraphEntityType::Insight` + the `graph_node_one_per_entity` partial-unique dedup index), but is called only from graph.rs's own tests today; the shipped `SetInsightStatus` path does NOT seed a node — S-IDEA adds that call. Treat `add_entity_ref_node`'s `Conflict` (re-accept after archive — archiving doesn't remove the node, S4 orphan-on-delete model) as a **benign no-op**, not an error. If touching the shipped insight handler is undesirable, an explicit `ResearchIngestInsight{insight_id}` verb is the fallback — but prefer folding it into accept.

## 7. Frontend — the idea→research→insight→task flow

Hangs off the shipped «Идеи» surface (`IdeasList` in the ProjectPanel «Идеи» tab AND the project-less idea inbox). Per idea card:
- **«Исследовать»** → `ResearchRunDialog`: pick a connected+enabled MCP server → `McpListTools` → pick a tool → an args JSON field (owner-supplied; can seed from the idea title/body) → a **spend-approval preflight** panel (shows the effective `TrustListPolicies` for the scope + «стоимость внешнего вызова обычно неизвестна до вызова; текущий лимит: …») → «Запустить». Fires `researchStartRun`.
- **`ResearchPane`** (per idea): lists `research_run`s (status badge pending/running/done/failed); a done run → the ResearchArtifact viewer (fetch via `mcpGetArtifact(artifact_id)`, render content + «непроверенные данные» untrusted banner); a failed run → the error + «сформировать insight без ресёрча» (Q8).
- **«Сформировать insight»** (from a done run OR the degraded path) → `FormInsightDialog`: title/body prefilled from the artifact (owner edits), `source` = `research-run:<id>`; a **fit-context** side panel: the project's goals (with `metric_refs`) + the idea/insight `GraphNeighborhood`; owner sets `fit_verdict`(fit/no_fit/unknown) + `fit_reasoning`. Creates the insight (`CreateInsight`) + `SetInsightFitVerdict`.
- **«Принять» / «Архив»** on the insight → `SetInsightStatus`(accepted/archived) + `resolution_reasoning`; on accept, graph-ingest (D9).
- **«В backlog»** on an accepted insight → `CreateTask{source:Insight, source_id, projectId}` → flips the idea `researching→specced`. Owner may add subtasks (`CreateTask{parent_id}`).
- **«Создать проект из идеи»** on a project-less idea → the spawn flow: `pickFolder`→`createWorkspace`(sessiond)→`orchdCreateProject{name:idea.title, workspace_ids}`→`orchdSetIdeaProject`.
- All mutating controls `disabled` while `orchdDown` (honest degradation, S3/S4/S-EXT discipline).

## 8. Testing strategy & DoD

- **orchd `research` unit** (in-memory Db + a fake `ToolCaller`/`connect_fn` seam like the S-EXT `mcp::invoke` tests): `start_run` inserts `pending` + flips idea `captured→researching` in ONE transaction; the run task on a fake success sets `done`+artifact_id+invocation_id (artifact is the durable `mcp_artifact`, is_untrusted=1) + pushes; on a fake `PolicyCapExceeded` → `failed{policy_cap_exceeded}` + NO artifact + push; on a transport error → `failed{transport}` (invocation_id NULL — accepted); the CHECK `(status='done')=(artifact_id NOT NULL)` holds; `error_kind` never contains args/secret. `list_runs`/`get_run`. Graph-ingest: accepting a research-insight seeds exactly one `entity_ref` node; re-accept after archive → `Conflict` handled as benign no-op (still one node).
- **boot-reconcile (D11)**: open a Db, insert a `research_run{running}` + a `{pending}` directly, run the boot-reconcile step, assert both are now `failed{interrupted}` and a `done`/`failed` row is untouched.
- **connect-timeout (D12)**: extend the shipped `mcp::invoke` tests — a `connect_fn` that never resolves (a pending future) → `call_tool` returns `McpError::Timeout` within ~`timeout_ms`, not a hang (use a short `timeout_ms` + `tokio::time` pause/advance or a real sub-second timeout).
- **orchd socket**: `ResearchStartRun` → `ResearchRun{pending}` + a listener eventually gets `ResearchRunsChanged` with the run reaching `done` (drive via a loopback stub MCP server, reuse the S-EXT dispatch-test stub); `ResearchListRuns` returns it, no extra push on reads.
- **core**: `research_*` commands proxy + error-map; broker `ResearchRunsChanged`→`orchd://research-runs-changed` camelCase `{ideaId}`.
- **frontend**: ipc wrapper name/arg parity; store `researchRunsByIdea` refresh + coarse-invalidation bind; ResearchRunDialog fires `researchStartRun` with the picked server/tool/args + the spend-preflight shows the policy; ResearchPane renders status + the untrusted artifact banner; FormInsightDialog shows fit-context (goals + graph neighborhood) and fires CreateInsight/SetInsightFitVerdict; accept→CreateTask; spawn-project flow calls the 3 existing wrappers in order; ALL mutating controls disabled while `orchdDown` (click-asserted, the T8 discipline).
- **e2e (the DoD)** — a new phase in the orchd e2e harness against a LOCAL stub MCP research server (reuse the S-EXT stub, add a `research` tool returning canned findings): CreateIdea → register+connect the stub server → grant consent → `ResearchStartRun` → poll `ResearchGetRun` until `done` → assert the `mcp_artifact` exists (durable) → form an Insight (`CreateInsight`+`SetInsightFitVerdict{fit}`) → accept → `CreateTask{source:insight}` → `OrchdShutdown{drain}` → relaunch → assert the idea (lifecycle `specced`), the research_run (`done`+artifact_id), the insight (fit+accepted), and the task all SURVIVE. Log `phaseN OK: idea→research→insight→task survives restart`.
- **e2e boot-reconcile phase (D11)** — a SECOND phase that exercises the in-flight-at-restart race the survival phase deliberately avoids: register a stub server whose `research` tool BLOCKS (never returns / returns after a long delay) → `ResearchStartRun` → poll until the run is `running` (NOT `done`) → `OrchdShutdown{drain}` → relaunch → assert the run came back **`failed{interrupted}`** (boot-reconcile fired), not stuck `running`. Log `phaseN OK: interrupted research run reconciled to failed on restart`.
- **gate**: `bash scripts/final-suite.sh` → `ALL GATES PASSED` (9 stages); orchd coverage ≥80%; ts-rs parity; RUST_TEST_THREADS=4; retry-once ONLY the known BL-40 attach flake; the CI keychain-unlock step (S-EXT T19) already covers any keychain-touching path (the research stub is no-auth, so likely none). No env-fragile timing asserts (S4 lesson — the run test uses a fake seam, not wall-clock).

**DoD (roadmap row):** idea → prowl research → evaluated insight → task lands in the backlog, end-to-end, WITHOUT the S6 agent org (proven by the e2e phase against the stub; the real prowl.chat server is the §9 Human step). Metric enabler: ideas reaching «specced».

## 9. Human steps (residual — end, non-blocking to the autonomous path)

- **Real prowl.chat research server**: the owner connects prowl.chat as an MCP server (S-EXT «Серверы» UI) + its research tool. The autonomous path proves the whole loop against a local stub; wiring real prowl is: owner adds the server + key, then picks its research tool in `ResearchRunDialog`. One block, at the very end.
- **Notarized signed build** (unchanged from prior slices) — credential-gated; not on the test path.

## 10. Backlog deltas (filed by the docs task)

LLM-computed fit-verdict (S6a — overrides Q10 once the provider layer ships); token-streaming research pane (needs a streaming tool + persistent session — aligns with the S-EXT persistent-session backlog item BL-70); automated agent task-decomposition (S6b); a first-class ResearchArtifact provenance viewer beyond the reused mcp_artifact viewer; research-run cancel/retry controls; metric timeseries for fit-context (S8, today `metric_refs` strings only); a prowl-aware convenience adapter that auto-seeds `session_id` into a research call's args (Q5 v1-override, D13); JoinHandle-tracking so `OrchdShutdown{drain}` best-effort awaits in-flight research tasks (D11 nice-to-have — boot-reconcile is the correctness backstop).
