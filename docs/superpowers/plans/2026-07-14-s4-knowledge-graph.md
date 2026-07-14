# S4 — Knowledge Graph: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a knowledge graph (typed nodes/edges, per-project + cross-project) to the orchd store with a workspace-wide agent retrieval API and an editable `@xyflow/react` canvas as a 7th ProjectPanel tab. Unblocks S6.

**Architecture:** All in existing crates — `crates/orchd` (schema v2 + `graph.rs` persistence/retrieval + dispatch), `crates/orchd-proto` (append graph entities/verbs/push + ts-rs), `src-tauri` (graph commands + broker event), `src/` (orchd.ts wrappers + store slice + pure `graphMapping` helpers + thin `GraphCanvas` + tab). sessiond/daemon-core/paths/protocol UNTOUCHED. Spec is authoritative: `docs/superpowers/specs/2026-07-14-s4-knowledge-graph-design.md` (D1-D12, §3 wire, §4 DDL, §5 retrieval, §7 frontend).

**Tech Stack:** existing orchd stack (`rusqlite`, `ciborium`, `ts-rs`, `uuid`, `sha2`, `tokio`) + ONE new frontend dep `@xyflow/react` v12 (Context7-verified; package `@xyflow/react`, controlled API, CSS `@xyflow/react/dist/style.css`).

## Global Constraints

- orchd-proto graph entities/verbs/pushes copied VERBATIM from spec §3 (order FROZEN append-only — new variants at the END of OrchdRequest/OrchdResponse/OrchdPush); orchd version space stays `[1,1]` (additive verbs, no bump).
- orchd.db schema v2 = spec §4 DDL VERBATIM, applied as a second `Migration { upto: 2 }` appended to the existing table in `crates/orchd/src/persistence.rs`; `SCHEMA_VERSION` 1→2; the v2 step ALSO runs the idempotent strategic-goal entityRef backfill for pre-S4 projects. `foreign_keys=ON` (already set both paths).
- Enum⇄TEXT: DB stores snake_case §4 CHECK literals (`entityRef` is the one camelCase literal — matches DDL); wire/TS is camelCase; persistence owns the mapping via explicit helpers (S3 pattern, NEVER serde for the DB rep).
- Retrieval is WORKSPACE-WIDE: `neighborhood`/`search_nodes(project_id: None)` traverse cross-project; `neighborhood` depth clamped ≤6; a perf test asserts <100 ms on a 500-node graph.
- entityRef nodes are SOFT-refs (no FK to domain tables); deleting a domain entity must NOT delete/corrupt the node; label re-resolved at read time, orphan flagged.
- Every mutating verb: archived-project guard (both endpoints for edges) ⇒ `Invariant`; failed verb broadcasts NO push; cross-project edge change ⇒ `GraphChanged` for BOTH projects; content never logged; all Rust tests hermetic (temp HOME, never the real app-support).
- Frontend: vitest is `environment: "node"`, NO setupFiles — `GraphCanvas.tsx` (mounts ReactFlow) is NOT rendered in any test; ALL tested logic is in pure `src/components/graph/graphMapping.ts`. Mutating controls `disabled` while `orchdDown`.
- `#[tauri::command]` = thin wrapper over a testable inner fn; command names = `orchd_graph_*`; frontend wrappers match verbatim; event const `orchd://graph-changed`.
- Version consts at every preamble site (unchanged — no new client); ts-rs `src/ipc/orchd-types.ts` regenerated + gate-diffed.
- Gate: `bash scripts/final-suite.sh` → `ALL GATES PASSED` (9 stages; the orchd e2e gains a cross-project-survival phase; orchd coverage ≥80%). Commits conventional + trailer `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`. Fix waves use follow-up commits (not amend). Work in worktree `worktree-s4`, never main.

## Task graph

Sequential contracts: T1 (proto) → T2 (schema v2 + graph.rs persistence) → T3 (retrieval + perf) → T4 (dispatch+push) → T5 (core commands+broker) → T6 (frontend ipc+store+pure mapping) → T7 (GraphCanvas + tab) → T8 (e2e phase + gate) → T9 (docs + CHANGELOG) → T10 (whole-branch review + merge). Mostly sequential (each builds on the prior); T2/T3 share graph.rs so are ordered.

---

### Task 1: orchd-proto graph entities + verbs + push + ts-rs

