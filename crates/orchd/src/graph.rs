//! Knowledge-graph node/edge persistence (S4 spec §4 schema v2, §5 persistence + invariants).
//! Sibling module to `persistence` (crate-private — `mod graph;` in `lib.rs`): builds directly on
//! `persistence::Db`'s `conn()` seam plus its `pub(crate)` helpers (`ensure_project_active`,
//! `now_ms`, `is_constraint_violation`) exactly like `export.rs` reuses `persistence`'s
//! `insert_*_raw` helpers. Enum⇄TEXT snake_case mapping mirrors S3's idea/insight/task helpers
//! (e.g. `IdeaLifecycle::InDev` ⇒ DB literal `"in_dev"`): the wire serde repr is camelCase, the
//! DB CHECK-constraint literal is snake_case, and this module owns that mapping —
//! `GraphNodeKind::EntityRef` ⇒ DB literal `'entity_ref'`.
//!
//! `socket_server.rs`'s dispatch match is fully wired to this module's mutators/readers (S4 T4) —
//! no blanket `#![allow(dead_code)]` here. [`Db::add_entity_ref_node`] is internal-only (not a
//! wire verb) but IS wired to a real caller: `persistence::Db::set_insight_status`'s accept path
//! (S-IDEA spec §6 D9, task T4) — see that method's own doc comment.
use std::collections::HashSet;

use bpa_orchd_proto::{
    GraphEdge, GraphEdgeKind, GraphEntityType, GraphNeighborhood, GraphNode, GraphNodeKind,
    GraphView,
};
use rusqlite::{Connection, OptionalExtension};
use uuid::Uuid;

use crate::persistence::{
    ensure_project_active, is_constraint_violation, now_ms, Db, OrchdPersistError,
};

// ---- graph_node/graph_edge enum <-> TEXT helpers (S4 spec §4 CHECK literals, snake_case — this
// module OWNS the mapping, deliberately distinct from `bpa_orchd_proto`'s camelCase wire serde
// reprs; e.g. `GraphNodeKind::EntityRef` is `"entityRef"` on the wire but `"entity_ref"` in the DB
// CHECK constraint). ----

fn encode_node_kind(k: &GraphNodeKind) -> &'static str {
    match k {
        GraphNodeKind::Concept => "concept",
        GraphNodeKind::Fact => "fact",
        GraphNodeKind::Artifact => "artifact",
        GraphNodeKind::Decision => "decision",
        GraphNodeKind::Note => "note",
        GraphNodeKind::EntityRef => "entity_ref",
    }
}

fn decode_node_kind(s: &str) -> Result<GraphNodeKind, OrchdPersistError> {
    match s {
        "concept" => Ok(GraphNodeKind::Concept),
        "fact" => Ok(GraphNodeKind::Fact),
        "artifact" => Ok(GraphNodeKind::Artifact),
        "decision" => Ok(GraphNodeKind::Decision),
        "note" => Ok(GraphNodeKind::Note),
        "entity_ref" => Ok(GraphNodeKind::EntityRef),
        other => Err(OrchdPersistError::Io(format!(
            "corrupt graph_node.kind value: {other}"
        ))),
    }
}

/// Used by [`Db::add_entity_ref_node`] (and, transitively, [`map_entity_ref_conflict`]) — wired
/// into `persistence::Db::set_insight_status`'s accept path (S-IDEA spec §6 D9, task T4).
fn encode_entity_type(t: &GraphEntityType) -> &'static str {
    match t {
        GraphEntityType::Goal => "goal",
        GraphEntityType::Idea => "idea",
        GraphEntityType::Insight => "insight",
        GraphEntityType::Task => "task",
    }
}

fn decode_entity_type(s: &str) -> Result<GraphEntityType, OrchdPersistError> {
    match s {
        "goal" => Ok(GraphEntityType::Goal),
        "idea" => Ok(GraphEntityType::Idea),
        "insight" => Ok(GraphEntityType::Insight),
        "task" => Ok(GraphEntityType::Task),
        other => Err(OrchdPersistError::Io(format!(
            "corrupt graph_node.entity_type value: {other}"
        ))),
    }
}

fn encode_edge_kind(k: &GraphEdgeKind) -> &'static str {
    match k {
        GraphEdgeKind::Relates => "relates",
        GraphEdgeKind::Depends => "depends",
        GraphEdgeKind::Derives => "derives",
        GraphEdgeKind::Supports => "supports",
        GraphEdgeKind::Contradicts => "contradicts",
        GraphEdgeKind::Parent => "parent",
    }
}

fn decode_edge_kind(s: &str) -> Result<GraphEdgeKind, OrchdPersistError> {
    match s {
        "relates" => Ok(GraphEdgeKind::Relates),
        "depends" => Ok(GraphEdgeKind::Depends),
        "derives" => Ok(GraphEdgeKind::Derives),
        "supports" => Ok(GraphEdgeKind::Supports),
        "contradicts" => Ok(GraphEdgeKind::Contradicts),
        "parent" => Ok(GraphEdgeKind::Parent),
        other => Err(OrchdPersistError::Io(format!(
            "corrupt graph_edge.kind value: {other}"
        ))),
    }
}

/// Raw `graph_node` row (text-encoded `kind`/`entity_type`) before decoding into the wire
/// [`GraphNode`] type. Mirrors `persistence::GoalRow`'s shape.
struct GraphNodeRow {
    id: String,
    project_id: String,
    kind: String,
    entity_type: Option<String>,
    entity_id: Option<String>,
    label: String,
    body: String,
    pos_x: f64,
    pos_y: f64,
    created_at: i64,
    updated_at: i64,
}

impl GraphNodeRow {
    fn from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<GraphNodeRow> {
        Ok(GraphNodeRow {
            id: r.get(0)?,
            project_id: r.get(1)?,
            kind: r.get(2)?,
            entity_type: r.get(3)?,
            entity_id: r.get(4)?,
            label: r.get(5)?,
            body: r.get(6)?,
            pos_x: r.get(7)?,
            pos_y: r.get(8)?,
            created_at: r.get(9)?,
            updated_at: r.get(10)?,
        })
    }

    fn into_node(self) -> Result<GraphNode, OrchdPersistError> {
        let entity_type = self
            .entity_type
            .as_deref()
            .map(decode_entity_type)
            .transpose()?;
        Ok(GraphNode {
            id: self.id,
            project_id: self.project_id,
            kind: decode_node_kind(&self.kind)?,
            entity_type,
            entity_id: self.entity_id,
            label: self.label,
            body: self.body,
            pos_x: self.pos_x,
            pos_y: self.pos_y,
            created_at: self.created_at,
            updated_at: self.updated_at,
            // A freshly-read row is not-yet-resolved; `resolve_node_label` (below) is the only
            // place that flips this to `true`, for an entityRef whose source row is gone.
            is_orphan: false,
        })
    }
}

fn load_node(conn: &Connection, id: &str) -> Result<GraphNode, OrchdPersistError> {
    conn.query_row(
        "SELECT id, project_id, kind, entity_type, entity_id, label, body, pos_x, pos_y,
                created_at, updated_at
         FROM graph_node WHERE id = ?1",
        rusqlite::params![id],
        GraphNodeRow::from_row,
    )
    .optional()?
    .ok_or(OrchdPersistError::NotFound)?
    .into_node()
}

/// A node's own `project_id` (used by every node mutator to look up the archived-project guard
/// target before mutating). Unknown id ⇒ `NotFound`.
fn node_project_id(conn: &Connection, node_id: &str) -> Result<String, OrchdPersistError> {
    conn.query_row(
        "SELECT project_id FROM graph_node WHERE id = ?1",
        rusqlite::params![node_id],
        |r| r.get(0),
    )
    .optional()?
    .ok_or(OrchdPersistError::NotFound)
}

/// Maps a `graph_node_one_per_entity` partial-unique-index hit to `Conflict` (S4 spec §5:
/// "duplicate (type,id) ⇒ Conflict"), otherwise passes the raw SQL error through unchanged.
///
/// Only called from [`Db::add_entity_ref_node`]. `persistence::Db::set_insight_status`'s accept
/// path (S-IDEA spec §6 D9, task T4) treats exactly this `Conflict` shape as a benign no-op — a
/// re-accept after archive finds the entityRef node already seeded and does not error.
fn map_entity_ref_conflict(
    e: rusqlite::Error,
    entity_type: &GraphEntityType,
    entity_id: &str,
) -> OrchdPersistError {
    if is_constraint_violation(&e) {
        OrchdPersistError::Conflict(format!(
            "an entityRef node already exists for {}:{entity_id}",
            encode_entity_type(entity_type)
        ))
    } else {
        OrchdPersistError::Sql(e)
    }
}

struct GraphEdgeRow {
    id: String,
    source_node_id: String,
    target_node_id: String,
    kind: String,
    label: String,
    created_at: i64,
}

impl GraphEdgeRow {
    fn from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<GraphEdgeRow> {
        Ok(GraphEdgeRow {
            id: r.get(0)?,
            source_node_id: r.get(1)?,
            target_node_id: r.get(2)?,
            kind: r.get(3)?,
            label: r.get(4)?,
            created_at: r.get(5)?,
        })
    }

    fn into_edge(self) -> Result<GraphEdge, OrchdPersistError> {
        Ok(GraphEdge {
            id: self.id,
            source_node_id: self.source_node_id,
            target_node_id: self.target_node_id,
            kind: decode_edge_kind(&self.kind)?,
            label: self.label,
            created_at: self.created_at,
        })
    }
}

fn load_edge(conn: &Connection, id: &str) -> Result<GraphEdge, OrchdPersistError> {
    conn.query_row(
        "SELECT id, source_node_id, target_node_id, kind, label, created_at
         FROM graph_edge WHERE id = ?1",
        rusqlite::params![id],
        GraphEdgeRow::from_row,
    )
    .optional()?
    .ok_or(OrchdPersistError::NotFound)?
    .into_edge()
}

/// Maps a `graph_edge_uniq` unique-index hit to `Conflict` (S4 spec §5: "duplicate
/// (source,target,kind) ⇒ Conflict"), otherwise passes the raw SQL error through unchanged.
fn map_edge_conflict(
    e: rusqlite::Error,
    source_node_id: &str,
    target_node_id: &str,
    kind: &GraphEdgeKind,
) -> OrchdPersistError {
    if is_constraint_violation(&e) {
        OrchdPersistError::Conflict(format!(
            "edge {source_node_id}->{target_node_id} ({}) already exists",
            encode_edge_kind(kind)
        ))
    } else {
        OrchdPersistError::Sql(e)
    }
}

