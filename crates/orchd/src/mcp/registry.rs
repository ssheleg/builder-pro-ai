//! `mcp_server` / `mcp_tool` CRUD (S-EXT spec §4 schema v3, task T2). Builds directly on
//! `persistence::Db`'s `conn()` seam plus its `now_ms`/`OrchdPersistError` — exactly like
//! `crate::graph` builds on `persistence` (see that module's doc comment). Enum⇄TEXT snake_case
//! mapping mirrors S3/S4's existing helpers (e.g. `persistence::encode_goal_kind`,
//! `graph::encode_node_kind`): the DB CHECK-constraint literal IS the snake_case Rust variant
//! name lowercased, there is no separate camelCase wire repr yet (T3 hasn't landed).
use std::collections::BTreeMap;

use rusqlite::{Connection, OptionalExtension};
use uuid::Uuid;

use crate::persistence::{now_ms, Db, OrchdPersistError};

use super::{
    McpAuthKind, McpScope, McpServerPatch, McpServerRow, McpToolRow, McpTransport, NewMcpServer,
    NewMcpTool,
};

// ---- mcp_server enum <-> TEXT helpers (spec §4 CHECK literals) ----

fn encode_transport(t: &McpTransport) -> &'static str {
    match t {
        McpTransport::Http => "http",
        McpTransport::Stdio => "stdio",
    }
}

fn decode_transport(s: &str) -> Result<McpTransport, OrchdPersistError> {
    match s {
        "http" => Ok(McpTransport::Http),
        "stdio" => Ok(McpTransport::Stdio),
        other => Err(OrchdPersistError::Io(format!(
            "corrupt mcp_server.transport value: {other}"
        ))),
    }
}

fn encode_scope(s: &McpScope) -> &'static str {
    match s {
        McpScope::Global => "global",
        McpScope::Project => "project",
    }
}

fn decode_scope(s: &str) -> Result<McpScope, OrchdPersistError> {
    match s {
        "global" => Ok(McpScope::Global),
        "project" => Ok(McpScope::Project),
        other => Err(OrchdPersistError::Io(format!(
            "corrupt mcp_server.scope value: {other}"
        ))),
    }
}

fn encode_auth_kind(k: &McpAuthKind) -> &'static str {
    match k {
        McpAuthKind::None => "none",
        McpAuthKind::Bearer => "bearer",
        McpAuthKind::Oauth => "oauth",
    }
}

fn decode_auth_kind(s: &str) -> Result<McpAuthKind, OrchdPersistError> {
    match s {
        "none" => Ok(McpAuthKind::None),
        "bearer" => Ok(McpAuthKind::Bearer),
        "oauth" => Ok(McpAuthKind::Oauth),
        other => Err(OrchdPersistError::Io(format!(
            "corrupt mcp_server.auth_kind value: {other}"
        ))),
    }
}

/// `mcp_server.args_json` JSON array-of-strings round-trip (mirrors
/// `persistence::decode_metric_refs`/`decode_tags`'s shape).
fn encode_args(args: &[String]) -> Result<String, OrchdPersistError> {
    serde_json::to_string(args)
        .map_err(|e| OrchdPersistError::Io(format!("failed to serialize mcp_server.args: {e}")))
}

fn decode_args(s: &str) -> Result<Vec<String>, OrchdPersistError> {
    serde_json::from_str(s)
        .map_err(|e| OrchdPersistError::Io(format!("corrupt mcp_server.args_json: {e}")))
}

/// `mcp_server.env_json` JSON object-of-strings round-trip. `BTreeMap` (not `HashMap`) so
/// round-tripped env vars come back in a deterministic order — the DB stores JSON text either
/// way, this only affects Rust-side iteration order.
fn encode_env(env: &BTreeMap<String, String>) -> Result<String, OrchdPersistError> {
    serde_json::to_string(env)
        .map_err(|e| OrchdPersistError::Io(format!("failed to serialize mcp_server.env: {e}")))
}

fn decode_env(s: &str) -> Result<BTreeMap<String, String>, OrchdPersistError> {
    serde_json::from_str(s)
        .map_err(|e| OrchdPersistError::Io(format!("corrupt mcp_server.env_json: {e}")))
}

