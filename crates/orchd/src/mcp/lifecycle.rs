//! MCP connect lifecycle (S-EXT spec §6, D9/D10, task T5): the trust-gated entry point that
//! establishes a session, fetches the server's tool list, and caches it into `mcp_tool` (T2's
//! `Db::upsert_mcp_tools`, via [`super::cache::to_new_tools`]).
//!
//! Does NOT emit the `McpToolsChanged` push itself — spec: "Push `McpToolsChanged` is emitted by
//! the dispatch layer (T6), not here."

use std::future::Future;

use bpa_orchd_proto::McpConnectReport;

use super::{cache, resolve_bearer, McpServerRow, OrchdMcpError, ToolCaller};
use crate::persistence::Db;
use crate::trust::{self, Action, Decision};

/// Connect to `server_id`'s MCP server (spec D10: trust-gated on a persisted `consent_grant`,
/// `kind='connect'`). On success, returns the negotiated protocol version + the number of tools
/// just cached.
///
/// `connect_fn` builds the live [`ToolCaller`] session: production callers pass
/// [`super::connect_session`]; tests inject a fake factory so this module needs no
/// network/rmcp. Called at most once, after authorization succeeds (never before — a denied
/// connect must never touch the network).
pub async fn connect<F, Fut, S>(
    db: &Db,
    server_id: &str,
    connect_fn: F,
) -> Result<McpConnectReport, OrchdMcpError>
where
    F: FnOnce(McpServerRow, Option<String>) -> Fut,
    Fut: Future<Output = Result<S, bpa_mcp::McpError>>,
    S: ToolCaller,
{
    let server = db.get_mcp_server(server_id)?;

    // Phase 1 ships HTTP only (spec D6): the fingerprint is the server's URL, matching spec D10
    // ("fingerprint = url (http) ... at grant time"). `add_mcp_server`'s own CHECK invariant
    // guarantees `url` is `Some` whenever `transport='http'`, so this never silently fingerprints
    // an empty string for a real http server.
    let fingerprint = server.url.clone().unwrap_or_default();
    let decision = trust::authorize(
        db,
        &Action::Connect {
            server_id: server.id.clone(),
            fingerprint,
        },
    )?;
    if matches!(decision, Decision::Deny { .. }) {
        return Err(OrchdMcpError::ConsentRequired);
    }

    let bearer = resolve_bearer(&server).map_err(|e| OrchdMcpError::Secret(e.to_string()))?;
    let server_id_owned = server.id.clone();

    let session = connect_fn(server, bearer)
        .await
        .map_err(OrchdMcpError::Mcp)?;

    let tools = session.list_tools().await.map_err(OrchdMcpError::Mcp)?;
    let protocol_version = session.protocol_version();
    let tool_count = tools.len() as i64;

    db.upsert_mcp_tools(&server_id_owned, cache::to_new_tools(&tools))?;

    tracing::info!(
        server_id = %server_id_owned,
        tool_count,
        protocol_version = %protocol_version,
        "mcp: connected"
    );

    Ok(McpConnectReport {
        protocol_version,
        tool_count,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;
    use std::sync::Arc;

    use bpa_mcp::{McpError, McpTool};
    use serde_json::json;

    use super::*;
    use crate::mcp::test_support::FakeSession;
    use crate::mcp::{McpAuthKind, McpScope, McpServerPatch, McpTransport, NewMcpServer};

    fn new_db() -> Db {
        Db::open_in_memory().unwrap()
    }

    fn add_server(db: &Db) -> McpServerRow {
        db.add_mcp_server(NewMcpServer {
            name: "Prowl".to_string(),
            transport: McpTransport::Http,
            url: Some("https://example.com/mcp".to_string()),
            command: None,
            args: vec![],
            env: Default::default(),
            scope: McpScope::Global,
            project_id: None,
            auth_kind: McpAuthKind::None,
            secret_ref: None,
            account_id: None,
            enabled: true,
            timeout_ms: 5_000,
            max_retries: 2,
        })
        .unwrap()
    }

    #[tokio::test]
    async fn connect_without_consent_is_denied_and_never_touches_the_network() {
        let db = new_db();
        let server = add_server(&db);

        let touched = Arc::new(AtomicUsize::new(0));
        let touched_for_closure = touched.clone();
        let connect_fn = move |_server: McpServerRow, _bearer: Option<String>| {
            touched_for_closure.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            async move {
                Ok::<FakeSession, McpError>(FakeSession::new(vec![], Arc::new(AtomicUsize::new(0))))
            }
        };

        let err = connect(&db, &server.id, connect_fn).await.unwrap_err();
        assert!(matches!(err, OrchdMcpError::ConsentRequired));

        assert!(db.list_mcp_tools(&server.id).unwrap().is_empty());
        assert_eq!(
            touched.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "a denied connect must never call connect_fn"
        );

        let denied: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM audit_log WHERE action='connect' AND decision='deny' \
                 AND reason='consent_required'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(denied, 1);
    }

    #[tokio::test]
    async fn connect_after_url_change_is_denied_and_bearer_is_never_sent() {
        // The credential-exfil path (task-5 review): consent granted for url A, server repointed
        // to url B, then a connect attempt. connect_fn (which would resolve+send the bearer) must
        // NOT be invoked — the stored bearer never reaches url B.
        let db = new_db();
        let server = add_server(&db);
        db.grant_consent(&server.id, "connect", &server.url.clone().unwrap())
            .unwrap();
        db.update_mcp_server(
            &server.id,
            McpServerPatch {
                url: Some("https://evil.example.com/mcp".to_string()),
                ..Default::default()
            },
        )
        .unwrap();

        let touched = Arc::new(AtomicUsize::new(0));
        let touched_for_closure = touched.clone();
        let connect_fn = move |_server: McpServerRow, _bearer: Option<String>| {
            touched_for_closure.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            async move {
                Ok::<FakeSession, McpError>(FakeSession::new(vec![], Arc::new(AtomicUsize::new(0))))
            }
        };

        let err = connect(&db, &server.id, connect_fn).await.unwrap_err();
        assert!(matches!(err, OrchdMcpError::ConsentRequired));
        assert_eq!(
            touched.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "connect_fn must not run: the bearer must never be sent to the repointed url"
        );
    }

    #[tokio::test]
    async fn connect_after_consent_caches_tools_and_returns_report() {
        let db = new_db();
        let server = add_server(&db);
        db.grant_consent(&server.id, "connect", &server.url.clone().unwrap())
            .unwrap();

        let tool = McpTool {
            name: "search".to_string(),
            title: Some("Search".to_string()),
            description: Some("Full text search".to_string()),
            input_schema: json!({"type": "object"}),
        };
        let connect_fn = move |_server: McpServerRow, _bearer: Option<String>| async move {
            Ok::<FakeSession, McpError>(FakeSession::new(vec![tool], Arc::new(AtomicUsize::new(0))))
        };

        let report = connect(&db, &server.id, connect_fn).await.unwrap();

        assert_eq!(report.tool_count, 1);
        assert_eq!(report.protocol_version, "2025-11-25");

        let tools = db.list_mcp_tools(&server.id).unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "search");
        assert_eq!(tools[0].title.as_deref(), Some("Search"));
        assert!(tools[0].enabled, "freshly cached tools default to enabled");

        let allowed: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM audit_log WHERE action='connect' AND decision='allow'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(allowed, 1);
    }

    #[tokio::test]
    async fn connect_replaces_a_previously_cached_tool_set() {
        let db = new_db();
        let server = add_server(&db);
        db.grant_consent(&server.id, "connect", &server.url.clone().unwrap())
            .unwrap();
        // pre-seed a stale cached tool that the fresh connect must replace, not append to.
        db.upsert_mcp_tools(
            &server.id,
            vec![crate::mcp::NewMcpTool {
                name: "stale".to_string(),
                title: None,
                description: None,
                input_schema_json: "{}".to_string(),
            }],
        )
        .unwrap();

        let connect_fn = move |_server: McpServerRow, _bearer: Option<String>| async move {
            Ok::<FakeSession, McpError>(FakeSession::new(vec![], Arc::new(AtomicUsize::new(0))))
        };

        let report = connect(&db, &server.id, connect_fn).await.unwrap();
        assert_eq!(report.tool_count, 0);
        assert!(db.list_mcp_tools(&server.id).unwrap().is_empty());
    }
}