**Files:** Modify `crates/orchd-proto/src/lib.rs`; regenerate `src/ipc/orchd-types.ts`; extend `crates/orchd-proto/tests/roundtrip.rs` + `tests/ts_export.rs`.
**Produces (spec §3 VERBATIM):** entities `GraphNode`, `GraphNodeKind{Concept,Fact,Artifact,Decision,Note,EntityRef}`, `GraphEntityType{Goal,Idea,Insight,Task,Ruleset}`, `GraphEdge`, `GraphEdgeKind{Relates,Depends,Derives,Supports,Contradicts,Parent}`, `GraphView{nodes,edges,external_nodes}`, `GraphNeighborhood{root_id,nodes,edges}` (all `#[derive(…, TS)]` camelCase, `#[ts(export_to="orchd-types.ts")]`); appended `OrchdRequest::{GraphAddNode,GraphUpdateNode,GraphMoveNode,GraphDeleteNode,GraphAddEdge,GraphDeleteEdge,GraphListProject,GraphNeighborhood,GraphSearch}` (exact fields per spec §3), `OrchdResponse::{GraphNode,GraphEdge,GraphView,Neighborhood,GraphNodes}`, `OrchdPush::GraphChanged{project_id}`. All at the END of their enums.

- [ ] **Step 1: RED.** roundtrip.rs: CBOR encode/decode one instance of every new Request/Response/Push variant (non-default fields); serde-string asserts (`GraphNodeKind::EntityRef`⇒`"entityRef"`, `GraphEdgeKind::Contradicts`⇒`"contradicts"`, `GraphEntityType::Ruleset`⇒`"ruleset"`). ts_export: `orchd-types.ts` contains `GraphNode`/`GraphEdge`/`GraphView`/`GraphNeighborhood` with camelCase (`projectId`,`sourceNodeId`,`posX`,`entityType`,`externalNodes`,`rootId`). FAIL.
- [ ] **Step 2: GREEN.** Add types + variants; run ts export; commit the regenerated `src/ipc/orchd-types.ts`.
- [ ] **Step 3:** `cargo test -p bpa-orchd-proto` + second regen `git diff --exit-code -- src/ipc/orchd-types.ts` → PASS; clippy/fmt clean. Commit `feat(orchd-proto): S4 graph wire — nodes/edges/view/neighborhood verbs + GraphChanged (spec §3, frozen append-only)`.

### Task 2: schema v2 + graph persistence (CRUD)

