# S4 — Knowledge Graph (per-project + cross-project, viz, agent retrieval API)

**Date:** 2026-07-14
**Status:** Approved (owner delegated full decision authority; decisions D1–D12 locked below)
**Depends on:** S3 (`[0.4.0]` shipped — orchd daemon + app-domain store + ProjectPanel). Builds
entirely on the orchd store; NO sessiond change.
**Roadmap row:** overview §3 S4 — "Knowledge graph (per-project + cross-project links, viz)".
DoD: a cross-project link survives BOTH projects' restarts; the retrieval API returns a goal's
subgraph <100 ms; the graph is editable in the UI. **Hard-blocks S6 (owner decision D6).**

---

## 0. Goal

Add a knowledge graph to the orchd app-domain store: typed nodes (concepts/facts/artifacts/
decisions/notes + `entityRef` wrappers over domain entities) and typed edges, per-project AND
cross-project. Ship (a) the persistence + a **workspace-wide agent retrieval API** (read AND
write — the interface S6 agents will use to query and grow project knowledge), (b) the orchd wire
verbs + pushes, (c) the core client/commands, and (d) an editable `@xyflow/react` graph canvas as
a 7th ProjectPanel tab. After S4 the retrieval API exists and S6 is unblocked.

## 1. Owner decisions (locked)

