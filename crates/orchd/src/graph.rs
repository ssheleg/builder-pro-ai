//! Knowledge-graph node/edge persistence (S4 spec §4 schema v2, §5 persistence + invariants).
//! Sibling module to `persistence` (crate-private — `mod graph;` in `lib.rs`): builds directly on
//! `persistence::Db`'s `conn()` seam plus its `pub(crate)` helpers (`ensure_project_active`,
//! `now_ms`, `is_constraint_violation`) exactly like `export.rs` reuses `persistence`'s
//! `insert_*_raw` helpers. Enum⇄TEXT snake_case mapping mirrors S3's idea/insight/task helpers
//! (e.g. `IdeaLifecycle::InDev` ⇒ DB literal `"in_dev"`): the wire serde repr is camelCase, the
//! DB CHECK-constraint literal is snake_case, and this module owns that mapping —
//! `GraphNodeKind::EntityRef` ⇒ DB literal `'entity_ref'`.
//!
//! `#![allow(dead_code)]`: this task (S4 T2) ships the persistence layer only — every item here
//! is exercised by this module's own `#[cfg(test)]` suite, but `socket_server.rs`'s dispatch
//! match currently has only a placeholder wildcard arm for the `Graph*` wire verbs (S4 spec §6
//! dispatch wiring is a separate follow-up task). Without this allow, `cargo clippy --all-targets
//! -D warnings` would flag every export here as dead code in the plain `lib` build (where
//! `#[cfg(test)]` isn't compiled). Remove this attribute once dispatch wires these in.
#![allow(dead_code)]

use bpa_orchd_proto::{GraphEdge, GraphEdgeKind, GraphEntityType, GraphNode, GraphNodeKind};
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

impl Db {
    /// `GraphAddNode` (S4 spec §5): rejects `kind == EntityRef` — entityRef nodes are created
    /// ONLY via [`Db::add_entity_ref_node`], never this generic wire verb (this prevents the
    /// DDL's atomic `(kind='entity_ref') = (entity_type/entity_id set)` CHECK from firing as a
    /// raw SQL error for a caller that forgot to also supply `entity_type`/`entity_id`). Archived
    /// project ⇒ `Invariant`; unknown project ⇒ `NotFound`.
    pub(crate) fn add_node(
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
    /// verb in S4 — internal-only, used by the D6 seed + future auto-population.
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
    pub(crate) fn update_node(
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
    pub(crate) fn delete_node(&self, id: &str) -> Result<(), OrchdPersistError> {
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
    pub(crate) fn add_edge(
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

    /// `GraphDeleteEdge` (S4 spec §5). Unknown id ⇒ `NotFound`.
    pub(crate) fn delete_edge(&self, id: &str) -> Result<(), OrchdPersistError> {
        let tx = self.conn().unchecked_transaction()?;
        let exists: Option<i64> = tx
            .query_row(
                "SELECT 1 FROM graph_edge WHERE id = ?1",
                rusqlite::params![id],
                |r| r.get(0),
            )
            .optional()?;
        exists.ok_or(OrchdPersistError::NotFound)?;
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

    // ---- schema v2 (fresh DB) ----

    #[test]
    fn fresh_db_is_schema_v2_with_graph_tables_and_all_five_indexes() {
        let db = new_db();
        let version: i64 = db
            .conn()
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 2);
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
}