/// Validates the two spec §4 CHECK invariants on `mcp_server` in Rust BEFORE the insert, so a
/// caller gets a typed `Validation` error rather than a raw SQLite `ConstraintViolation` (task-2
/// brief: "enforce the §4 CHECK invariants in Rust too"). Both are biconditionals in the DDL
/// (`(scope='project') = (project_id IS NOT NULL)`, `(transport='http') = (url IS NOT NULL)`), so
/// both directions are checked, not just the "positive" direction named in the brief's examples.
fn validate_new_server(new: &NewMcpServer) -> Result<(), OrchdPersistError> {
    match (&new.scope, &new.project_id) {
        (McpScope::Project, None) => {
            return Err(OrchdPersistError::Validation(
                "mcp_server: scope='project' requires project_id".to_string(),
            ))
        }
        (McpScope::Global, Some(_)) => {
            return Err(OrchdPersistError::Validation(
                "mcp_server: scope='global' requires project_id to be absent".to_string(),
            ))
        }
        _ => {}
    }
    match (&new.transport, &new.url) {
        (McpTransport::Http, None) => {
            return Err(OrchdPersistError::Validation(
                "mcp_server: transport='http' requires url".to_string(),
            ))
        }
        (McpTransport::Stdio, Some(_)) => {
            return Err(OrchdPersistError::Validation(
                "mcp_server: transport='stdio' requires url to be absent".to_string(),
            ))
        }
        _ => {}
    }
    Ok(())
}

/// Raw `mcp_server` row (text-encoded `transport`/`scope`/`auth_kind`, JSON-encoded
/// `args`/`env`) before decoding into [`McpServerRow`] — mirrors `persistence::GoalRow`'s shape.
struct McpServerRawRow {
    id: String,
    name: String,
    transport: String,
    url: Option<String>,
    command: Option<String>,
    args_json: String,
    env_json: String,
    scope: String,
    project_id: Option<String>,
    auth_kind: String,
    secret_ref: Option<String>,
    account_id: Option<String>,
    enabled: i64,
    timeout_ms: i64,
    max_retries: i64,
    protocol_version: Option<String>,
    created_at: i64,
    updated_at: i64,
}

impl McpServerRawRow {
    fn from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<McpServerRawRow> {
        Ok(McpServerRawRow {
            id: r.get(0)?,
            name: r.get(1)?,
            transport: r.get(2)?,
            url: r.get(3)?,
            command: r.get(4)?,
            args_json: r.get(5)?,
            env_json: r.get(6)?,
            scope: r.get(7)?,
            project_id: r.get(8)?,
            auth_kind: r.get(9)?,
            secret_ref: r.get(10)?,
            account_id: r.get(11)?,
            enabled: r.get(12)?,
            timeout_ms: r.get(13)?,
            max_retries: r.get(14)?,
            protocol_version: r.get(15)?,
            created_at: r.get(16)?,
            updated_at: r.get(17)?,
        })
    }