/// The domain table that owns an `entityRef`'s live label (S4 spec §5 D3): all four have a
/// `title` column ("all four have `title` — NO ruleset case, `GraphEntityType` has no `Ruleset`
/// variant per §3's own note that `RuleSet` has no title/label field").
fn entity_table(t: &GraphEntityType) -> &'static str {
    match t {
        GraphEntityType::Goal => "goal",
        GraphEntityType::Idea => "idea",
        GraphEntityType::Insight => "insight",
        GraphEntityType::Task => "task",
    }
}

/// Looks up an `entityRef`'s live `title` from its source domain row (S4 spec §5: "resolving an
/// entityRef's live label happens at read time"). `Ok(None)` means the source row is gone
/// (deleted) — the caller keeps the node's STORED `label` in that case (D3 soft-ref: the node
/// persists, the UI flags «источник удалён»); this helper itself does not know or care which
/// node it's resolving for, it just answers "does entity_id still exist in this domain table,
/// and if so what's its title".
fn resolve_entity_label(
    conn: &Connection,
    entity_type: &GraphEntityType,
    entity_id: &str,
) -> Result<Option<String>, OrchdPersistError> {
    let table = entity_table(entity_type);
    let sql = format!("SELECT title FROM {table} WHERE id = ?1");
    Ok(conn
        .query_row(&sql, rusqlite::params![entity_id], |r| r.get(0))
        .optional()?)
}

/// Re-resolves a [`GraphNode`]'s `label` from its live domain row AT READ TIME when it's an
/// `entityRef` node (S4 spec §5 `list_project_graph`). Non-`entityRef` nodes pass through
/// unchanged (`is_orphan` stays `false`, its `into_node` default). An orphaned `entityRef`
/// (source row deleted) keeps its STORED `label` unchanged AND has `is_orphan` set `true` —
/// `resolve_entity_label` returning `None` is exactly that "orphan" signal (D3: the UI flags
/// «источник удалён» off this wire field).
fn resolve_node_label(
    conn: &Connection,
    mut node: GraphNode,
) -> Result<GraphNode, OrchdPersistError> {
    if node.kind != GraphNodeKind::EntityRef {
        return Ok(node);
    }
    let (Some(entity_type), Some(entity_id)) =
        (node.entity_type.as_ref(), node.entity_id.as_deref())
    else {
        return Ok(node);
    };
    match resolve_entity_label(conn, entity_type, entity_id)? {
        Some(live_label) => {
            node.label = live_label;
            node.is_orphan = false;
        }
        None => node.is_orphan = true,
    }
    Ok(node)
}

impl Db {
    /// `GraphAddNode` (S4 spec §5): rejects `kind == EntityRef` — entityRef nodes are created
    /// ONLY via [`Db::add_entity_ref_node`], never this generic wire verb (this prevents the
    /// DDL's atomic `(kind='entity_ref') = (entity_type/entity_id set)` CHECK from firing as a
    /// raw SQL error for a caller that forgot to also supply `entity_type`/`entity_id`). Archived
    /// project ⇒ `Invariant`; unknown project ⇒ `NotFound`.
    ///
    /// `pub` (bumped from `pub(crate)` closing BL-62, S4 §8): the wire dispatch in
    /// `socket_server.rs` has exposed this verb over the socket since T10 anyway, so the
    /// crate-private restriction was never a real security boundary; the bump lets
    /// `tests/no_secrets_in_logs_graph.rs` drive it directly, mirroring how
    /// `persistence::Db`'s ruleset/project methods are already `pub` for the identical
    /// no-secrets-in-logs test-support reason.
    pub fn add_node(
        &self,
        project_id: &str,
        kind: GraphNodeKind,
        label: &str,
        body: &str,
        pos_x: f64,
        pos_y: f64,
    ) -> Result<GraphNode, OrchdPersistError> {
        if matches!(kind, GraphNodeKind::EntityRef) {
            return Err(OrchdPersistError::Validation(
                "add_node: entityRef nodes can only be created via add_entity_ref_node".to_string(),
            ));
        }
        let tx = self.conn().unchecked_transaction()?;
        ensure_project_active(&tx, project_id)?;
        let id = Uuid::new_v4().to_string();
        let now = now_ms();
        tx.execute(
            "INSERT INTO graph_node
               (id, project_id, kind, entity_type, entity_id, label, body, pos_x, pos_y,
                created_at, updated_at)
             VALUES (?1, ?2, ?3, NULL, NULL, ?4, ?5, ?6, ?7, ?8, ?8)",
            rusqlite::params![
                id,
                project_id,
                encode_node_kind(&kind),
                label,
                body,
                pos_x,
                pos_y,
                now
            ],
        )?;
        let node = load_node(&tx, &id)?;
        tx.commit()?;
        Ok(node)
    }

    /// `add_entity_ref_node` (S4 spec §5, D3/D6): archived project ⇒ `Invariant`; duplicate
    /// `(entity_type, entity_id)` ⇒ `Conflict` (partial unique index `graph_node_one_per_entity`
    /// — D3: "exactly one entityRef node per (entity_type, entity_id)"). NOT exposed as a wire
    /// verb — internal-only.
    ///
    /// Wired into `persistence::Db::set_insight_status`'s accept path (S-IDEA spec §6 D9, task
    /// T4): accepting a research-formed insight seeds it as an `entity_ref` graph node, treating
    /// a `Conflict` (re-accept after archive — archiving never removes the node) as benign. In
    /// particular the D6 strategic-goal seed does NOT use this method: it calls the free fn
    /// [`seed_strategic_entity_ref`] instead, because that seed must run INSIDE a transaction the
    /// caller already owns (`create_project`'s own tx for new projects, and `migrate_v2`'s tx for
    /// the v1→v2 backfill, which needs the plain `fn(&Transaction) -> rusqlite::Result<()>` shape
    /// to plug into the migration runner) — whereas this method opens and commits its own
    /// transaction via `self.conn().unchecked_transaction()` and additionally enforces
    /// `ensure_project_active`, neither of which the seed's two call sites can accommodate (the
    /// project row `create_project` seeds against isn't committed yet, and the migration backfill
    /// must not skip archived projects). Do not "fix" this by routing the seed through this
    /// method — the INSERTs only look similar; the transaction-ownership contracts genuinely
    /// differ. For the SAME reason, `set_insight_status` cannot fold this call into its own open
    /// transaction (SQLite has no nested `BEGIN` on one connection) — it calls this method
    /// sequentially, AFTER its own status-update transaction commits.
    pub(crate) fn add_entity_ref_node(
        &self,
        project_id: &str,
        entity_type: GraphEntityType,
        entity_id: &str,
        label: &str,
        pos_x: f64,
        pos_y: f64,
    ) -> Result<GraphNode, OrchdPersistError> {
        let tx = self.conn().unchecked_transaction()?;
        ensure_project_active(&tx, project_id)?;
        let id = Uuid::new_v4().to_string();
        let now = now_ms();
        tx.execute(
            "INSERT INTO graph_node
               (id, project_id, kind, entity_type, entity_id, label, body, pos_x, pos_y,
                created_at, updated_at)
             VALUES (?1, ?2, 'entity_ref', ?3, ?4, ?5, '', ?6, ?7, ?8, ?8)",
            rusqlite::params![
                id,
                project_id,
                encode_entity_type(&entity_type),
                entity_id,
                label,
                pos_x,
                pos_y,
                now
            ],
        )
        .map_err(|e| map_entity_ref_conflict(e, &entity_type, entity_id))?;
        let node = load_node(&tx, &id)?;
        tx.commit()?;
        Ok(node)
    }

    /// `GraphUpdateNode` (S4 spec §5): `label`/`body` left untouched when `None`. Unknown id ⇒
    /// `NotFound`; archived project (via the node's own project) ⇒ `Invariant`.
    ///
    /// `pub` (bumped from `pub(crate)` closing BL-62, S4 §8) — see [`Db::add_node`]'s note.
    pub fn update_node(
        &self,
        id: &str,
        label: Option<&str>,
        body: Option<&str>,
    ) -> Result<GraphNode, OrchdPersistError> {
        let tx = self.conn().unchecked_transaction()?;
        let project_id = node_project_id(&tx, id)?;
        ensure_project_active(&tx, &project_id)?;
        if label.is_some() || body.is_some() {
            tx.execute(
                "UPDATE graph_node SET
                   label = COALESCE(?2, label),
                   body = COALESCE(?3, body),
                   updated_at = ?4
                 WHERE id = ?1",
                rusqlite::params![id, label, body, now_ms()],
            )?;
        }
        let node = load_node(&tx, id)?;
        tx.commit()?;
        Ok(node)
    }

    /// `GraphMoveNode` (S4 spec §5, frequent verb): unknown id ⇒ `NotFound`; archived project ⇒
    /// `Invariant`.
    pub(crate) fn move_node(
        &self,
        id: &str,
        pos_x: f64,
        pos_y: f64,
    ) -> Result<GraphNode, OrchdPersistError> {
        let tx = self.conn().unchecked_transaction()?;
        let project_id = node_project_id(&tx, id)?;
        ensure_project_active(&tx, &project_id)?;
        tx.execute(
            "UPDATE graph_node SET pos_x = ?2, pos_y = ?3, updated_at = ?4 WHERE id = ?1",
            rusqlite::params![id, pos_x, pos_y, now_ms()],
        )?;
        let node = load_node(&tx, id)?;
        tx.commit()?;
        Ok(node)
    }