| # | Decision |
|---|---|
| **D1** | Graph lives in **orchd.db, schema v2** (additive forward-only migration via `daemon_core::migrate::run_migrations`, appended as `Migration { upto: 2 }`; `SCHEMA_VERSION` 1→2). No wire-version bump needed beyond additive orchd-proto verbs (frozen append-only). |
| **D2** | **Node identity = uuid v4** (same as every orchd id). |
| **D3** | **`entityRef` nodes are SOFT-refs** (`entity_type` + `entity_id`, NO FK to domain tables): deleting a goal/idea/insight/task does NOT delete or corrupt its graph node — the node persists and the UI renders it as «источник удалён». Resolving an entityRef's live label happens at read time. Exactly one entityRef node per (entity_type, entity_id) (partial unique index). |
| **D4** | **Cross-project links = plain edges between nodes of different projects.** An edge references two `graph_node.id`; the endpoints may live in different projects. Edge FK `ON DELETE CASCADE` within orchd.db is legal (single store) — deleting a node removes its incident edges automatically. |
| **D5** | **Retrieval API is WORKSPACE-WIDE** (owner decision D6 / charter: "read AND write, workspace-wide — an agent working project A can query and read project B's knowledge"). `GraphNeighborhood`/`GraphSearch` traverse cross-project edges and are NOT project-scoped. `<100 ms` DoD met by a depth-bounded recursive CTE over indexed edge endpoints. |
| **D6** | **Strategic-goal entityRef node auto-seeded on `CreateProject`** (in the same tx that already creates the strategic goal + ruleset row) — a project's graph is never empty. |
| **D7** | **Coarse invalidation push `GraphChanged { project_id }`** (mirrors S3's coarse pushes; a cross-project edge change broadcasts `GraphChanged` for BOTH affected projects). |
| **D8** | **UI = a 7th ProjectPanel tab «Граф»** rendering `@xyflow/react` (v12, package `@xyflow/react`) as a controlled component; node positions persist (`GraphMoveNode`, debounced 400 ms). |
| **D9** | **xyflow is a new frontend dependency** — Context7-verified (v12 controlled API: `nodes`/`edges` props + `onNodesChange`/`onEdgesChange` via `applyNodeChanges`/`applyEdgeChanges`, `nodeTypes`, `onConnect`+`addEdge`, `ReactFlowProvider`, CSS `@xyflow/react/dist/style.css`). It is the ONLY new external dep. |
| **D10** | **Testability:** vitest runs `environment: "node"` with NO setupFiles — ReactFlow cannot be rendered/measured there. So ALL graph logic that needs testing lives in PURE functions (domain→xyflow node/edge mapping, cross-project ghost derivation, position-change→GraphMoveNode debounce, retrieval-result shaping) tested directly; the thin `<GraphCanvas/>` React shell that mounts `<ReactFlow/>` is NOT vitest-rendered (its wiring is exercised only via the pure helpers it delegates to). Rust retrieval/persistence gets full unit + socket tests; a new e2e phase proves cross-project survival. |
| **D11** | **Security/honesty reuse:** every mutating verb honors the archived-project guard (a node/edge whose project is archived ⇒ `Invariant`; a cross-project edge is blocked if EITHER endpoint's project is archived); `orchdDown` disables the graph's mutating controls (S3 §10 pattern); failed mutations broadcast no push; no content logged. |
| **D12** | **No agent runtime in S4.** The retrieval API is the CONTRACT (wire verbs + core commands) that S6 agents will call; S4 ships the API + the human-facing graph editor, NOT any agent that uses it. Auto-population of the graph from domain events beyond the D6 strategic seed is deferred (backlog). |

## 2. Architecture

All work is in the existing crates (no new crate): `crates/orchd` (schema v2 + graph persistence
+ retrieval + dispatch), `crates/orchd-proto` (append graph verbs/pushes + entities + ts-rs),
`src-tauri` (graph commands + broker events, in the existing orchd_client), `src/` (orchd.ts
wrappers + store graph slice + `GraphCanvas.tsx` + pure helpers + ProjectPanel tab). `sessiond`,
`daemon-core`, `paths`, `protocol` are UNTOUCHED.

## 3. Wire protocol (`bpa-orchd-proto`, appended — order FROZEN append-only)

New entities (`#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]`,
`#[serde(rename_all = "camelCase")]`, exported to `src/ipc/orchd-types.ts`):

```rust
pub struct GraphNode {
    pub id: String, pub project_id: String, pub kind: GraphNodeKind,
    pub entity_type: Option<GraphEntityType>, pub entity_id: Option<String>,
    pub label: String, pub body: String, pub pos_x: f64, pub pos_y: f64,
    pub created_at: i64, pub updated_at: i64,
}
pub enum GraphNodeKind { Concept, Fact, Artifact, Decision, Note, EntityRef }
pub enum GraphEntityType { Goal, Idea, Insight, Task, Ruleset }

pub struct GraphEdge {
    pub id: String, pub source_node_id: String, pub target_node_id: String,
    pub kind: GraphEdgeKind, pub label: String, pub created_at: i64,
}
pub enum GraphEdgeKind { Relates, Depends, Derives, Supports, Contradicts, Parent }

// Retrieval result: the project's own nodes + all incident edges + the foreign endpoints
// (cross-project "ghosts") so the UI can render boundary edges.
pub struct GraphView {
    pub nodes: Vec<GraphNode>,          // nodes with project_id == the queried project
    pub edges: Vec<GraphEdge>,          // every edge incident to any of `nodes`
    pub external_nodes: Vec<GraphNode>, // foreign endpoints of cross-project edges (ghosts)
}
// Neighborhood: subgraph within N hops of a start node, cross-project (the agent retrieval query).
pub struct GraphNeighborhood {
    pub root_id: String, pub nodes: Vec<GraphNode>, pub edges: Vec<GraphEdge>,
}
```

Appended to `OrchdRequest` (END of enum):
```rust
GraphAddNode { project_id: String, kind: GraphNodeKind, label: String, body: String,
               pos_x: f64, pos_y: f64 },                              // → GraphNode
GraphUpdateNode { id: String, label: Option<String>, body: Option<String> },  // → GraphNode
GraphMoveNode { id: String, pos_x: f64, pos_y: f64 },                // → GraphNode (frequent)
GraphDeleteNode { id: String },                                      // → Ack (cascades edges)
GraphAddEdge { source_node_id: String, target_node_id: String, kind: GraphEdgeKind,
               label: String },                                      // → GraphEdge
GraphDeleteEdge { id: String },                                      // → Ack
GraphListProject { project_id: String },                             // → GraphView
GraphNeighborhood { node_id: String, depth: u32 },                   // → Neighborhood (retrieval)
GraphSearch { query: String, project_id: Option<String> },           // → Nodes (workspace-wide when None)
```
Appended to `OrchdResponse` (END): `GraphNode(GraphNode)`, `GraphEdge(GraphEdge)`,
`GraphView(GraphView)`, `Neighborhood(GraphNeighborhood)`, `GraphNodes(Vec<GraphNode>)`.
Appended to `OrchdPush` (END): `GraphChanged { project_id: String }`.

## 4. `orchd.db` schema v2 (LOCKED DDL, `Migration { upto: 2 }`)

```sql
CREATE TABLE graph_node (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES project(id) ON DELETE CASCADE,
  kind TEXT NOT NULL CHECK (kind IN ('concept','fact','artifact','decision','note','entityRef')),
  entity_type TEXT CHECK (entity_type IN ('goal','idea','insight','task','ruleset')),
  entity_id TEXT,
  label TEXT NOT NULL, body TEXT NOT NULL DEFAULT '',
  pos_x REAL NOT NULL DEFAULT 0, pos_y REAL NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL,
  CHECK ((kind = 'entityRef') = (entity_type IS NOT NULL AND entity_id IS NOT NULL))
);
CREATE INDEX graph_node_by_project ON graph_node(project_id);
CREATE UNIQUE INDEX graph_node_one_per_entity
  ON graph_node(entity_type, entity_id) WHERE kind = 'entityRef';
CREATE TABLE graph_edge (
  id TEXT PRIMARY KEY,
  source_node_id TEXT NOT NULL REFERENCES graph_node(id) ON DELETE CASCADE,
  target_node_id TEXT NOT NULL REFERENCES graph_node(id) ON DELETE CASCADE,
  kind TEXT NOT NULL CHECK (kind IN ('relates','depends','derives','supports','contradicts','parent')),
  label TEXT NOT NULL DEFAULT '', created_at INTEGER NOT NULL,
  CHECK (source_node_id <> target_node_id)
);
CREATE INDEX graph_edge_by_source ON graph_edge(source_node_id);
CREATE INDEX graph_edge_by_target ON graph_edge(target_node_id);
CREATE UNIQUE INDEX graph_edge_uniq ON graph_edge(source_node_id, target_node_id, kind);
-- migration also runs, ONCE, an idempotent backfill: for every existing project's strategic goal
-- that has no entityRef node, INSERT one (so pre-S4 projects get a seeded graph on upgrade).
-- user_version → 2
```

`foreign_keys = ON` already set on both open paths (S3). Timestamps unix-ms; ids uuid v4.

## 5. Persistence + retrieval (`crates/orchd/src/graph.rs` — new module, `Db` methods)

All `Result<_, OrchdPersistError>`. Enum⇄TEXT via explicit snake_case helpers (matching the §4
CHECK literals; wire strings are camelCase — the persistence layer owns the mapping, exactly like
S3's task/idea enums).

- `add_node(project_id, kind, label, body, pos_x, pos_y) -> GraphNode` (archived project ⇒ `Invariant`; unknown project ⇒ `NotFound`).
- `add_entity_ref_node(project_id, entity_type, entity_id, label, pos_x, pos_y) -> GraphNode` (used by the D6 seed + future auto-population; duplicate (type,id) ⇒ `Conflict` via the partial unique index).
- `update_node(id, label?, body?) -> GraphNode`; `move_node(id, pos_x, pos_y) -> GraphNode`; `delete_node(id) -> ()` (FK cascades edges). All archived-guarded via the node's project.
- `add_edge(source, target, kind, label) -> GraphEdge` — reject self-loop (`Invariant`), duplicate (source,target,kind) ⇒ `Conflict`; unknown endpoint ⇒ `NotFound`; **archived guard: reject if EITHER endpoint's project is archived** (`Invariant`). Cross-project edges allowed.
- `delete_edge(id) -> ()`.
- `list_project_graph(project_id) -> GraphView` — nodes where `project_id = ?`; edges incident to any of them (source OR target in the set); `external_nodes` = the endpoint nodes NOT in the project (deduped). entityRef labels refreshed from the live domain row at read time (a helper joins goal/idea/insight/task/ruleset by entity_id; a missing source keeps the stored label + the UI flags «источник удалён»).
- `neighborhood(node_id, depth) -> GraphNeighborhood` — recursive CTE from `node_id` following edges in BOTH directions up to `depth` hops (cap depth at 6 to bound cost), returning the reachable nodes + connecting edges. Cross-project. This is the `<100 ms` retrieval query — indexed on `graph_edge(source_node_id)`/`(target_node_id)`; a test asserts a 500-node/1000-edge graph neighborhood(depth 3) returns in well under 100 ms.
- `search_nodes(query, project_id: Option<&str>) -> Vec<GraphNode>` — `label`/`body` `LIKE %query%` (case-insensitive); `project_id: None` ⇒ workspace-wide (all projects); `Some` ⇒ that project only. Ordered by updated_at DESC, capped at 200 rows.
- `seed_strategic_entity_ref(tx, project_id, strategic_goal_id, title)` — called inside `create_project`'s tx (D6). The v2 migration backfill reuses the same insert for existing projects.

**Invariants table:**
| Invariant | Error |
|---|---|
| add/update/move/delete node or edge on an archived project (either endpoint for edges) | `Invariant("project archived")` |
| self-loop edge | `Invariant` |
| duplicate (source,target,kind) edge | `Conflict` |
| second entityRef for one (entity_type, entity_id) | `Conflict` |
| unknown node/edge/project id | `NotFound` |
| `neighborhood` depth > 6 | clamped to 6 (not an error) |

## 6. Dispatch + push (`socket_server.rs`)

Each graph verb → its `graph.rs` method; reply the entity/`Ack`/`GraphView`/`Neighborhood`/
`GraphNodes`; on SUCCESS of a MUTATING verb broadcast `GraphChanged { project_id }` (for
`GraphAddEdge`/`GraphDeleteEdge` where the two endpoints span two projects, broadcast
`GraphChanged` for BOTH project_ids). Read verbs (`GraphListProject`/`GraphNeighborhood`/
`GraphSearch`) broadcast nothing. Failed verb → no push. Error mapping unchanged (§S3 §6).

## 7. Frontend

- **`src/ipc/orchd.ts`** (append): typed wrappers `orchdGraphAddNode`, `orchdGraphUpdateNode`, `orchdGraphMoveNode`, `orchdGraphDeleteNode`, `orchdGraphAddEdge`, `orchdGraphDeleteEdge`, `orchdGraphListProject`, `orchdGraphNeighborhood`, `orchdGraphSearch` (names/args verbatim vs the Tauri commands).
- **`src/ipc/events.ts`** (append): `onOrchdGraphChanged(cb: (p: {projectId: string}) => void)` for `orchd://graph-changed`.
- **store graph slice:** `graphByProject: Record<string, GraphView>`, `refreshGraph(projectId)` (fetch+replace); the App mount effect binds `orchd://graph-changed` → `refreshGraph(payload.projectId)` (only if that project's graph is loaded / is the active project).
- **`src/components/graph/graphMapping.ts`** (PURE, fully tested): `toFlowNodes(view: GraphView): FlowNode[]` (domain node → xyflow node incl. `position:{x:posX,y:posY}`, `type` by kind, `data` with label/kind/entity info/`isExternal` for ghosts/`isOrphan` when an entityRef's source is gone), `toFlowEdges(view): FlowEdge[]`, `flowPositionChangeToMove(change): {id,posX,posY} | null` (map an xyflow position NodeChange to a GraphMoveNode arg; ignore non-position changes), a `debounceMoves` helper contract. These are the D10 testable seam.
- **`src/components/graph/GraphCanvas.tsx`** (thin shell, NOT vitest-rendered): wraps `<ReactFlowProvider><ReactFlow …/></ReactFlowProvider>`; controlled via `toFlowNodes/toFlowEdges(graphByProject[projectId])`; `onNodesChange` → apply locally + debounced `orchdGraphMoveNode` per moved node; `onConnect` → `orchdGraphAddEdge`; a small toolbar (add node of a chosen kind, delete selected node/edge, a search box → `orchdGraphSearch` highlighting results); entityRef nodes click → navigate to the entity (switch ProjectPanel tab / openProject); external ghost node click → `openProject(itsProjectId)`; imports the xyflow CSS. All mutating controls `disabled` while `orchdDown` (D11), with the shared OrchdDownBanner shown by ProjectPanel (S3 already renders it on orchdDown).
- **`ProjectPanel.tsx`:** add a 7th tab `{ key: "graph", label: "Граф" }` → `<GraphCanvas projectId={projectId} />`.
- **`docs/design-system.md`:** append rows for the graph-node atom + graph-canvas toolbar.
- **deps:** `npm i @xyflow/react` (pin the resolved v12.x in package.json + lockfile).

## 8. Testing & DoD

- TDD. **Rust:** graph.rs unit tests (every §5 method + every §5 invariant, entityRef soft-ref survival across a domain-entity delete, cross-project edge create/list/neighborhood, the D6 seed, the v2 migration backfill from a real v1 fixture, the `<100 ms` neighborhood perf assertion on a synthetic 500-node graph); orchd-proto CBOR round-trip for every new variant + ts_export parity for the new entities; socket dispatch tests (mutate→response+`GraphChanged` push; cross-project edge → push for BOTH projects; read verbs → no push; archived guard). All Rust tests hermetic (temp HOME).
- **Frontend:** `graphMapping.test.ts` covers toFlowNodes/toFlowEdges (incl. ghost + orphan flags + position mapping), flowPositionChangeToMove (position vs non-position changes), the debounce contract; store `refreshGraph` + the graph-changed binding; orchd.ts wrapper name/arg parity. `GraphCanvas.tsx` is NOT rendered under vitest (D10) — its logic is covered via the pure helpers.
- **e2e (`tests/e2e/orchd-survive.mjs`, extend):** add a phase — create two projects, add a node in each, add a CROSS-PROJECT edge, `OrchdShutdown{drain}` → relaunch → `GraphListProject(A)` still shows the cross-project edge with B's node as an external ghost (the DoD "cross-project link survives BOTH projects' restarts"). Keep the existing phases green.
- **Gate:** `bash scripts/final-suite.sh` → `ALL GATES PASSED` (9 stages; stage 6 also diffs `src/ipc/orchd-types.ts` — already wired; orchd coverage stays ≥80%; e2e:orchd covers the new phase). No new gate stage.
- Migration UX: an existing `[0.4.0]` orchd.db (schema v1) upgrades to v2 on first boot, backfills strategic-goal entityRef nodes, sessiond DB untouched.

## 9. Docs & release (the close task)

`docs/architecture.md` (graph in the orchd store + retrieval API note), overview §3 S4 row →
SHIPPED `[0.5.0]` + «Current slice» advanced (next: S-EXT ∥ S5 → S-IDEA), README (features +
real re-measured counts), `CHANGELOG.md` `[0.5.0]`, `docs/traceability.md` S4 rows,
`docs/design-system.md` graph atoms, `docs/backlog.md` (add: auto-populate graph from domain
events beyond the strategic seed; graph node/edge bulk ops; retrieval ranking/embeddings — S8/S-EXT).

## 10. Out of scope (explicitly)

Agent runtime that USES the retrieval API (S6); auto-population beyond the D6 strategic seed
(backlog); embeddings/semantic retrieval (later — S4 retrieval is structural + LIKE-text);
graph layout algorithms server-side (xyflow client-side only; positions owner-set + persisted);
node/edge history/versioning; sessiond/terminal changes.