**Files:** Create `crates/orchd/src/graph.rs` (+ `mod graph;` in lib.rs); modify `crates/orchd/src/persistence.rs` (SCHEMA_VERSION→2, append `Migration{upto:2}` with the §4 DDL + backfill, and call `graph::seed_strategic_entity_ref` inside `create_project`'s tx).
**Consumes:** T1 entities. **Produces (spec §5):** `Db` methods (or `graph::` fns taking `&Db`/`&Connection`, matching the file's style): `add_node`, `add_entity_ref_node`, `update_node`, `move_node`, `delete_node`, `add_edge`, `delete_edge`, `seed_strategic_entity_ref(tx, project_id, strategic_goal_id, title)` — signatures per spec §5, all `Result<_, OrchdPersistError>`; enum⇄TEXT snake_case helpers; every §5 invariant enforced (archived guard incl. both edge endpoints, self-loop, dup edge, dup entityRef, unknown id).

- [ ] **Step 1: RED.** In-memory-DB tests: fresh DB is v2 with graph_node/graph_edge + indexes; a REAL v1 fixture (build a v1 DB in-test with a project+strategic goal) migrates → v2 + the strategic-goal entityRef node backfilled (assert exactly one, `kind='entityRef'`, `entity_type='goal'`); add_node/entity_ref (dup (type,id)⇒Conflict); update/move; delete_node cascades its edges; add_edge cross-project ok, self-loop⇒Invariant, dup(source,target,kind)⇒Conflict, unknown endpoint⇒NotFound; archived project blocks add_node AND add_edge (either endpoint archived); **entityRef soft-ref survival**: create an entityRef on a goal, delete the goal via `delete_goal`, assert the graph node still exists; create_project auto-seeds the strategic entityRef node (via list). FAIL.
- [ ] **Step 2: GREEN.** Implement graph.rs + the migration step + the create_project seed call.
- [ ] **Step 3:** `cargo test -p bpa-orchd graph` + `cargo test -p bpa-orchd persistence` (S3 tests unchanged) → PASS. Commit `feat(orchd): schema v2 + graph node/edge persistence + strategic-seed + v1→v2 backfill (S4 §4/§5)`.

### Task 3: retrieval (list/neighborhood/search) + perf

**Files:** Modify `crates/orchd/src/graph.rs`.
**Consumes:** T2. **Produces (spec §5):** `list_project_graph(project_id) -> GraphView` (nodes in project; incident edges; external ghost endpoints deduped; entityRef labels re-resolved from the live domain row, orphan keeps stored label); `neighborhood(node_id, depth) -> GraphNeighborhood` (bidirectional recursive CTE ≤6 hops, cross-project); `search_nodes(query, project_id: Option<&str>) -> Vec<GraphNode>` (LIKE label/body, workspace-wide when None, cap 200, updated_at DESC).

- [ ] **Step 1: RED.** Tests: list_project_graph returns own nodes + a cross-project edge's foreign endpoint in `external_nodes`, not in `nodes`; an entityRef whose goal was renamed shows the NEW label (read-time resolve); an entityRef whose goal was deleted is present with a stale label + is detectably orphan (the resolve helper returns None); neighborhood(depth 2) from a node returns exactly the 2-hop reachable set across a cross-project edge; depth 99 clamps to 6; search None spans projects, Some scopes; **perf:** build 500 nodes + 1000 edges in-memory, assert `neighborhood(root, 3)` completes < 100 ms (generous CI margin: assert < 100 ms measured, and separately that it returns the right node count). FAIL.
- [ ] **Step 2: GREEN + Step 3:** `cargo test -p bpa-orchd graph` → PASS. Commit `feat(orchd): graph retrieval — list-project/neighborhood/workspace-search + <100ms perf (S4 §5, D5)`.

### Task 4: socket dispatch + GraphChanged push

**Files:** Modify `crates/orchd/src/socket_server.rs`.
**Consumes:** T2/T3. **Produces (spec §6):** dispatch arms for all 9 graph verbs → the graph.rs method → the right response variant; mutating verbs broadcast `GraphChanged{project_id}` on success only; `GraphAddEdge`/`GraphDeleteEdge` broadcast `GraphChanged` for BOTH endpoints' projects when they differ; read verbs broadcast nothing; error mapping per §S3 §6.

- [ ] **Step 1: RED.** Socket tests (stub client over run(), orchd version consts, temp HOME): GraphAddNode → `Response::GraphNode` + a second connection receives `GraphChanged{project_id}`; a cross-project GraphAddEdge → TWO `GraphChanged` pushes (both project ids) observed; GraphListProject → GraphView, no push; add-edge self-loop → `Error{Invariant}` + no push; GraphNeighborhood over a built graph → correct subgraph. FAIL.
- [ ] **Step 2: GREEN + Step 3:** `cargo test -p bpa-orchd` (full crate) → PASS. Commit `feat(orchd): graph dispatch + GraphChanged push (both projects for cross-project edges) (S4 §6)`.

### Task 5: core commands + broker event

**Files:** Modify `src-tauri/src/commands.rs` (9 `orchd_graph_*` commands + inner fns), `src-tauri/src/broker.rs` (map `OrchdPush::GraphChanged` → `EV_ORCHD_GRAPH_CHANGED = "orchd://graph-changed"` `{projectId}`), `src-tauri/src/lib.rs` (register).
**Consumes:** T1 wire, T4 dispatch. **Produces:** `orchd_graph_add_node`, `orchd_graph_update_node`, `orchd_graph_move_node`, `orchd_graph_delete_node`, `orchd_graph_add_edge`, `orchd_graph_delete_edge`, `orchd_graph_list_project`, `orchd_graph_neighborhood`, `orchd_graph_search` — each builds the OrchdRequest, calls `state.orchd()?.request()`, matches the expected response variant, maps `OrchdResponse::Error` → `CommandError::Daemon`; `map_orchd_push` gains the GraphChanged arm.

- [ ] **Step 1: RED.** Stub-orchd tests: `orchd_graph_add_node` happy + an `Invariant` maps to `CommandError::Daemon{code:"Invariant"}`; broker unit: `OrchdPush::GraphChanged{project_id}` → `EV_ORCHD_GRAPH_CHANGED` + `{projectId}` camelCase. FAIL.
- [ ] **Step 2: GREEN + Step 3:** `cargo test -p builder-pro-ai` → PASS. Commit `feat(core): orchd_graph_* commands + orchd://graph-changed event (S4 §7)`.

### Task 6: frontend ipc + store slice + pure graphMapping

**Files:** Modify `src/ipc/orchd.ts` (9 wrappers), `src/ipc/events.ts` (`onOrchdGraphChanged`), `src/store/store.ts` + test (`graphByProject`, `refreshGraph`), `src/App.tsx` (bind the event); create `src/components/graph/graphMapping.ts` + `graphMapping.test.ts`.
**Consumes:** T1 generated `orchd-types.ts`, T5 commands. **Produces:** typed `orchdGraph*` wrappers (names/args match T5 verbatim); `onOrchdGraphChanged(cb:(p:{projectId:string})=>void)` for `orchd://graph-changed`; store `graphByProject: Record<string, GraphView>` + `refreshGraph(projectId)` (fetch via `orchdGraphListProject` + replace); App binds `orchd://graph-changed` → `refreshGraph(payload.projectId)` when that project is loaded/active. PURE `graphMapping.ts`: `toFlowNodes(view): FlowNode[]` (position from posX/posY, `type` by kind, `data:{label,kind,entityType?,entityId?,isExternal,isOrphan}`), `toFlowEdges(view): FlowEdge[]`, `flowPositionChangeToMove(change): {id,posX,posY}|null` (only for xyflow `type==='position'` changes with a `position`), `dedupeMovesById(moves)` (keep last per id — the debounce-flush contract). `FlowNode`/`FlowEdge` are local TS types (not xyflow imports in the pure file, to keep it renderless-testable — shape-compatible with xyflow's Node/Edge).

- [ ] **Step 1: RED.** graphMapping.test.ts: toFlowNodes maps posX/posY→position.{x,y} + kind→type + entityRef flags (isExternal for external_nodes, isOrphan when a helper marks it); toFlowEdges maps source/target/kind; flowPositionChangeToMove returns the move for a position change and null for a select/dimension change; dedupeMovesById keeps the last per id. store.test: refreshGraph replaces graphByProject[p]; graph-changed for project P re-fetches only P. orchd.ts test: each wrapper passes the exact command name + camelCase args. FAIL.
- [ ] **Step 2: GREEN + Step 3:** `npx vitest run` + `npx tsc --noEmit` → PASS. Commit `feat(ui): graph ipc + store slice + pure graphMapping helpers (S4 §7, D10)`.

### Task 7: GraphCanvas component + ProjectPanel tab

**Files:** Create `src/components/graph/GraphCanvas.tsx`; modify `src/components/ProjectPanel.tsx` (7th tab), `package.json` (`@xyflow/react`), `docs/design-system.md` (graph atoms). Run `npm i @xyflow/react` (pin v12.x; commit package.json + lockfile).
**Consumes:** T6 store slice + pure helpers. **Produces:** `<GraphCanvas projectId={string} />` — on mount `refreshGraph(projectId)`; `<ReactFlowProvider><ReactFlow nodes={toFlowNodes(view)} edges={toFlowEdges(view)} nodeTypes={…} onNodesChange={…} onEdgesChange={…} onConnect={…} fitView/></ReactFlowProvider>`; `onNodesChange` applies locally + collects position changes → per-id debounced (400 ms) `orchdGraphMoveNode`; `onConnect` → `orchdGraphAddEdge(source,target,'relates','')`; a toolbar (add-node kind `<select>` + button → `orchdGraphAddNode`; delete selected → `orchdGraphDeleteNode`/`orchdGraphDeleteEdge`; a search `<input>` → `orchdGraphSearch` highlighting matches); entityRef node click → navigate to the entity (switch tab or openProject); external ghost click → `openProject(externalProjectId)`; ALL mutating controls `disabled` while `orchdDown` (read from store); import `@xyflow/react/dist/style.css`. Errors → `showToast(describeOrchdError(e))`. ProjectPanel: append `{ key:"graph", label:"Граф" }` → renders `<GraphCanvas/>`.
**NOTE (D10):** `GraphCanvas.tsx` is NOT rendered in vitest (ReactFlow needs a real DOM). Do NOT add a test that renders it; its logic lives in T6's pure helpers. A ProjectPanel test asserts the «Граф» tab exists and (with children mocked) switches to a GraphCanvas marker — mock `./graph/GraphCanvas` to a stub in that test to avoid importing ReactFlow under node env.

- [ ] **Step 1: RED.** ProjectPanel test (mock `./graph/GraphCanvas`): the «Граф» tab renders + selecting it shows the GraphCanvas stub. (No GraphCanvas render test — D10.) FAIL/adjust.
- [ ] **Step 2: GREEN + Step 3:** `npx vitest run` + `npx tsc --noEmit` → PASS (tsc type-checks GraphCanvas.tsx even though it's not rendered — catches xyflow API misuse). Commit `feat(ui): graph canvas (@xyflow/react) + ProjectPanel «Граф» tab (S4 §7, D8)`.

### Task 8: e2e cross-project-survival phase + gate

**Files:** Modify `tests/e2e/orchd-survive.mjs` (new phase; add the graph frame encoders mirroring the existing orchd request encoders), run the full gate.
**Consumes:** everything. **Produces (spec §8 DoD):** a phase — create two projects (P1, P2), `GraphAddNode` in each, `GraphAddEdge` P1-node→P2-node (cross-project), `OrchdShutdown{drain}` → relaunch → `GraphListProject(P1)` returns the cross-project edge with P2's node in `externalNodes` (proving "a cross-project link survives BOTH projects' restarts"). Log `[e2e-orchd] phaseN OK: cross-project graph edge survived restart`.

- [ ] **Step 1:** Write the phase; `npm run e2e:orchd` → `ALL PHASES PASSED`.
- [ ] **Step 2:** `bash scripts/final-suite.sh` → `ALL GATES PASSED` (9 stages; orchd coverage ≥80% — add graph unit tests if short; the ts-rs parity already covers orchd-types.ts). If the only failure is the known attach.rs PTY flake, re-run once.
- [ ] **Step 3:** Commit `test(e2e): cross-project graph edge survives restart (S4 DoD) + gate green`.

### Task 9: docs truth + CHANGELOG [0.5.0]

**Files:** Modify `docs/architecture.md` (graph in orchd store + retrieval-API note), `docs/superpowers/specs/2026-07-01-builderpro-platform-overview.md` (§3 S4 row → SHIPPED `[0.5.0]` + «Current slice» → next S-EXT ∥ S5 → S-IDEA; note S4 unblocks S6), `README.md` (features + re-measured real test counts + graph mention), `CHANGELOG.md` (`[0.5.0]`: knowledge graph, workspace-wide retrieval API, xyflow canvas, cross-project edges), `docs/traceability.md` (S4 rows), `docs/design-system.md` (graph atoms present), `docs/backlog.md` (add: auto-populate graph from domain events beyond the strategic seed; graph bulk ops; embeddings/semantic retrieval — S8/S-EXT).

- [ ] **Step 1:** All edits; `git grep -niE 'todo|tbd' docs/architecture.md` → empty; README counts re-measured (`cargo test --workspace`/`npx vitest run`).
- [ ] **Step 2:** `bash scripts/final-suite.sh` → `ALL GATES PASSED`.
- [ ] **Step 3:** Commit `docs: S4 shipped — knowledge graph + retrieval API, CHANGELOG [0.5.0]`.

### Task 10: whole-branch adversarial review + merge

- [ ] **Step 1:** `scripts/review-package $(git merge-base main HEAD) HEAD`; multi-lens adversarial review (lenses: schema-v2 migration + backfill honesty vs a real v1 DB; entityRef soft-ref survival + orphan resolve; cross-project edge + push-both-projects + list-project ghost derivation; retrieval CTE correctness + the <100 ms claim; archived-guard on both edge endpoints; frontend pure-helper coverage + no ReactFlow-under-node-env + orchdDown-disable; e2e cross-project survival; gate/coverage). Verify → fix waves (follow-up commits) → re-gate.
- [ ] **Step 2:** `superpowers:finishing-a-development-branch` — verify gate, ff-merge to main, push origin.

---

## Self-review (done at write time)

Spec→task coverage: §3 wire→T1; §4 DDL + migration/backfill + seed→T2; §5 CRUD→T2, retrieval+perf→T3; §6 dispatch/push→T4; §7 core→T5, ipc/store/pure→T6, canvas/tab→T7; §8 e2e+gate→T8; §9 docs→T9; review/merge→T10. D10 testability honored (pure helpers in T6, GraphCanvas not vitest-rendered in T7). D5 workspace-wide + <100ms in T3. D6 seed + backfill in T2. D11 archived-guard/orchdDown across T2/T4/T7. No placeholders; enum kinds consistent across T1/T2 (concept/fact/artifact/decision/note/entityRef; relates/depends/derives/supports/contradicts/parent); command names `orchd_graph_*` ↔ wrappers `orchdGraph*` ↔ event `orchd://graph-changed` consistent T5/T6. Sequential (shared files: graph.rs T2/T3, socket_server T4, persistence.rs T2) — no parallel group.