    /// `GraphDeleteNode` (S4 spec §5): FK `ON DELETE CASCADE` removes incident edges
    /// automatically (D4). Unknown id ⇒ `NotFound`; archived project ⇒ `Invariant`.
    ///
    /// `pub` (bumped from `pub(crate)` closing BL-62, S4 §8) — see [`Db::add_node`]'s note.
    pub fn delete_node(&self, id: &str) -> Result<(), OrchdPersistError> {
        let tx = self.conn().unchecked_transaction()?;
        let project_id = node_project_id(&tx, id)?;
        ensure_project_active(&tx, &project_id)?;
        tx.execute(
            "DELETE FROM graph_node WHERE id = ?1",
            rusqlite::params![id],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// `GraphAddEdge` (S4 spec §5): self-loop ⇒ `Invariant`; duplicate `(source,target,kind)` ⇒
    /// `Conflict` (unique index `graph_edge_uniq`); unknown endpoint ⇒ `NotFound`; archived guard
    /// rejects if EITHER endpoint's project is archived (`Invariant`) — cross-project edges are
    /// otherwise allowed (D4).
    ///
    /// `pub` (bumped from `pub(crate)` closing BL-62, S4 §8) — see [`Db::add_node`]'s note.
    pub fn add_edge(
        &self,
        source_node_id: &str,
        target_node_id: &str,
        kind: GraphEdgeKind,
        label: &str,
    ) -> Result<GraphEdge, OrchdPersistError> {
        if source_node_id == target_node_id {
            return Err(OrchdPersistError::Invariant(
                "graph edge source and target must differ (no self-loops)".to_string(),
            ));
        }
        let tx = self.conn().unchecked_transaction()?;
        let source_project = node_project_id(&tx, source_node_id)?;
        let target_project = node_project_id(&tx, target_node_id)?;
        ensure_project_active(&tx, &source_project)?;
        ensure_project_active(&tx, &target_project)?;
        let id = Uuid::new_v4().to_string();
        let now = now_ms();
        tx.execute(
            "INSERT INTO graph_edge (id, source_node_id, target_node_id, kind, label, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                id,
                source_node_id,
                target_node_id,
                encode_edge_kind(&kind),
                label,
                now
            ],
        )
        .map_err(|e| map_edge_conflict(e, source_node_id, target_node_id, &kind))?;
        let edge = load_edge(&tx, &id)?;
        tx.commit()?;
        Ok(edge)
    }

    /// `GraphDeleteEdge` (S4 spec §5). Unknown id ⇒ `NotFound`; archived guard rejects if EITHER
    /// endpoint's project is archived (`Invariant`) — mirroring `add_edge` and matching the §5
    /// invariants table ("delete node OR edge on an archived project [either endpoint for edges] ⇒
    /// Invariant"). The endpoint-project lookup reuses the same JOIN as `edge_endpoint_projects`.
    pub(crate) fn delete_edge(&self, id: &str) -> Result<(), OrchdPersistError> {
        let tx = self.conn().unchecked_transaction()?;
        let endpoints: Option<(String, String)> = tx
            .query_row(
                "SELECT sn.project_id, tn.project_id
                 FROM graph_edge e
                 JOIN graph_node sn ON sn.id = e.source_node_id
                 JOIN graph_node tn ON tn.id = e.target_node_id
                 WHERE e.id = ?1",
                rusqlite::params![id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let (source_project, target_project) = endpoints.ok_or(OrchdPersistError::NotFound)?;
        ensure_project_active(&tx, &source_project)?;
        if target_project != source_project {
            ensure_project_active(&tx, &target_project)?;
        }
        tx.execute(
            "DELETE FROM graph_edge WHERE id = ?1",
            rusqlite::params![id],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Both endpoint projects of an edge (S4 spec §5): read BEFORE `delete_edge`, since the row
    /// is gone afterward. Used by dispatch to broadcast `GraphChanged` to both projects (spec §6,
    /// mirrors the S3 `goal_project_id`/`task_project_id` pre-lookup pattern for delete verbs).
    /// Unknown edge id ⇒ `NotFound`.
    pub(crate) fn edge_endpoint_projects(
        &self,
        edge_id: &str,
    ) -> Result<(String, String), OrchdPersistError> {
        self.conn()
            .query_row(
                "SELECT sn.project_id, tn.project_id
                 FROM graph_edge e
                 JOIN graph_node sn ON sn.id = e.source_node_id
                 JOIN graph_node tn ON tn.id = e.target_node_id
                 WHERE e.id = ?1",
                rusqlite::params![edge_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .ok_or(OrchdPersistError::NotFound)
    }

    /// The node's own project id PLUS every distinct foreign project reachable via an incident
    /// edge (S4 spec §5): read BEFORE delete/mutate, for the cross-project `GraphChanged`
    /// invalidation of node delete/update/move (spec §6 — a node appears as an `external_nodes`
    /// ghost in those foreign projects' views). Own project is always first; the order of the
    /// foreign projects afterward is unspecified (dedup only). Unknown node id ⇒ `NotFound`.
    pub(crate) fn node_project_ids_reachable(
        &self,
        node_id: &str,
    ) -> Result<Vec<String>, OrchdPersistError> {
        let conn = self.conn();
        let own = node_project_id(conn, node_id)?;
        let mut stmt = conn.prepare(
            "SELECT DISTINCT n.project_id
             FROM graph_edge e
             JOIN graph_node n
               ON n.id = CASE WHEN e.source_node_id = ?1 THEN e.target_node_id
                              ELSE e.source_node_id END
             WHERE e.source_node_id = ?1 OR e.target_node_id = ?1",
        )?;
        let foreign: Vec<String> = stmt
            .query_map(rusqlite::params![node_id], |r| r.get(0))?
            .collect::<Result<_, _>>()?;

        let mut ids = vec![own];
        for pid in foreign {
            if !ids.contains(&pid) {
                ids.push(pid);
            }
        }
        Ok(ids)
    }
}

// ---- retrieval (S4 spec §5 / task-3): `list_project_graph`, `neighborhood`, `search_nodes` ----

impl Db {
    /// `GraphListProject` (S4 spec §5): `nodes` = every `graph_node` row with
    /// `project_id = project_id`; `edges` = every `graph_edge` incident to any of those nodes
    /// (source OR target in the set); `external_nodes` = the incident edges' endpoint nodes NOT
    /// in the project (the cross-project "ghosts"), deduped. `entityRef` node labels (in both
    /// `nodes` and `external_nodes`) are re-resolved from their live domain row at read time
    /// (D3, [`resolve_node_label`]) — an orphan (source deleted) keeps its stored label AND has
    /// `is_orphan` set `true` on the wire, the signal the UI renders as «источник удалён». Unknown
    /// project ⇒ `NotFound` (mirrors [`Db::list_goals`]'s existence check).
    pub(crate) fn list_project_graph(
        &self,
        project_id: &str,
    ) -> Result<GraphView, OrchdPersistError> {
        let conn = self.conn();
        let exists: bool = conn
            .query_row(
                "SELECT 1 FROM project WHERE id = ?1",
                rusqlite::params![project_id],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !exists {
            return Err(OrchdPersistError::NotFound);
        }

        let mut node_stmt = conn.prepare(
            "SELECT id, project_id, kind, entity_type, entity_id, label, body, pos_x, pos_y,
                    created_at, updated_at
             FROM graph_node WHERE project_id = ?1",
        )?;
        let own_rows: Vec<GraphNodeRow> = node_stmt
            .query_map(rusqlite::params![project_id], GraphNodeRow::from_row)?
            .collect::<Result<_, _>>()?;
        let own_ids: HashSet<String> = own_rows.iter().map(|r| r.id.clone()).collect();

        let mut nodes = Vec::with_capacity(own_rows.len());
        for row in own_rows {
            nodes.push(resolve_node_label(conn, row.into_node()?)?);
        }

        let mut edge_stmt = conn.prepare(
            "SELECT id, source_node_id, target_node_id, kind, label, created_at
             FROM graph_edge
             WHERE source_node_id IN (SELECT id FROM graph_node WHERE project_id = ?1)
                OR target_node_id IN (SELECT id FROM graph_node WHERE project_id = ?1)",
        )?;
        let edge_rows: Vec<GraphEdgeRow> = edge_stmt
            .query_map(rusqlite::params![project_id], GraphEdgeRow::from_row)?
            .collect::<Result<_, _>>()?;

        let mut external_ids: Vec<String> = Vec::new();
        for row in &edge_rows {
            for endpoint in [&row.source_node_id, &row.target_node_id] {
                if !own_ids.contains(endpoint) && !external_ids.contains(endpoint) {
                    external_ids.push(endpoint.clone());
                }
            }
        }
        let edges: Vec<GraphEdge> = edge_rows
            .into_iter()
            .map(GraphEdgeRow::into_edge)
            .collect::<Result<_, _>>()?;

        let mut external_nodes = Vec::with_capacity(external_ids.len());
        for id in external_ids {
            external_nodes.push(resolve_node_label(conn, load_node(conn, &id)?)?);
        }

        Ok(GraphView {
            nodes,
            edges,
            external_nodes,
        })
    }

    /// `GraphNeighborhood` (S4 spec §5, the agent retrieval query, `<100 ms` DoD): a recursive
    /// CTE walk from `node_id` following `graph_edge` in BOTH directions up to `depth` hops,
    /// cross-project (no project filter — D5: the retrieval API is workspace-wide). `depth` is
    /// clamped to ≤6 (spec §5 invariants table: "not an error"). Indexed on
    /// `graph_edge(source_node_id)`/`(target_node_id)` (spec §4). The recursive walk itself runs
    /// EXACTLY ONCE (collecting only the reachable `node_id`s); node/edge details are then
    /// fetched via plain indexed `IN (...)` lookups against that fixed id set — re-running the
    /// full recursive CTE a second time (once per detail query) roughly doubled the measured cost
    /// in this module's perf-DoD test, so this shape is deliberate, not an equivalent rewrite.
    /// Unknown `node_id` ⇒ `NotFound`.
    pub(crate) fn neighborhood(
        &self,
        node_id: &str,
        depth: u32,
    ) -> Result<GraphNeighborhood, OrchdPersistError> {
        let conn = self.conn();
        // Confirm the root exists before spending a recursive-CTE pass on a dangling id.
        node_project_id(conn, node_id)?;
        let depth = depth.min(6);

        // `UNION` (not `UNION ALL`) — SQLite's recursive-CTE dedup drops any `(node_id, hop)` row
        // that already exists in the accumulated result, so a node can appear at most `depth + 1`
        // times (once per hop level) even across a cyclic/dense graph: growth is bounded by
        // `node_count * (depth + 1)`, not exponential in edge fan-out. Two separate recursive
        // terms (one per direction) rather than one term with `source = ? OR target = ?` — SQLite
        // can drive each term off its own single-column index
        // (`graph_edge_by_source`/`graph_edge_by_target`, spec §4) directly, where the `OR` form
        // forced a full-table scan per queue item in this module's perf-DoD test.
        let mut reach_stmt = conn.prepare(
            "WITH RECURSIVE reach(node_id, hop) AS (
                SELECT ?1, 0
                UNION
                SELECT e.target_node_id, r.hop + 1
                  FROM reach r JOIN graph_edge e ON e.source_node_id = r.node_id
                 WHERE r.hop < ?2
                UNION
                SELECT e.source_node_id, r.hop + 1
                  FROM reach r JOIN graph_edge e ON e.target_node_id = r.node_id
                 WHERE r.hop < ?2
             )
             SELECT DISTINCT node_id FROM reach",
        )?;
        let reach_ids: Vec<String> = reach_stmt
            .query_map(rusqlite::params![node_id, depth], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        // Unreachable in practice — `node_id` itself is always in `reach` at hop 0, and its
        // existence was already confirmed above — but keeps this function total rather than
        // panicking on an empty `IN ()` clause if that ever changed.
        if reach_ids.is_empty() {
            return Ok(GraphNeighborhood {
                root_id: node_id.to_string(),
                nodes: Vec::new(),
                edges: Vec::new(),
            });
        }

        let placeholders = reach_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let id_params: Vec<&dyn rusqlite::ToSql> = reach_ids
            .iter()
            .map(|s| s as &dyn rusqlite::ToSql)
            .collect();

        let mut node_stmt = conn.prepare(&format!(
            "SELECT id, project_id, kind, entity_type, entity_id, label, body, pos_x, pos_y,
                    created_at, updated_at
             FROM graph_node WHERE id IN ({placeholders})"
        ))?;
        let nodes: Vec<GraphNode> = node_stmt
            .query_map(id_params.as_slice(), GraphNodeRow::from_row)?
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(GraphNodeRow::into_node)
            .collect::<Result<_, _>>()?;

        let mut edge_params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(reach_ids.len() * 2);
        edge_params.extend(id_params.iter().copied());
        edge_params.extend(id_params.iter().copied());
        let mut edge_stmt = conn.prepare(&format!(
            "SELECT id, source_node_id, target_node_id, kind, label, created_at
             FROM graph_edge
             WHERE source_node_id IN ({placeholders}) AND target_node_id IN ({placeholders})"
        ))?;
        let edges: Vec<GraphEdge> = edge_stmt
            .query_map(edge_params.as_slice(), GraphEdgeRow::from_row)?
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(GraphEdgeRow::into_edge)
            .collect::<Result<_, _>>()?;

        Ok(GraphNeighborhood {
            root_id: node_id.to_string(),
            nodes,
            edges,
        })
    }

    /// `GraphSearch` (S4 spec §5): `label` OR `body` `LIKE '%query%'` — SQLite's `LIKE` is
    /// case-insensitive for ASCII by default (no `COLLATE NOCASE` needed, and the DB's default
    /// collation is already `BINARY`/case-sensitive equality elsewhere, so this stays scoped to
    /// `LIKE`'s own built-in ASCII case-folding). `project_id: None` ⇒ workspace-wide (every
    /// project); `Some(pid)` ⇒ that project only. `ORDER BY updated_at DESC, id` (the `id`
    /// tiebreak makes ties deterministic, mirroring [`Db::list_ideas`]'s
    /// `ORDER BY created_at DESC, id`), capped at 200 rows. `entityRef` label resolution is NOT
    /// applied here (spec §5: "the stored label is fine for search").
    pub(crate) fn search_nodes(
        &self,
        query: &str,
        project_id: Option<&str>,
    ) -> Result<Vec<GraphNode>, OrchdPersistError> {
        let conn = self.conn();
        let pattern = format!("%{query}%");
        let rows: Vec<GraphNodeRow> = match project_id {
            Some(pid) => {
                let mut stmt = conn.prepare(
                    "SELECT id, project_id, kind, entity_type, entity_id, label, body, pos_x,
                            pos_y, created_at, updated_at
                     FROM graph_node
                     WHERE (label LIKE ?1 OR body LIKE ?1) AND project_id = ?2
                     ORDER BY updated_at DESC, id
                     LIMIT 200",
                )?;
                let rows = stmt
                    .query_map(rusqlite::params![pattern, pid], GraphNodeRow::from_row)?
                    .collect::<Result<_, _>>()?;
                rows
            }
            None => {
                let mut stmt = conn.prepare(
                    "SELECT id, project_id, kind, entity_type, entity_id, label, body, pos_x,
                            pos_y, created_at, updated_at
                     FROM graph_node
                     WHERE (label LIKE ?1 OR body LIKE ?1)
                     ORDER BY updated_at DESC, id
                     LIMIT 200",
                )?;
                let rows = stmt
                    .query_map(rusqlite::params![pattern], GraphNodeRow::from_row)?
                    .collect::<Result<_, _>>()?;
                rows
            }
        };
        rows.into_iter()
            .map(GraphNodeRow::into_node)
            .collect::<Result<_, _>>()
    }
}

/// D6 strategic-goal `entityRef` seed (S4 spec §5): called inside `create_project`'s tx for NEW
/// projects, AND reused by [`crate::persistence::migrate_v2`]'s v1→v2 backfill for pre-existing
/// projects (the migration only calls this for a goal that doesn't already have an entityRef
/// node, so re-running it is never attempted against a row that would collide with the
/// `graph_node_one_per_entity` partial unique index). Returns a plain `rusqlite::Result` (rather
/// than `OrchdPersistError`) so it can be called directly from a `Migration::apply` step
/// (`fn(&Transaction) -> rusqlite::Result<()>`) as well as via `?` from `create_project`
/// (`OrchdPersistError: From<rusqlite::Error>`).
pub(crate) fn seed_strategic_entity_ref(
    tx: &rusqlite::Transaction,
    project_id: &str,
    strategic_goal_id: &str,
    title: &str,
) -> rusqlite::Result<()> {
    let now = now_ms();
    let node_id = Uuid::new_v4().to_string();
    tx.execute(
        "INSERT INTO graph_node
           (id, project_id, kind, entity_type, entity_id, label, body, pos_x, pos_y,
            created_at, updated_at)
         VALUES (?1, ?2, 'entity_ref', 'goal', ?3, ?4, '', 0, 0, ?5, ?5)",
        rusqlite::params![node_id, project_id, strategic_goal_id, title, now],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpa_orchd_proto::GoalKind;

    fn new_db() -> Db {
        Db::open_in_memory().unwrap()
    }

    fn new_project(db: &Db) -> String {
        // `project_workspace.workspace_id` is UNIQUE across the whole table (one project per
        // workspace, S3 spec §5.2) — a fresh uuid per call so multi-project tests don't collide.
        let workspace_id = Uuid::new_v4().to_string();
        db.create_project("P", "", &[workspace_id]).unwrap().id
    }

    fn strategic_goal_id(db: &Db, project_id: &str) -> String {
        db.list_goals(project_id)
            .unwrap()
            .into_iter()
            .find(|g| g.kind == GoalKind::Strategic)
            .expect("strategic goal must exist")
            .id
    }

    fn add_concept(db: &Db, project_id: &str, label: &str) -> GraphNode {
        db.add_node(project_id, GraphNodeKind::Concept, label, "", 0.0, 0.0)
            .unwrap()
    }

    fn table_exists(conn: &Connection, name: &str) -> bool {
        conn.query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [name],
            |_| Ok(()),
        )
        .is_ok()
    }

    fn index_exists(conn: &Connection, name: &str) -> bool {
        conn.query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'index' AND name = ?1",
            [name],
            |_| Ok(()),
        )
        .is_ok()
    }

    // ---- schema v2 graph tables, still present in the now-current schema v4 (fresh DB) ----

    #[test]
    fn fresh_db_is_schema_v4_with_graph_tables_and_all_five_indexes() {
        let db = new_db();
        let version: i64 = db
            .conn()
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        // S-EXT spec §4 bumped SCHEMA_VERSION 2->3, and S-IDEA spec §4 bumped it 3->4
        // (both additive); the S4 graph tables this test checks for are unaffected — still
        // created by `migrate_v2`, which `migrate_v3`/`migrate_v4` build on top of, never replace.
        assert_eq!(version, 4);
        assert!(table_exists(db.conn(), "graph_node"));
        assert!(table_exists(db.conn(), "graph_edge"));
        for idx in [
            "graph_node_by_project",
            "graph_node_one_per_entity",
            "graph_edge_by_source",
            "graph_edge_by_target",
            "graph_edge_uniq",
        ] {
            assert!(index_exists(db.conn(), idx), "missing index {idx}");
        }
    }

    // ---- v1 -> v2 migration backfill (REAL v1 fixture, per the task-2 brief) ----

    #[test]
    fn v1_fixture_migrates_to_v2_and_backfills_strategic_entity_ref() {
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        let v1_steps: &[bpa_daemon_core::migrate::Migration] =
            &[bpa_daemon_core::migrate::Migration {
                upto: 1,
                apply: crate::persistence::migrate_v1,
            }];
        bpa_daemon_core::migrate::run_migrations(&conn, 0, 1, v1_steps).unwrap();
        assert!(
            !table_exists(&conn, "graph_node"),
            "the v1 fixture must NOT have graph tables yet"
        );

        let now = 1_700_000_000_000i64;
        let project_id = Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO project (id, name, description, status, created_at, updated_at)
             VALUES (?1, 'Acme', '', 'active', ?2, ?2)",
            rusqlite::params![project_id, now],
        )
        .unwrap();
        let goal_id = Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO goal (id, project_id, parent_id, kind, title, body, ord, status,
                                metric_refs, created_at, updated_at)
             VALUES (?1, ?2, NULL, 'strategic', ?3, '', 0, 'active', '[]', ?4, ?4)",
            rusqlite::params![
                goal_id,
                project_id,
                crate::persistence::STRATEGIC_GOAL_TITLE,
                now
            ],
        )
        .unwrap();

        let v2_steps: &[bpa_daemon_core::migrate::Migration] =
            &[bpa_daemon_core::migrate::Migration {
                upto: 2,
                apply: crate::persistence::migrate_v2,
            }];
        bpa_daemon_core::migrate::run_migrations(&conn, 1, 2, v2_steps).unwrap();

        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 2);
        assert!(table_exists(&conn, "graph_node"));
        assert!(table_exists(&conn, "graph_edge"));

        let mut stmt = conn
            .prepare("SELECT kind, entity_type, entity_id, project_id FROM graph_node")
            .unwrap();
        let rows: Vec<(String, Option<String>, Option<String>, String)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            rows.len(),
            1,
            "exactly one entity_ref node must be backfilled"
        );
        let (kind, entity_type, entity_id, node_project_id) = &rows[0];
        // Raw DB literal check (task-2 brief): the CHECK-constraint value is snake_case
        // `entity_ref`, never the wire camelCase `entityRef`.
        assert_eq!(kind, "entity_ref");
        assert_eq!(entity_type.as_deref(), Some("goal"));
        assert_eq!(entity_id.as_deref(), Some(goal_id.as_str()));
        assert_eq!(node_project_id, &project_id);
    }

    #[test]
    fn v1_fixture_with_multiple_projects_backfills_each_independently() {
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        let v1_steps: &[bpa_daemon_core::migrate::Migration] =
            &[bpa_daemon_core::migrate::Migration {
                upto: 1,
                apply: crate::persistence::migrate_v1,
            }];
        bpa_daemon_core::migrate::run_migrations(&conn, 0, 1, v1_steps).unwrap();

        let now = 1_700_000_000_000i64;
        let mut goal_ids = Vec::new();
        for name in ["Acme", "Globex"] {
            let project_id = Uuid::new_v4().to_string();
            conn.execute(
                "INSERT INTO project (id, name, description, status, created_at, updated_at)
                 VALUES (?1, ?2, '', 'active', ?3, ?3)",
                rusqlite::params![project_id, name, now],
            )
            .unwrap();
            let goal_id = Uuid::new_v4().to_string();
            conn.execute(
                "INSERT INTO goal (id, project_id, parent_id, kind, title, body, ord, status,
                                    metric_refs, created_at, updated_at)
                 VALUES (?1, ?2, NULL, 'strategic', ?3, '', 0, 'active', '[]', ?4, ?4)",
                rusqlite::params![
                    goal_id,
                    project_id,
                    crate::persistence::STRATEGIC_GOAL_TITLE,
                    now
                ],
            )
            .unwrap();
            goal_ids.push(goal_id);
        }

        let v2_steps: &[bpa_daemon_core::migrate::Migration] =
            &[bpa_daemon_core::migrate::Migration {
                upto: 2,
                apply: crate::persistence::migrate_v2,
            }];
        bpa_daemon_core::migrate::run_migrations(&conn, 1, 2, v2_steps).unwrap();

        let mut stmt = conn
            .prepare(
                "SELECT entity_id FROM graph_node WHERE kind = 'entity_ref' AND entity_type = 'goal'
                 ORDER BY entity_id",
            )
            .unwrap();
        let mut backfilled: Vec<String> = stmt
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        backfilled.sort();
        let mut expected = goal_ids.clone();
        expected.sort();
        assert_eq!(backfilled, expected);
    }

    // ---- create_project auto-seed (D6) ----

    /// Row shape for [`create_project_auto_seeds_strategic_entity_ref_node`] — named instead of
    /// an inline 5-tuple to avoid `clippy::type_complexity`.
    struct SeededNodeRow {
        kind: String,
        entity_type: Option<String>,
        entity_id: Option<String>,
        project_id: String,
        label: String,
    }

    #[test]
    fn create_project_auto_seeds_strategic_entity_ref_node() {
        let db = new_db();
        let project_id = new_project(&db);
        let goal_id = strategic_goal_id(&db, &project_id);

        let mut stmt = db
            .conn()
            .prepare(
                "SELECT kind, entity_type, entity_id, project_id, label
                 FROM graph_node WHERE project_id = ?1",
            )
            .unwrap();
        let rows: Vec<SeededNodeRow> = stmt
            .query_map(rusqlite::params![project_id], |r| {
                Ok(SeededNodeRow {
                    kind: r.get(0)?,
                    entity_type: r.get(1)?,
                    entity_id: r.get(2)?,
                    project_id: r.get(3)?,
                    label: r.get(4)?,
                })
            })
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();

        assert_eq!(
            rows.len(),
            1,
            "a new project's graph must be seeded with exactly one node"
        );
        let row = &rows[0];
        assert_eq!(row.kind, "entity_ref");
        assert_eq!(row.entity_type.as_deref(), Some("goal"));
        assert_eq!(row.entity_id.as_deref(), Some(goal_id.as_str()));
        assert_eq!(row.project_id, project_id);
        assert_eq!(row.label, crate::persistence::STRATEGIC_GOAL_TITLE);
    }

    // ---- add_node ----

    #[test]
    fn add_node_happy_path_creates_concept_node() {
        let db = new_db();
        let project_id = new_project(&db);
        let node = db
            .add_node(
                &project_id,
                GraphNodeKind::Concept,
                "A concept",
                "body text",
                1.5,
                2.5,
            )
            .unwrap();
        assert!(uuid::Uuid::parse_str(&node.id).is_ok(), "id must be a uuid");
        assert_eq!(node.project_id, project_id);
        assert_eq!(node.kind, GraphNodeKind::Concept);
        assert_eq!(node.entity_type, None);
        assert_eq!(node.entity_id, None);
        assert_eq!(node.label, "A concept");
        assert_eq!(node.body, "body text");
        assert_eq!(node.pos_x, 1.5);
        assert_eq!(node.pos_y, 2.5);
        assert!(node.created_at > 0);
        assert_eq!(node.created_at, node.updated_at);

        // Raw DB kind literal is snake_case, not the wire camelCase repr.
        let raw_kind: String = db
            .conn()
            .query_row(
                "SELECT kind FROM graph_node WHERE id = ?1",
                rusqlite::params![node.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(raw_kind, "concept");
    }

    #[test]
    fn add_node_rejects_entity_ref_kind_with_validation() {
        let db = new_db();
        let project_id = new_project(&db);
        let err = db
            .add_node(&project_id, GraphNodeKind::EntityRef, "x", "", 0.0, 0.0)
            .unwrap_err();
        assert!(matches!(err, OrchdPersistError::Validation(_)));
    }

    #[test]
    fn add_node_unknown_project_is_not_found() {
        let db = new_db();
        let err = db
            .add_node("no-such-project", GraphNodeKind::Concept, "x", "", 0.0, 0.0)
            .unwrap_err();
        assert!(matches!(err, OrchdPersistError::NotFound));
    }

    #[test]
    fn add_node_on_archived_project_is_invariant() {
        let db = new_db();
        let project_id = new_project(&db);
        db.archive_project(&project_id).unwrap();
        let err = db
            .add_node(&project_id, GraphNodeKind::Concept, "x", "", 0.0, 0.0)
            .unwrap_err();
        assert!(matches!(err, OrchdPersistError::Invariant(_)));
    }

    // ---- add_entity_ref_node ----

    #[test]
    fn add_entity_ref_node_happy_path() {
        let db = new_db();
        let project_id = new_project(&db);
        let idea = db
            .create_idea(Some(project_id.as_str()), "An idea", "")
            .unwrap();
        let node = db
            .add_entity_ref_node(
                &project_id,
                GraphEntityType::Idea,
                &idea.id,
                "An idea",
                3.0,
                4.0,
            )
            .unwrap();
        assert_eq!(node.kind, GraphNodeKind::EntityRef);
        assert_eq!(node.entity_type, Some(GraphEntityType::Idea));
        assert_eq!(node.entity_id, Some(idea.id.clone()));
        assert_eq!(node.label, "An idea");
        assert_eq!(node.body, "");

        let raw_kind: String = db
            .conn()
            .query_row(
                "SELECT kind FROM graph_node WHERE id = ?1",
                rusqlite::params![node.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(raw_kind, "entity_ref");
    }

    #[test]
    fn add_entity_ref_node_duplicate_type_and_id_is_conflict() {
        let db = new_db();
        let project_id = new_project(&db);
        let idea = db
            .create_idea(Some(project_id.as_str()), "An idea", "")
            .unwrap();
        db.add_entity_ref_node(
            &project_id,
            GraphEntityType::Idea,
            &idea.id,
            "An idea",
            0.0,
            0.0,
        )
        .unwrap();
        let err = db
            .add_entity_ref_node(
                &project_id,
                GraphEntityType::Idea,
                &idea.id,
                "An idea again",
                0.0,
                0.0,
            )
            .unwrap_err();
        assert!(matches!(err, OrchdPersistError::Conflict(_)));
    }

    #[test]
    fn add_entity_ref_node_on_archived_project_is_invariant() {
        let db = new_db();
        let project_id = new_project(&db);
        let idea = db
            .create_idea(Some(project_id.as_str()), "An idea", "")
            .unwrap();
        db.archive_project(&project_id).unwrap();
        let err = db
            .add_entity_ref_node(
                &project_id,
                GraphEntityType::Idea,
                &idea.id,
                "An idea",
                0.0,
                0.0,
            )
            .unwrap_err();
        assert!(matches!(err, OrchdPersistError::Invariant(_)));
    }

    // ---- update_node / move_node ----

    #[test]
    fn update_node_updates_label_and_body_independently() {
        let db = new_db();
        let project_id = new_project(&db);
        let node = add_concept(&db, &project_id, "orig");

        let updated = db.update_node(&node.id, Some("new label"), None).unwrap();
        assert_eq!(updated.label, "new label");
        assert_eq!(updated.body, node.body);

        let updated2 = db.update_node(&node.id, None, Some("new body")).unwrap();
        assert_eq!(updated2.label, "new label");
        assert_eq!(updated2.body, "new body");
    }

    #[test]
    fn update_node_unknown_id_is_not_found() {
        let db = new_db();
        let err = db.update_node("no-such-node", Some("x"), None).unwrap_err();
        assert!(matches!(err, OrchdPersistError::NotFound));
    }

    #[test]
    fn update_node_on_archived_project_is_invariant() {
        let db = new_db();
        let project_id = new_project(&db);
        let node = add_concept(&db, &project_id, "orig");
        db.archive_project(&project_id).unwrap();
        let err = db
            .update_node(&node.id, Some("new label"), Some("new body"))
            .unwrap_err();
        assert!(matches!(err, OrchdPersistError::Invariant(_)));

        // The row must be untouched — the guard rejects BEFORE the UPDATE runs.
        let (label, body): (String, String) = db
            .conn()
            .query_row(
                "SELECT label, body FROM graph_node WHERE id = ?1",
                rusqlite::params![node.id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(label, "orig", "update must not run on an archived project");
        assert_eq!(body, node.body);
    }

    #[test]
    fn move_node_updates_position() {
        let db = new_db();
        let project_id = new_project(&db);
        let node = add_concept(&db, &project_id, "orig");
        let moved = db.move_node(&node.id, 10.0, 20.0).unwrap();
        assert_eq!(moved.pos_x, 10.0);
        assert_eq!(moved.pos_y, 20.0);
        assert_eq!(moved.label, node.label);
    }

    #[test]
    fn move_node_on_archived_project_is_invariant() {
        let db = new_db();
        let project_id = new_project(&db);
        let node = add_concept(&db, &project_id, "orig");
        db.archive_project(&project_id).unwrap();
        let err = db.move_node(&node.id, 1.0, 1.0).unwrap_err();
        assert!(matches!(err, OrchdPersistError::Invariant(_)));
    }

    // ---- delete_node ----

    #[test]
    fn delete_node_cascades_incident_edges() {
        let db = new_db();
        let project_id = new_project(&db);
        let a = add_concept(&db, &project_id, "a");
        let b = add_concept(&db, &project_id, "b");
        let edge = db
            .add_edge(&a.id, &b.id, GraphEdgeKind::Relates, "")
            .unwrap();

        db.delete_node(&a.id).unwrap();

        let node_gone: Option<i64> = db
            .conn()
            .query_row(
                "SELECT 1 FROM graph_node WHERE id = ?1",
                rusqlite::params![a.id],
                |r| r.get(0),
            )
            .optional()
            .unwrap();
        assert!(node_gone.is_none());

        let edge_gone: Option<i64> = db
            .conn()
            .query_row(
                "SELECT 1 FROM graph_edge WHERE id = ?1",
                rusqlite::params![edge.id],
                |r| r.get(0),
            )
            .optional()
            .unwrap();
        assert!(
            edge_gone.is_none(),
            "deleting a node must cascade its incident edges"
        );
    }

    #[test]
    fn delete_node_unknown_id_is_not_found() {
        let db = new_db();
        let err = db.delete_node("no-such-node").unwrap_err();
        assert!(matches!(err, OrchdPersistError::NotFound));
    }

    #[test]
    fn delete_node_on_archived_project_is_invariant() {
        let db = new_db();
        let project_id = new_project(&db);
        let node = add_concept(&db, &project_id, "orig");
        db.archive_project(&project_id).unwrap();
        let err = db.delete_node(&node.id).unwrap_err();
        assert!(matches!(err, OrchdPersistError::Invariant(_)));

        // The row must still be present — the guard rejects BEFORE the DELETE runs.
        let still_present: Option<i64> = db
            .conn()
            .query_row(
                "SELECT 1 FROM graph_node WHERE id = ?1",
                rusqlite::params![node.id],
                |r| r.get(0),
            )
            .optional()
            .unwrap();
        assert!(
            still_present.is_some(),
            "delete must not run on an archived project"
        );
    }

    // ---- add_edge ----

    #[test]
    fn add_edge_cross_project_ok() {
        let db = new_db();
        let project_a = new_project(&db);
        let project_b = new_project(&db);
        let a = add_concept(&db, &project_a, "a");
        let b = add_concept(&db, &project_b, "b");
        let edge = db
            .add_edge(&a.id, &b.id, GraphEdgeKind::Relates, "cross")
            .unwrap();
        assert_eq!(edge.source_node_id, a.id);
        assert_eq!(edge.target_node_id, b.id);
        assert_eq!(edge.kind, GraphEdgeKind::Relates);
        assert_eq!(edge.label, "cross");
        assert!(uuid::Uuid::parse_str(&edge.id).is_ok());

        let raw_kind: String = db
            .conn()
            .query_row(
                "SELECT kind FROM graph_edge WHERE id = ?1",
                rusqlite::params![edge.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(raw_kind, "relates");
    }

    #[test]
    fn add_edge_self_loop_is_invariant() {
        let db = new_db();
        let project_id = new_project(&db);
        let a = add_concept(&db, &project_id, "a");
        let err = db
            .add_edge(&a.id, &a.id, GraphEdgeKind::Relates, "")
            .unwrap_err();
        assert!(matches!(err, OrchdPersistError::Invariant(_)));
    }

    #[test]
    fn add_edge_duplicate_source_target_kind_is_conflict() {
        let db = new_db();
        let project_id = new_project(&db);
        let a = add_concept(&db, &project_id, "a");
        let b = add_concept(&db, &project_id, "b");
        db.add_edge(&a.id, &b.id, GraphEdgeKind::Relates, "")
            .unwrap();
        let err = db
            .add_edge(&a.id, &b.id, GraphEdgeKind::Relates, "again")
            .unwrap_err();
        assert!(matches!(err, OrchdPersistError::Conflict(_)));

        // A different kind between the same endpoints is NOT a conflict.
        db.add_edge(&a.id, &b.id, GraphEdgeKind::Depends, "")
            .unwrap();
    }

    #[test]
    fn add_edge_unknown_endpoint_is_not_found() {
        let db = new_db();
        let project_id = new_project(&db);
        let a = add_concept(&db, &project_id, "a");
        let err = db
            .add_edge(&a.id, "no-such-node", GraphEdgeKind::Relates, "")
            .unwrap_err();
        assert!(matches!(err, OrchdPersistError::NotFound));
        let err2 = db
            .add_edge("no-such-node", &a.id, GraphEdgeKind::Relates, "")
            .unwrap_err();
        assert!(matches!(err2, OrchdPersistError::NotFound));
    }

    #[test]
    fn add_edge_blocked_when_source_project_archived() {
        let db = new_db();
        let project_a = new_project(&db);
        let project_b = new_project(&db);
        let a = add_concept(&db, &project_a, "a");
        let b = add_concept(&db, &project_b, "b");
        db.archive_project(&project_a).unwrap();
        let err = db
            .add_edge(&a.id, &b.id, GraphEdgeKind::Relates, "")
            .unwrap_err();
        assert!(matches!(err, OrchdPersistError::Invariant(_)));
    }

    #[test]
    fn add_edge_blocked_when_target_project_archived() {
        let db = new_db();
        let project_a = new_project(&db);
        let project_b = new_project(&db);
        let a = add_concept(&db, &project_a, "a");
        let b = add_concept(&db, &project_b, "b");
        db.archive_project(&project_b).unwrap();
        let err = db
            .add_edge(&a.id, &b.id, GraphEdgeKind::Relates, "")
            .unwrap_err();
        assert!(matches!(err, OrchdPersistError::Invariant(_)));
    }

    // ---- delete_edge ----

    #[test]
    fn delete_edge_removes_row() {
        let db = new_db();
        let project_id = new_project(&db);
        let a = add_concept(&db, &project_id, "a");
        let b = add_concept(&db, &project_id, "b");
        let edge = db
            .add_edge(&a.id, &b.id, GraphEdgeKind::Relates, "")
            .unwrap();
        db.delete_edge(&edge.id).unwrap();
        let gone: Option<i64> = db
            .conn()
            .query_row(
                "SELECT 1 FROM graph_edge WHERE id = ?1",
                rusqlite::params![edge.id],
                |r| r.get(0),
            )
            .optional()
            .unwrap();
        assert!(gone.is_none());
    }

    #[test]
    fn delete_edge_unknown_id_is_not_found() {
        let db = new_db();
        let err = db.delete_edge("no-such-edge").unwrap_err();
        assert!(matches!(err, OrchdPersistError::NotFound));
    }

    #[test]
    fn delete_edge_on_archived_project_endpoint_is_invariant() {
        let db = new_db();
        let project_a = new_project(&db);
        let project_b = new_project(&db);
        let a = add_concept(&db, &project_a, "a");
        let b = add_concept(&db, &project_b, "b");
        // Cross-project edge; archive ONLY the target endpoint's project.
        let edge = db
            .add_edge(&a.id, &b.id, GraphEdgeKind::Relates, "")
            .unwrap();
        db.archive_project(&project_b).unwrap();

        let err = db.delete_edge(&edge.id).unwrap_err();
        assert!(
            matches!(err, OrchdPersistError::Invariant(_)),
            "delete_edge with an archived endpoint project must be Invariant, got {err:?}"
        );

        // The edge row must still exist — the guard rejects BEFORE the DELETE runs.
        let still_present: Option<i64> = db
            .conn()
            .query_row(
                "SELECT 1 FROM graph_edge WHERE id = ?1",
                rusqlite::params![edge.id],
                |r| r.get(0),
            )
            .optional()
            .unwrap();
        assert!(
            still_present.is_some(),
            "delete_edge must not run when an endpoint's project is archived"
        );
    }

    // ---- entityRef soft-ref survival (D3) ----

    #[test]
    fn entity_ref_node_survives_deletion_of_its_non_strategic_source_idea() {
        let db = new_db();
        let project_id = new_project(&db);
        // The strategic goal's own entityRef (D6 seed) can never be exercised here — S3's
        // `delete_goal` refuses to delete the strategic goal — so this uses an ADDITIONAL
        // domain entity (an idea) instead, per the task-2 brief.
        let idea = db
            .create_idea(Some(project_id.as_str()), "Soft ref idea", "")
            .unwrap();
        let node = db
            .add_entity_ref_node(
                &project_id,
                GraphEntityType::Idea,
                &idea.id,
                "Soft ref idea",
                0.0,
                0.0,
            )
            .unwrap();

        db.delete_idea(&idea.id).unwrap();

        let survived: (String, String, String) = db
            .conn()
            .query_row(
                "SELECT kind, entity_type, entity_id FROM graph_node WHERE id = ?1",
                rusqlite::params![node.id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .expect("entityRef node must survive its source idea's deletion (D3 soft-ref)");
        assert_eq!(survived.0, "entity_ref");
        assert_eq!(survived.1, "idea");
        assert_eq!(survived.2, idea.id);

        // The domain row itself really is gone (proves this is a soft-ref, not FK-protected).
        assert!(db.list_ideas(Some(project_id.as_str())).unwrap().is_empty());
    }

    // ---- edge_endpoint_projects / node_project_ids_reachable ----

    #[test]
    fn edge_endpoint_projects_returns_both_projects() {
        let db = new_db();
        let project_a = new_project(&db);
        let project_b = new_project(&db);
        let a = add_concept(&db, &project_a, "a");
        let b = add_concept(&db, &project_b, "b");
        let edge = db
            .add_edge(&a.id, &b.id, GraphEdgeKind::Relates, "")
            .unwrap();

        let (sp, tp) = db.edge_endpoint_projects(&edge.id).unwrap();
        assert_eq!(sp, project_a);
        assert_eq!(tp, project_b);
    }

    #[test]
    fn edge_endpoint_projects_unknown_edge_is_not_found() {
        let db = new_db();
        let err = db.edge_endpoint_projects("no-such-edge").unwrap_err();
        assert!(matches!(err, OrchdPersistError::NotFound));
    }

    #[test]
    fn node_project_ids_reachable_returns_own_and_foreign_projects() {
        let db = new_db();
        let project_a = new_project(&db);
        let project_b = new_project(&db);
        let project_c = new_project(&db);
        let a = add_concept(&db, &project_a, "a");
        let b = add_concept(&db, &project_b, "b");
        let c = add_concept(&db, &project_c, "c");
        db.add_edge(&a.id, &b.id, GraphEdgeKind::Relates, "")
            .unwrap();
        db.add_edge(&a.id, &c.id, GraphEdgeKind::Depends, "")
            .unwrap();

        let mut ids = db.node_project_ids_reachable(&a.id).unwrap();
        ids.sort();
        let mut expected = vec![project_a.clone(), project_b.clone(), project_c.clone()];
        expected.sort();
        assert_eq!(ids, expected);
    }

    #[test]
    fn node_project_ids_reachable_returns_only_own_when_no_edges() {
        let db = new_db();
        let project_id = new_project(&db);
        let a = add_concept(&db, &project_id, "a");
        let ids = db.node_project_ids_reachable(&a.id).unwrap();
        assert_eq!(ids, vec![project_id]);
    }

    #[test]
    fn node_project_ids_reachable_unknown_node_is_not_found() {
        let db = new_db();
        let err = db.node_project_ids_reachable("no-such-node").unwrap_err();
        assert!(matches!(err, OrchdPersistError::NotFound));
    }

    // ==================== retrieval (task-3): list_project_graph ====================

    #[test]
    fn list_project_graph_unknown_project_is_not_found() {
        let db = new_db();
        let err = db.list_project_graph("no-such-project").unwrap_err();
        assert!(matches!(err, OrchdPersistError::NotFound));
    }

    #[test]
    fn list_project_graph_includes_own_nodes_edges_and_cross_project_external_ghost() {
        let db = new_db();
        let project_a = new_project(&db);
        let project_b = new_project(&db);
        let a1 = add_concept(&db, &project_a, "a1");
        let a2 = add_concept(&db, &project_a, "a2");
        let b1 = add_concept(&db, &project_b, "b1");

        let inner_edge = db
            .add_edge(&a1.id, &a2.id, GraphEdgeKind::Relates, "")
            .unwrap();
        let cross_edge = db
            .add_edge(&a1.id, &b1.id, GraphEdgeKind::Depends, "")
            .unwrap();

        let view = db.list_project_graph(&project_a).unwrap();

        // 2 own concept nodes + the D6-seeded strategic-goal entityRef node.
        assert_eq!(view.nodes.len(), 3);
        let node_ids: Vec<&str> = view.nodes.iter().map(|n| n.id.as_str()).collect();
        assert!(node_ids.contains(&a1.id.as_str()));
        assert!(node_ids.contains(&a2.id.as_str()));
        assert!(
            !node_ids.contains(&b1.id.as_str()),
            "a foreign node must NOT appear in `nodes`"
        );

        let edge_ids: Vec<&str> = view.edges.iter().map(|e| e.id.as_str()).collect();
        assert!(edge_ids.contains(&inner_edge.id.as_str()));
        assert!(
            edge_ids.contains(&cross_edge.id.as_str()),
            "an edge incident to an own node must be included even if its other endpoint is foreign"
        );

        assert_eq!(
            view.external_nodes.len(),
            1,
            "the foreign endpoint, deduped"
        );
        assert_eq!(view.external_nodes[0].id, b1.id);
        assert_eq!(view.external_nodes[0].project_id, project_b);
    }

    #[test]
    fn list_project_graph_dedupes_external_ghost_reached_by_multiple_edges() {
        let db = new_db();
        let project_a = new_project(&db);
        let project_b = new_project(&db);
        let a1 = add_concept(&db, &project_a, "a1");
        let a2 = add_concept(&db, &project_a, "a2");
        let b1 = add_concept(&db, &project_b, "b1");
        db.add_edge(&a1.id, &b1.id, GraphEdgeKind::Relates, "")
            .unwrap();
        db.add_edge(&a2.id, &b1.id, GraphEdgeKind::Depends, "")
            .unwrap();

        let view = db.list_project_graph(&project_a).unwrap();
        assert_eq!(
            view.external_nodes.len(),
            1,
            "b1 is reached by two edges but must appear only once in external_nodes"
        );
        assert_eq!(view.edges.len(), 2);
    }

    #[test]
    fn list_project_graph_resolves_entity_ref_label_from_renamed_source_at_read_time() {
        let db = new_db();
        let project_id = new_project(&db);
        let idea = db
            .create_idea(Some(project_id.as_str()), "Old title", "")
            .unwrap();
        let node = db
            .add_entity_ref_node(
                &project_id,
                GraphEntityType::Idea,
                &idea.id,
                "Old title",
                0.0,
                0.0,
            )
            .unwrap();

        db.update_idea(&idea.id, Some("New title"), None).unwrap();

        let view = db.list_project_graph(&project_id).unwrap();
        let resolved = view
            .nodes
            .iter()
            .find(|n| n.id == node.id)
            .expect("entityRef node must be present");
        assert_eq!(
            resolved.label, "New title",
            "entityRef label must be re-resolved from the live domain row at read time"
        );
        assert!(
            !resolved.is_orphan,
            "a live (resolved) entityRef node must not be flagged orphan"
        );
    }

    #[test]
    fn list_project_graph_keeps_stored_label_when_entity_ref_source_is_deleted() {
        let db = new_db();
        let project_id = new_project(&db);
        let idea = db
            .create_idea(Some(project_id.as_str()), "Doomed idea", "")
            .unwrap();
        let node = db
            .add_entity_ref_node(
                &project_id,
                GraphEntityType::Idea,
                &idea.id,
                "Doomed idea",
                0.0,
                0.0,
            )
            .unwrap();
        let plain = add_concept(&db, &project_id, "Plain note");

        db.delete_idea(&idea.id).unwrap();

        let view = db.list_project_graph(&project_id).unwrap();
        let orphan = view
            .nodes
            .iter()
            .find(|n| n.id == node.id)
            .expect("an orphaned entityRef node must still be present (D3 soft-ref)");
        assert_eq!(
            orphan.label, "Doomed idea",
            "orphaned entityRef must keep its stored label when the source row is gone"
        );
        assert!(
            orphan.is_orphan,
            "an entityRef whose source row was deleted must be flagged is_orphan (D3, UI renders «источник удалён»)"
        );

        let plain_node = view
            .nodes
            .iter()
            .find(|n| n.id == plain.id)
            .expect("plain (non-entityRef) node must be present");
        assert!(
            !plain_node.is_orphan,
            "a non-entityRef node must never be flagged orphan"
        );
    }

    #[test]
    fn list_project_graph_resolves_entity_ref_label_in_external_nodes_too() {
        let db = new_db();
        let project_a = new_project(&db);
        let project_b = new_project(&db);
        let idea = db
            .create_idea(Some(project_b.as_str()), "B-side idea", "")
            .unwrap();
        let ghost_node = db
            .add_entity_ref_node(
                &project_b,
                GraphEntityType::Idea,
                &idea.id,
                "B-side idea",
                0.0,
                0.0,
            )
            .unwrap();
        let a1 = add_concept(&db, &project_a, "a1");
        db.add_edge(&a1.id, &ghost_node.id, GraphEdgeKind::Relates, "")
            .unwrap();

        db.update_idea(&idea.id, Some("B-side idea renamed"), None)
            .unwrap();

        let view = db.list_project_graph(&project_a).unwrap();
        let ghost = view
            .external_nodes
            .iter()
            .find(|n| n.id == ghost_node.id)
            .expect("cross-project entityRef ghost must be present");
        assert_eq!(ghost.label, "B-side idea renamed");
    }

    // ==================== retrieval (task-3): neighborhood ====================

    #[test]
    fn neighborhood_unknown_node_id_is_not_found() {
        let db = new_db();
        let err = db.neighborhood("no-such-node", 2).unwrap_err();
        assert!(matches!(err, OrchdPersistError::NotFound));
    }

    #[test]
    fn neighborhood_depth_2_returns_exact_2hop_reachable_set_across_cross_project_edge() {
        let db = new_db();
        let project_a = new_project(&db);
        let project_b = new_project(&db);
        let n0 = add_concept(&db, &project_a, "n0"); // root
        let n1 = add_concept(&db, &project_a, "n1"); // hop 1, same project
        let n2 = add_concept(&db, &project_b, "n2"); // hop 2, cross-project via n1
        let n3 = add_concept(&db, &project_b, "n3"); // hop 3, must NOT be reached at depth 2
        let unrelated = add_concept(&db, &project_a, "unrelated");

        db.add_edge(&n0.id, &n1.id, GraphEdgeKind::Relates, "")
            .unwrap();
        db.add_edge(&n1.id, &n2.id, GraphEdgeKind::Relates, "")
            .unwrap(); // cross-project edge
        db.add_edge(&n2.id, &n3.id, GraphEdgeKind::Relates, "")
            .unwrap();

        let nb = db.neighborhood(&n0.id, 2).unwrap();
        assert_eq!(nb.root_id, n0.id);

        let mut ids: Vec<&str> = nb.nodes.iter().map(|n| n.id.as_str()).collect();
        ids.sort();
        let mut expected = vec![n0.id.as_str(), n1.id.as_str(), n2.id.as_str()];
        expected.sort();
        assert_eq!(
            ids, expected,
            "exactly the 2-hop reachable set, including the cross-project node"
        );
        assert!(
            !nb.nodes.iter().any(|n| n.id == n3.id),
            "the 3-hop node must not appear at depth 2"
        );
        assert!(!nb.nodes.iter().any(|n| n.id == unrelated.id));

        assert_eq!(
            nb.edges.len(),
            2,
            "only the n0-n1 and n1-n2 edges have both endpoints in the 2-hop reachable set"
        );
    }

    #[test]
    fn neighborhood_depth_over_6_is_clamped_to_6() {
        let db = new_db();
        let project_id = new_project(&db);
        // A 7-hop chain: n0-n1-...-n7 (8 nodes, 7 edges). n7 is exactly 7 hops from n0.
        let chain: Vec<GraphNode> = (0..8)
            .map(|i| add_concept(&db, &project_id, &format!("n{i}")))
            .collect();
        for i in 0..7 {
            db.add_edge(&chain[i].id, &chain[i + 1].id, GraphEdgeKind::Relates, "")
                .unwrap();
        }

        let nb = db.neighborhood(&chain[0].id, 99).unwrap();
        let ids: HashSet<&str> = nb.nodes.iter().map(|n| n.id.as_str()).collect();

        for node in chain.iter().take(7) {
            assert!(
                ids.contains(node.id.as_str()),
                "{} must be within the clamped depth-6 reach",
                node.label
            );
        }
        assert!(
            !ids.contains(chain[7].id.as_str()),
            "n7 is 7 hops away — depth 99 clamped to 6 must NOT reach it"
        );
        assert_eq!(nb.nodes.len(), 7, "exactly n0..n6 (depth <= 6)");
    }

    #[test]
    fn neighborhood_traverses_edges_in_both_directions() {
        let db = new_db();
        let project_id = new_project(&db);
        let a = add_concept(&db, &project_id, "a");
        let b = add_concept(&db, &project_id, "b");
        // a -> b as source->target; querying FROM b must still reach a (bidirectional).
        db.add_edge(&a.id, &b.id, GraphEdgeKind::Relates, "")
            .unwrap();

        let nb = db.neighborhood(&b.id, 1).unwrap();
        let ids: HashSet<&str> = nb.nodes.iter().map(|n| n.id.as_str()).collect();
        assert!(ids.contains(a.id.as_str()));
        assert!(ids.contains(b.id.as_str()));
    }

    // ==================== retrieval (task-3): search_nodes ====================

    #[test]
    fn search_nodes_none_project_spans_workspace() {
        let db = new_db();
        let project_a = new_project(&db);
        let project_b = new_project(&db);
        db.add_node(
            &project_a,
            GraphNodeKind::Concept,
            "Widget alpha",
            "",
            0.0,
            0.0,
        )
        .unwrap();
        db.add_node(
            &project_b,
            GraphNodeKind::Concept,
            "Widget beta",
            "",
            0.0,
            0.0,
        )
        .unwrap();
        db.add_node(
            &project_a,
            GraphNodeKind::Concept,
            "Unrelated",
            "",
            0.0,
            0.0,
        )
        .unwrap();

        let results = db.search_nodes("widget", None).unwrap();
        let labels: Vec<&str> = results.iter().map(|n| n.label.as_str()).collect();
        assert!(labels.contains(&"Widget alpha"));
        assert!(labels.contains(&"Widget beta"));
        assert!(!labels.contains(&"Unrelated"));
    }

    #[test]
    fn search_nodes_some_project_scopes_to_that_project() {
        let db = new_db();
        let project_a = new_project(&db);
        let project_b = new_project(&db);
        db.add_node(
            &project_a,
            GraphNodeKind::Concept,
            "Widget alpha",
            "",
            0.0,
            0.0,
        )
        .unwrap();
        db.add_node(
            &project_b,
            GraphNodeKind::Concept,
            "Widget beta",
            "",
            0.0,
            0.0,
        )
        .unwrap();

        let results = db.search_nodes("widget", Some(&project_a)).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].label, "Widget alpha");
    }

    #[test]
    fn search_nodes_matches_body_too() {
        let db = new_db();
        let project_id = new_project(&db);
        db.add_node(
            &project_id,
            GraphNodeKind::Concept,
            "Label only",
            "mentions gizmo in the body",
            0.0,
            0.0,
        )
        .unwrap();

        let results = db.search_nodes("gizmo", None).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn search_nodes_is_case_insensitive() {
        let db = new_db();
        let project_id = new_project(&db);
        db.add_node(
            &project_id,
            GraphNodeKind::Concept,
            "MixedCase Widget",
            "",
            0.0,
            0.0,
        )
        .unwrap();

        let results = db.search_nodes("WIDGET", None).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn search_nodes_orders_by_updated_at_desc() {
        let db = new_db();
        let project_id = new_project(&db);
        let older = db
            .add_node(
                &project_id,
                GraphNodeKind::Concept,
                "Match one",
                "",
                0.0,
                0.0,
            )
            .unwrap();
        let newer = db
            .add_node(
                &project_id,
                GraphNodeKind::Concept,
                "Match two",
                "",
                0.0,
                0.0,
            )
            .unwrap();
        // Force distinct timestamps regardless of clock resolution (mirrors the codebase's
        // `list_ideas_orders_created_at_desc` convention), so DESC order is unambiguous.
        db.conn()
            .execute(
                "UPDATE graph_node SET updated_at = 1000 WHERE id = ?1",
                rusqlite::params![older.id],
            )
            .unwrap();
        db.conn()
            .execute(
                "UPDATE graph_node SET updated_at = 2000 WHERE id = ?1",
                rusqlite::params![newer.id],
            )
            .unwrap();

        let results = db.search_nodes("match", None).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(
            results[0].id, newer.id,
            "most recently updated match sorts first"
        );
        assert_eq!(results[1].id, older.id);
    }

    #[test]
    fn search_nodes_caps_at_200_rows() {
        let db = new_db();
        let project_id = new_project(&db);
        for i in 0..205 {
            db.add_node(
                &project_id,
                GraphNodeKind::Concept,
                &format!("cap-match-{i}"),
                "",
                0.0,
                0.0,
            )
            .unwrap();
        }

        let results = db.search_nodes("cap-match", None).unwrap();
        assert_eq!(results.len(), 200);
    }

    // ==================== perf DoD: "a goal's subgraph <100 ms" (S4 roadmap DoD) ====================

    #[test]
    fn neighborhood_depth_3_on_500_node_1000_edge_graph_is_under_100ms_rooted_at_goal_node() {
        let db = new_db();
        let project_id = new_project(&db);
        let goal_node_id: String = db
            .conn()
            .query_row(
                "SELECT id FROM graph_node WHERE project_id = ?1 AND kind = 'entity_ref'
                   AND entity_type = 'goal'",
                rusqlite::params![project_id],
                |r| r.get(0),
            )
            .expect("the D6 seed must have created a strategic-goal entityRef node");

        // Build 499 more nodes (500 total incl. the goal node) as a 3-layer tree rooted at the
        // goal node: layer1 = 10 direct children, layer2 = 100 grandchildren (round-robin under
        // layer1), layer3 = 389 great-grandchildren (round-robin under layer2). Every node ends
        // up within EXACTLY 3 hops of the goal node — a "rich" neighborhood by construction, with
        // a trivial closed-form expected reachable count (all 500 nodes).
        let mut node_ids: Vec<String> = vec![goal_node_id.clone()];
        for i in 0..499 {
            let n = db
                .add_node(
                    &project_id,
                    GraphNodeKind::Concept,
                    &format!("n{i}"),
                    "",
                    0.0,
                    0.0,
                )
                .unwrap();
            node_ids.push(n.id);
        }
        let layer1 = node_ids[1..11].to_vec();
        let layer2 = node_ids[11..111].to_vec();
        let layer3 = node_ids[111..500].to_vec();
        assert_eq!(1 + layer1.len() + layer2.len() + layer3.len(), 500);

        for child in &layer1 {
            db.add_edge(&goal_node_id, child, GraphEdgeKind::Relates, "")
                .unwrap();
        }
        for (i, child) in layer2.iter().enumerate() {
            db.add_edge(&layer1[i % layer1.len()], child, GraphEdgeKind::Relates, "")
                .unwrap();
        }
        for (i, child) in layer3.iter().enumerate() {
            db.add_edge(&layer2[i % layer2.len()], child, GraphEdgeKind::Relates, "")
                .unwrap();
        }
        // 10 + 100 + 389 = 499 tree edges so far.

        // Pad to 1000 edges with extra chord edges among already-existing nodes — this does NOT
        // shrink the depth-3 reachable set (every node is already reachable via its tree-parent
        // path) but exercises the CTE against a denser edge set, matching the roadmap DoD's
        // "500-node/1000-edge" scale.
        let mut extra = 0usize;
        'outer: for offset in [137usize, 271, 359, 443, 91, 211] {
            for i in 0..500usize {
                if extra >= 501 {
                    break 'outer;
                }
                let b = (i + offset) % 500;
                if i == b {
                    continue;
                }
                match db.add_edge(&node_ids[i], &node_ids[b], GraphEdgeKind::Relates, "") {
                    Ok(_) => extra += 1,
                    Err(OrchdPersistError::Conflict(_)) => continue,
                    Err(e) => panic!("unexpected error building the perf-test graph: {e:?}"),
                }
            }
        }
        assert_eq!(
            extra, 501,
            "must have built exactly 501 chord edges (499 tree + 501 = 1000 total)"
        );

        let total_nodes: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM graph_node", [], |r| r.get(0))
            .unwrap();
        let total_edges: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM graph_edge", [], |r| r.get(0))
            .unwrap();
        assert_eq!(total_nodes, 500);
        assert_eq!(total_edges, 1000);

        let start = std::time::Instant::now();
        let neighborhood = db.neighborhood(&goal_node_id, 3).unwrap();
        let elapsed = start.elapsed();

        assert_eq!(neighborhood.root_id, goal_node_id);
        assert_eq!(
            neighborhood.nodes.len(),
            500,
            "every node in the synthetic 3-layer tree is within 3 hops of the goal node"
        );
        // The <100ms product SLA (spec §5 DoD) is a RELEASE-build property. A debug build
        // links `libsqlite3-sys`'s bundled SQLite compiled unoptimized (`-O0`), which runs the
        // recursive CTE ~10-50x slower, and CI additionally runs this suite under coverage
        // instrumentation on shared runners far slower than a dev laptop — so a hard 100ms debug
        // assertion measures the wrong thing and flakes across environments. Enforce the strict
        // budget only in release (the real shipped perf); in debug assert a generous ceiling that
        // still catches an algorithmic (O(n²)+ / lost depth-bound) regression without being
        // environment-fragile. Correctness (the exact reachable set) is asserted unconditionally
        // above — that is the guarantee that must never regress regardless of build profile.
        #[cfg(not(debug_assertions))]
        assert!(
            elapsed.as_millis() < 100,
            "release neighborhood(depth 3) on a 500-node/1000-edge graph took {elapsed:?}; DoD is <100ms"
        );
        #[cfg(debug_assertions)]
        assert!(
            elapsed.as_millis() < 2000,
            "debug neighborhood(depth 3) ceiling exceeded ({elapsed:?}); an unoptimized/instrumented \
             build is slow, but >2s signals an algorithmic regression, not just profile overhead"
        );
    }
}