    fn into_row(self) -> Result<McpServerRow, OrchdPersistError> {
        Ok(McpServerRow {
            id: self.id,
            name: self.name,
            transport: decode_transport(&self.transport)?,
            url: self.url,
            command: self.command,
            args: decode_args(&self.args_json)?,
            env: decode_env(&self.env_json)?,
            scope: decode_scope(&self.scope)?,
            project_id: self.project_id,
            auth_kind: decode_auth_kind(&self.auth_kind)?,
            secret_ref: self.secret_ref,
            account_id: self.account_id,
            enabled: self.enabled != 0,
            timeout_ms: self.timeout_ms,
            max_retries: self.max_retries,
            protocol_version: self.protocol_version,
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}

const MCP_SERVER_COLUMNS: &str = "id, name, transport, url, command, args_json, env_json, scope, \
     project_id, auth_kind, secret_ref, account_id, enabled, timeout_ms, max_retries, \
     protocol_version, created_at, updated_at";

fn load_server(conn: &Connection, id: &str) -> Result<McpServerRow, OrchdPersistError> {
    let sql = format!("SELECT {MCP_SERVER_COLUMNS} FROM mcp_server WHERE id = ?1");
    conn.query_row(&sql, rusqlite::params![id], McpServerRawRow::from_row)
        .optional()?
        .ok_or(OrchdPersistError::NotFound)?
        .into_row()
}

fn load_tool(conn: &Connection, id: &str) -> Result<McpToolRow, OrchdPersistError> {
    conn.query_row(
        "SELECT id, server_id, name, title, description, input_schema_json, enabled, fetched_at
         FROM mcp_tool WHERE id = ?1",
        rusqlite::params![id],
        |r| {
            Ok(McpToolRow {
                id: r.get(0)?,
                server_id: r.get(1)?,
                name: r.get(2)?,
                title: r.get(3)?,
                description: r.get(4)?,
                input_schema_json: r.get(5)?,
                enabled: r.get::<_, i64>(6)? != 0,
                fetched_at: r.get(7)?,
            })
        },
    )
    .optional()?
    .ok_or(OrchdPersistError::NotFound)
}

impl Db {
    /// `add_mcp_server` (S-EXT spec §4, task-2 brief): validates the scope⇄project_id and
    /// transport⇄url CHECK invariants in Rust ([`validate_new_server`]) before the insert — a
    /// caller that got either wrong gets a typed `Validation` error, never a raw SQLite
    /// `ConstraintViolation`. `id`/`created_at`/`updated_at` are assigned here (uuid v4 /
    /// `now_ms()`); `protocol_version` starts `NULL` (spec §4: "null until first connect").
    pub fn add_mcp_server(&self, new: NewMcpServer) -> Result<McpServerRow, OrchdPersistError> {
        validate_new_server(&new)?;
        let tx = self.conn().unchecked_transaction()?;
        let id = Uuid::new_v4().to_string();
        let now = now_ms();
        let args_json = encode_args(&new.args)?;
        let env_json = encode_env(&new.env)?;
        tx.execute(
            "INSERT INTO mcp_server
               (id, name, transport, url, command, args_json, env_json, scope, project_id,
                auth_kind, secret_ref, account_id, enabled, timeout_ms, max_retries,
                protocol_version, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, NULL, ?16, ?16)",
            rusqlite::params![
                id,
                new.name,
                encode_transport(&new.transport),
                new.url,
                new.command,
                args_json,
                env_json,
                encode_scope(&new.scope),
                new.project_id,
                encode_auth_kind(&new.auth_kind),
                new.secret_ref,
                new.account_id,
                new.enabled as i64,
                new.timeout_ms,
                new.max_retries,
                now,
            ],
        )?;
        let row = load_server(&tx, &id)?;
        tx.commit()?;
        Ok(row)
    }

    /// `get_mcp_server` (task-5 addition): the single-row counterpart to `list_mcp_servers`,
    /// needed by `mcp::lifecycle`/`mcp::invoke` (T5) to resolve a server's `url`/`auth_kind`/
    /// `timeout_ms`/`max_retries` before connecting or calling a tool. Not in the task-2 brief's
    /// original method list (T2 predates that need). Unknown `id` ⇒ `NotFound`.
    pub fn get_mcp_server(&self, id: &str) -> Result<McpServerRow, OrchdPersistError> {
        load_server(self.conn(), id)
    }

    /// `list_mcp_servers` (task-2 brief): `Some(project_id)` returns global-scope servers PLUS
    /// that project's own; `None` returns global-scope servers only (no project context to join
    /// against). A single parameterized query handles both via `?1 IS NOT NULL AND ...` rather
    /// than branching into two SQL strings.
    pub fn list_mcp_servers(
        &self,
        project_id: Option<&str>,
    ) -> Result<Vec<McpServerRow>, OrchdPersistError> {
        let mut stmt = self.conn().prepare(
            "SELECT id FROM mcp_server
             WHERE scope = 'global' OR (?1 IS NOT NULL AND scope = 'project' AND project_id = ?1)
             ORDER BY created_at, id",
        )?;
        let ids: Vec<String> = stmt
            .query_map(rusqlite::params![project_id], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        drop(stmt);
        ids.iter().map(|id| load_server(self.conn(), id)).collect()
    }

    /// `update_mcp_server` (task-2 brief): COALESCE idiom, mirrors
    /// `persistence::Db::update_project`/`update_goal` — a field left `None` in `patch` is left
    /// untouched, `updated_at` only bumps if at least one field was actually provided. `url` can
    /// only be RE-set to a new value, never cleared (same COALESCE convention), and is rejected
    /// with `Validation` when the server's `transport` is `stdio` — that combination is exactly
    /// the one way this patch shape could otherwise violate the spec §4
    /// `(transport='http') = (url IS NOT NULL)` CHECK (see [`super::McpServerPatch`]'s doc
    /// comment). Unknown `id` ⇒ `NotFound`.
    pub fn update_mcp_server(
        &self,
        id: &str,
        patch: McpServerPatch,
    ) -> Result<McpServerRow, OrchdPersistError> {
        let tx = self.conn().unchecked_transaction()?;
        let current_transport: String = tx
            .query_row(
                "SELECT transport FROM mcp_server WHERE id = ?1",
                rusqlite::params![id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(OrchdPersistError::NotFound)?;
        if patch.url.is_some() && current_transport == "stdio" {
            return Err(OrchdPersistError::Validation(
                "mcp_server: cannot set url on a stdio-transport server".to_string(),
            ));
        }

        let args_json = patch.args.as_deref().map(encode_args).transpose()?;
        let env_json = patch.env.as_ref().map(encode_env).transpose()?;
        let auth_kind_text = patch.auth_kind.as_ref().map(encode_auth_kind);

        let any_field = patch.name.is_some()
            || patch.url.is_some()
            || patch.command.is_some()
            || args_json.is_some()
            || env_json.is_some()
            || auth_kind_text.is_some()
            || patch.secret_ref.is_some()
            || patch.account_id.is_some()
            || patch.timeout_ms.is_some()
            || patch.max_retries.is_some();
        if any_field {
            tx.execute(
                "UPDATE mcp_server SET
                   name = COALESCE(?2, name),
                   url = COALESCE(?3, url),
                   command = COALESCE(?4, command),
                   args_json = COALESCE(?5, args_json),
                   env_json = COALESCE(?6, env_json),
                   auth_kind = COALESCE(?7, auth_kind),
                   secret_ref = COALESCE(?8, secret_ref),
                   account_id = COALESCE(?9, account_id),
                   timeout_ms = COALESCE(?10, timeout_ms),
                   max_retries = COALESCE(?11, max_retries),
                   updated_at = ?12
                 WHERE id = ?1",
                rusqlite::params![
                    id,
                    patch.name,
                    patch.url,
                    patch.command,
                    args_json,
                    env_json,
                    auth_kind_text,
                    patch.secret_ref,
                    patch.account_id,
                    patch.timeout_ms,
                    patch.max_retries,
                    now_ms(),
                ],
            )?;
        }
        let row = load_server(&tx, id)?;
        tx.commit()?;
        Ok(row)
    }

    /// `set_mcp_server_enabled` (task-2 brief). Unknown `id` ⇒ `NotFound`.
    pub fn set_mcp_server_enabled(
        &self,
        id: &str,
        enabled: bool,
    ) -> Result<McpServerRow, OrchdPersistError> {
        let tx = self.conn().unchecked_transaction()?;
        let changed = tx.execute(
            "UPDATE mcp_server SET enabled = ?2, updated_at = ?3 WHERE id = ?1",
            rusqlite::params![id, enabled as i64, now_ms()],
        )?;
        if changed == 0 {
            return Err(OrchdPersistError::NotFound);
        }
        let row = load_server(&tx, id)?;
        tx.commit()?;
        Ok(row)
    }

    /// `set_mcp_server_secret_ref` (task-2 brief): stores the Keychain account key a later task's
    /// bearer-auth secret-store flow (`bpa-secrets`) writes the actual token bytes under — this
    /// method only persists the REFERENCE, never a secret value itself. Unknown `id` ⇒
    /// `NotFound`.
    pub fn set_mcp_server_secret_ref(
        &self,
        id: &str,
        secret_ref: &str,
    ) -> Result<McpServerRow, OrchdPersistError> {
        let tx = self.conn().unchecked_transaction()?;
        let changed = tx.execute(
            "UPDATE mcp_server SET secret_ref = ?2, updated_at = ?3 WHERE id = ?1",
            rusqlite::params![id, secret_ref, now_ms()],
        )?;
        if changed == 0 {
            return Err(OrchdPersistError::NotFound);
        }
        let row = load_server(&tx, id)?;
        tx.commit()?;
        Ok(row)
    }

    /// `delete_mcp_server` (task-2 brief): `mcp_tool.server_id REFERENCES mcp_server(id) ON
    /// DELETE CASCADE` (spec §4) removes the server's cached tools automatically — `foreign_keys`
    /// is `ON` for every `Db` connection (`persistence::Db::open`/`open_in_memory`). Unknown `id`
    /// ⇒ `NotFound`.
    pub fn delete_mcp_server(&self, id: &str) -> Result<(), OrchdPersistError> {
        let tx = self.conn().unchecked_transaction()?;
        let changed = tx.execute(
            "DELETE FROM mcp_server WHERE id = ?1",
            rusqlite::params![id],
        )?;
        if changed == 0 {
            return Err(OrchdPersistError::NotFound);
        }
        tx.commit()?;
        Ok(())
    }

    /// `upsert_mcp_tools` (task-2 brief): REPLACES the server's entire cached tool list —
    /// deletes every existing `mcp_tool` row for `server_id`, then inserts `tools` fresh, each
    /// `enabled=1` (spec §4: "default on-fetch" — see [`super::NewMcpTool`]'s doc comment).
    /// Unknown `server_id` ⇒ `NotFound` (checked up front so an empty `tools` list against an
    /// unknown server doesn't silently no-op).
    pub fn upsert_mcp_tools(
        &self,
        server_id: &str,
        tools: Vec<NewMcpTool>,
    ) -> Result<(), OrchdPersistError> {
        let tx = self.conn().unchecked_transaction()?;
        let server_exists: Option<i64> = tx
            .query_row(
                "SELECT 1 FROM mcp_server WHERE id = ?1",
                rusqlite::params![server_id],
                |r| r.get(0),
            )
            .optional()?;
        if server_exists.is_none() {
            return Err(OrchdPersistError::NotFound);
        }
        tx.execute(
            "DELETE FROM mcp_tool WHERE server_id = ?1",
            rusqlite::params![server_id],
        )?;
        let now = now_ms();
        for tool in &tools {
            let id = Uuid::new_v4().to_string();
            tx.execute(
                "INSERT INTO mcp_tool
                   (id, server_id, name, title, description, input_schema_json, enabled, fetched_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7)",
                rusqlite::params![
                    id,
                    server_id,
                    tool.name,
                    tool.title,
                    tool.description,
                    tool.input_schema_json,
                    now,
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// `list_mcp_tools` (task-2 brief): every cached tool for `server_id`, name-ordered.
    pub fn list_mcp_tools(&self, server_id: &str) -> Result<Vec<McpToolRow>, OrchdPersistError> {
        let mut stmt = self
            .conn()
            .prepare("SELECT id FROM mcp_tool WHERE server_id = ?1 ORDER BY name")?;
        let ids: Vec<String> = stmt
            .query_map(rusqlite::params![server_id], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        drop(stmt);
        ids.iter().map(|id| load_tool(self.conn(), id)).collect()
    }

    /// `set_mcp_tool_enabled` (task-2 brief, per-tool allowlist toggle). Unknown `tool_id` ⇒
    /// `NotFound`. `mcp_tool` has no `updated_at` column (spec §4 — only `fetched_at`, the
    /// fetch-cache timestamp), so toggling `enabled` deliberately does not touch it.
    pub fn set_mcp_tool_enabled(
        &self,
        tool_id: &str,
        enabled: bool,
    ) -> Result<McpToolRow, OrchdPersistError> {
        let tx = self.conn().unchecked_transaction()?;
        let changed = tx.execute(
            "UPDATE mcp_tool SET enabled = ?2 WHERE id = ?1",
            rusqlite::params![tool_id, enabled as i64],
        )?;
        if changed == 0 {
            return Err(OrchdPersistError::NotFound);
        }
        let row = load_tool(&tx, tool_id)?;
        tx.commit()?;
        Ok(row)
    }

    /// `get_mcp_tool` (task-2 brief): `Ok(None)` for an unknown id (a lookup, not a mutator —
    /// unlike every other method above, a missing row is not itself an error here).
    pub fn get_mcp_tool(&self, tool_id: &str) -> Result<Option<McpToolRow>, OrchdPersistError> {
        match load_tool(self.conn(), tool_id) {
            Ok(row) => Ok(Some(row)),
            Err(OrchdPersistError::NotFound) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_db() -> Db {
        Db::open_in_memory().unwrap()
    }

    fn new_project(db: &Db) -> String {
        // `project_workspace.workspace_id` is UNIQUE across the whole table (S3 spec §5.2) — a
        // fresh uuid per call so multi-project tests don't collide (mirrors `graph`'s test
        // helper of the same name).
        let workspace_id = Uuid::new_v4().to_string();
        db.create_project("P", "", &[workspace_id]).unwrap().id
    }

    fn http_server(name: &str, scope: McpScope, project_id: Option<String>) -> NewMcpServer {
        NewMcpServer {
            name: name.to_string(),
            transport: McpTransport::Http,
            url: Some("https://example.com/mcp".to_string()),
            command: None,
            args: vec![],
            env: BTreeMap::new(),
            scope,
            project_id,
            auth_kind: McpAuthKind::None,
            secret_ref: None,
            account_id: None,
            enabled: true,
            timeout_ms: 30_000,
            max_retries: 2,
        }
    }

    fn stdio_server(name: &str) -> NewMcpServer {
        NewMcpServer {
            name: name.to_string(),
            transport: McpTransport::Stdio,
            url: None,
            command: Some("/usr/local/bin/some-mcp-server".to_string()),
            args: vec!["--flag".to_string()],
            env: BTreeMap::from([("KEY".to_string(), "value".to_string())]),
            scope: McpScope::Global,
            project_id: None,
            auth_kind: McpAuthKind::None,
            secret_ref: None,
            account_id: None,
            enabled: true,
            timeout_ms: 30_000,
            max_retries: 2,
        }
    }

    fn table_exists(conn: &Connection, name: &str) -> bool {
        conn.query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [name],
            |_| Ok(()),
        )
        .is_ok()
    }

    // ---- schema v3 (fresh DB) ----

    #[test]
    fn fresh_db_is_schema_v4_with_all_nine_s_ext_tables() {
        let db = new_db();
        let version: i64 = db
            .conn()
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        // S-IDEA spec §4 bumped SCHEMA_VERSION 3->4 (additive, `research_run` only); the nine
        // S-EXT tables this test checks for are unaffected — still created by `migrate_v3`, which
        // `migrate_v4` builds on top of, never replaces.
        assert_eq!(version, 4);
        for table in [
            "mcp_server",
            "mcp_tool",
            "account",
            "mcp_invocation",
            "mcp_artifact",
            "skill",
            "consent_grant",
            "policy",
            "audit_log",
        ] {
            assert!(table_exists(db.conn(), table), "missing table {table}");
        }
    }

    // ---- add_mcp_server ----

    #[test]
    fn add_http_global_server_round_trips() {
        let db = new_db();
        let new = http_server("Prowl", McpScope::Global, None);
        let row = db.add_mcp_server(new).unwrap();
        assert_eq!(row.name, "Prowl");
        assert_eq!(row.transport, McpTransport::Http);
        assert_eq!(row.url.as_deref(), Some("https://example.com/mcp"));
        assert_eq!(row.scope, McpScope::Global);
        assert_eq!(row.project_id, None);
        assert_eq!(row.auth_kind, McpAuthKind::None);
        assert!(row.enabled);
        assert_eq!(row.timeout_ms, 30_000);
        assert_eq!(row.max_retries, 2);
        assert_eq!(row.protocol_version, None);
        assert!(!row.id.is_empty());
        assert_eq!(row.created_at, row.updated_at);

        let fetched = db.list_mcp_servers(None).unwrap();
        assert_eq!(fetched.len(), 1);
        assert_eq!(fetched[0], row);
    }

    #[test]
    fn add_stdio_server_round_trips_args_and_env() {
        let db = new_db();
        let row = db.add_mcp_server(stdio_server("Local FS")).unwrap();
        assert_eq!(row.transport, McpTransport::Stdio);
        assert_eq!(row.url, None);
        assert_eq!(
            row.command.as_deref(),
            Some("/usr/local/bin/some-mcp-server")
        );
        assert_eq!(row.args, vec!["--flag".to_string()]);
        assert_eq!(row.env.get("KEY").map(String::as_str), Some("value"));
    }

    #[test]
    fn add_server_scope_project_without_project_id_is_validation() {
        let db = new_db();
        let mut new = http_server("Bad", McpScope::Project, None);
        new.project_id = None;
        let err = db.add_mcp_server(new).unwrap_err();
        assert!(matches!(err, OrchdPersistError::Validation(_)), "{err:?}");
    }

    #[test]
    fn add_server_scope_global_with_project_id_is_validation() {
        let db = new_db();
        let project_id = new_project(&db);
        let mut new = http_server("Bad", McpScope::Global, None);
        new.project_id = Some(project_id);
        let err = db.add_mcp_server(new).unwrap_err();
        assert!(matches!(err, OrchdPersistError::Validation(_)), "{err:?}");
    }

    #[test]
    fn add_server_transport_http_without_url_is_validation() {
        let db = new_db();
        let mut new = http_server("Bad", McpScope::Global, None);
        new.url = None;
        let err = db.add_mcp_server(new).unwrap_err();
        assert!(matches!(err, OrchdPersistError::Validation(_)), "{err:?}");
    }

    #[test]
    fn add_server_transport_stdio_with_url_is_validation() {
        let db = new_db();
        let mut new = stdio_server("Bad");
        new.url = Some("https://example.com".to_string());
        let err = db.add_mcp_server(new).unwrap_err();
        assert!(matches!(err, OrchdPersistError::Validation(_)), "{err:?}");
    }

    // ---- get_mcp_server ----

    #[test]
    fn get_mcp_server_round_trips() {
        let db = new_db();
        let row = db
            .add_mcp_server(http_server("Prowl", McpScope::Global, None))
            .unwrap();
        assert_eq!(db.get_mcp_server(&row.id).unwrap(), row);
    }

    #[test]
    fn get_mcp_server_unknown_id_is_not_found() {
        let db = new_db();
        let err = db.get_mcp_server("missing").unwrap_err();
        assert!(matches!(err, OrchdPersistError::NotFound), "{err:?}");
    }

    // ---- list_mcp_servers ----

    #[test]
    fn list_mcp_servers_returns_global_plus_own_project_not_other_projects() {
        let db = new_db();
        let project_a = new_project(&db);
        let project_b = new_project(&db);

        let global = db
            .add_mcp_server(http_server("Global", McpScope::Global, None))
            .unwrap();
        let a_server = db
            .add_mcp_server(http_server(
                "A-only",
                McpScope::Project,
                Some(project_a.clone()),
            ))
            .unwrap();
        let _b_server = db
            .add_mcp_server(http_server(
                "B-only",
                McpScope::Project,
                Some(project_b.clone()),
            ))
            .unwrap();

        let for_a = db.list_mcp_servers(Some(&project_a)).unwrap();
        let ids: Vec<&str> = for_a.iter().map(|r| r.id.as_str()).collect();
        assert!(ids.contains(&global.id.as_str()));
        assert!(ids.contains(&a_server.id.as_str()));
        assert_eq!(for_a.len(), 2, "must not include project B's server");

        let none_ctx = db.list_mcp_servers(None).unwrap();
        assert_eq!(
            none_ctx.len(),
            1,
            "no project context ⇒ global-scope servers only"
        );
        assert_eq!(none_ctx[0].id, global.id);
    }

    // ---- update_mcp_server ----

    #[test]
    fn update_mcp_server_patches_only_provided_fields() {
        let db = new_db();
        let row = db
            .add_mcp_server(http_server("Prowl", McpScope::Global, None))
            .unwrap();
        let patch = McpServerPatch {
            name: Some("Prowl Renamed".to_string()),
            timeout_ms: Some(60_000),
            ..Default::default()
        };
        let updated = db.update_mcp_server(&row.id, patch).unwrap();
        assert_eq!(updated.name, "Prowl Renamed");
        assert_eq!(updated.timeout_ms, 60_000);
        // untouched fields survive as-is
        assert_eq!(updated.url, row.url);
        assert_eq!(updated.max_retries, row.max_retries);
        assert!(updated.updated_at >= row.updated_at);
    }

    #[test]
    fn update_mcp_server_url_on_stdio_server_is_validation() {
        let db = new_db();
        let row = db.add_mcp_server(stdio_server("Local")).unwrap();
        let patch = McpServerPatch {
            url: Some("https://example.com".to_string()),
            ..Default::default()
        };
        let err = db.update_mcp_server(&row.id, patch).unwrap_err();
        assert!(matches!(err, OrchdPersistError::Validation(_)), "{err:?}");
    }

    #[test]
    fn update_mcp_server_unknown_id_is_not_found() {
        let db = new_db();
        let err = db
            .update_mcp_server("missing", McpServerPatch::default())
            .unwrap_err();
        assert!(matches!(err, OrchdPersistError::NotFound), "{err:?}");
    }

    // ---- set_mcp_server_enabled ----

    #[test]
    fn set_mcp_server_enabled_flips() {
        let db = new_db();
        let row = db
            .add_mcp_server(http_server("Prowl", McpScope::Global, None))
            .unwrap();
        assert!(row.enabled);

        let disabled = db.set_mcp_server_enabled(&row.id, false).unwrap();
        assert!(!disabled.enabled);

        let reenabled = db.set_mcp_server_enabled(&row.id, true).unwrap();
        assert!(reenabled.enabled);
    }

    #[test]
    fn set_mcp_server_enabled_unknown_id_is_not_found() {
        let db = new_db();
        let err = db.set_mcp_server_enabled("missing", true).unwrap_err();
        assert!(matches!(err, OrchdPersistError::NotFound), "{err:?}");
    }

    // ---- set_mcp_server_secret_ref ----

    #[test]
    fn set_mcp_server_secret_ref_persists() {
        let db = new_db();
        let mut new = http_server("Prowl", McpScope::Global, None);
        new.auth_kind = McpAuthKind::Bearer;
        let row = db.add_mcp_server(new).unwrap();
        assert_eq!(row.secret_ref, None);

        let updated = db
            .set_mcp_server_secret_ref(&row.id, "keychain-account-key-1")
            .unwrap();
        assert_eq!(
            updated.secret_ref.as_deref(),
            Some("keychain-account-key-1")
        );
    }

    // ---- delete_mcp_server (cascades tools) ----

    #[test]
    fn delete_mcp_server_cascades_its_tools() {
        let db = new_db();
        let row = db
            .add_mcp_server(http_server("Prowl", McpScope::Global, None))
            .unwrap();
        db.upsert_mcp_tools(
            &row.id,
            vec![NewMcpTool {
                name: "search".to_string(),
                title: None,
                description: None,
                input_schema_json: "{}".to_string(),
            }],
        )
        .unwrap();
        assert_eq!(db.list_mcp_tools(&row.id).unwrap().len(), 1);

        db.delete_mcp_server(&row.id).unwrap();

        assert!(db.list_mcp_servers(None).unwrap().is_empty());
        assert!(
            db.list_mcp_tools(&row.id).unwrap().is_empty(),
            "cascaded tools must be gone"
        );
    }

    #[test]
    fn delete_mcp_server_unknown_id_is_not_found() {
        let db = new_db();
        let err = db.delete_mcp_server("missing").unwrap_err();
        assert!(matches!(err, OrchdPersistError::NotFound), "{err:?}");
    }

    // ---- upsert_mcp_tools / list_mcp_tools / set_mcp_tool_enabled / get_mcp_tool ----

    #[test]
    fn upsert_mcp_tools_replaces_the_cached_set() {
        let db = new_db();
        let row = db
            .add_mcp_server(http_server("Prowl", McpScope::Global, None))
            .unwrap();

        db.upsert_mcp_tools(
            &row.id,
            vec![
                NewMcpTool {
                    name: "search".to_string(),
                    title: Some("Search".to_string()),
                    description: None,
                    input_schema_json: "{}".to_string(),
                },
                NewMcpTool {
                    name: "fetch".to_string(),
                    title: None,
                    description: None,
                    input_schema_json: "{}".to_string(),
                },
            ],
        )
        .unwrap();
        let first = db.list_mcp_tools(&row.id).unwrap();
        assert_eq!(first.len(), 2);
        let fetch_tool_id = first.iter().find(|t| t.name == "fetch").unwrap().id.clone();

        // disable one, then upsert a DIFFERENT tool set — the disabled state must not survive
        // (spec §4: "default on-fetch"), and the removed tool ("fetch") must be gone.
        db.set_mcp_tool_enabled(&fetch_tool_id, false).unwrap();
        db.upsert_mcp_tools(
            &row.id,
            vec![NewMcpTool {
                name: "search".to_string(),
                title: Some("Search v2".to_string()),
                description: Some("desc".to_string()),
                input_schema_json: "{\"type\":\"object\"}".to_string(),
            }],
        )
        .unwrap();

        let second = db.list_mcp_tools(&row.id).unwrap();
        assert_eq!(second.len(), 1, "second upsert must replace, not append");
        assert_eq!(second[0].name, "search");
        assert_eq!(second[0].title.as_deref(), Some("Search v2"));
        assert!(second[0].enabled, "replaced tool defaults to enabled=1");
        assert!(
            db.get_mcp_tool(&fetch_tool_id).unwrap().is_none(),
            "the removed 'fetch' tool row must be gone"
        );
    }

    #[test]
    fn upsert_mcp_tools_unknown_server_is_not_found() {
        let db = new_db();
        let err = db.upsert_mcp_tools("missing", vec![]).unwrap_err();
        assert!(matches!(err, OrchdPersistError::NotFound), "{err:?}");
    }

    #[test]
    fn set_mcp_tool_enabled_flips() {
        let db = new_db();
        let row = db
            .add_mcp_server(http_server("Prowl", McpScope::Global, None))
            .unwrap();
        db.upsert_mcp_tools(
            &row.id,
            vec![NewMcpTool {
                name: "search".to_string(),
                title: None,
                description: None,
                input_schema_json: "{}".to_string(),
            }],
        )
        .unwrap();
        let tool = db.list_mcp_tools(&row.id).unwrap().remove(0);
        assert!(tool.enabled);

        let disabled = db.set_mcp_tool_enabled(&tool.id, false).unwrap();
        assert!(!disabled.enabled);
        assert!(!db.get_mcp_tool(&tool.id).unwrap().unwrap().enabled);

        let reenabled = db.set_mcp_tool_enabled(&tool.id, true).unwrap();
        assert!(reenabled.enabled);
    }

    #[test]
    fn set_mcp_tool_enabled_unknown_id_is_not_found() {
        let db = new_db();
        let err = db.set_mcp_tool_enabled("missing", true).unwrap_err();
        assert!(matches!(err, OrchdPersistError::NotFound), "{err:?}");
    }

    #[test]
    fn get_mcp_tool_unknown_id_is_none() {
        let db = new_db();
        assert!(db.get_mcp_tool("missing").unwrap().is_none());
    }
}
