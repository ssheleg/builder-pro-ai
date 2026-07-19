//! MCP connect lifecycle (S-EXT spec §6, D9/D10, task T5): the trust-gated entry point that
//! establishes a session, fetches the server's tool list, and caches it into `mcp_tool` (T2's
//! `Db::upsert_mcp_tools`, via [`super::cache::to_new_tools`]).
//!
//! Does NOT emit the `McpToolsChanged` push itself — spec: "Push `McpToolsChanged` is emitted by
//! the dispatch layer (T6), not here."

use std::future::Future;
use std::sync::Arc;

use bpa_orchd_proto::McpConnectReport;
use tokio::sync::Mutex;

use super::{cache, resolve_bearer, McpServerRow, OrchdMcpError, ToolCaller};
use crate::persistence::Db;
use crate::trust::{self, Decision};

/// Connect to `server_id`'s MCP server (spec D10: trust-gated on a persisted `consent_grant`,
/// `kind='connect'`). On success, returns the negotiated protocol version + the number of tools
/// just cached.
///
/// `connect_fn` builds the live [`ToolCaller`] session: production callers pass
/// [`super::connect_session`]; tests inject a fake factory so this module needs no
/// network/rmcp. Called at most once, after authorization succeeds (never before — a denied
/// connect must never touch the network).
///
/// Takes the SHARED `Arc<Mutex<Db>>` (the exact type `socket_server::ServerDeps.db` holds) rather
/// than a bare `&Db`, and locks it TWICE — once to read (phase 1) and once to write (phase 3) —
/// with the network round-trip (phase 2) sandwiched BETWEEN, holding NO `Db` guard (T6 review
/// fix, S-EXT §6). Holding the single daemon DB mutex across `connect_fn`/`list_tools`'s awaits
/// would stall EVERY other orchd connection's DB op for the whole MCP round-trip (up to
/// `(1+max_retries)×timeout` — a self-inflicted DoS driven by a third-party server's latency),
/// and was also the sole reason `Db` previously needed a hand-written `Sync` bound (a `&Db`
/// captured across an `.await` makes the spawned per-connection task `!Send`). Phasing the lock
/// out fixes both: no `Db` reference is alive across any suspension point, so the future is `Send`
/// with `Db` staying the plain `Send + !Sync` it always was.
pub async fn connect<F, Fut, S>(
    db: &Arc<Mutex<Db>>,
    server_id: &str,
    connect_fn: F,
) -> Result<McpConnectReport, OrchdMcpError>
where
    F: FnOnce(McpServerRow, Option<String>) -> Fut,
    Fut: Future<Output = Result<S, bpa_mcp::McpError>>,
    S: ToolCaller,
{
    // ---- Phase 1: lock -> read. The guard is dropped at the end of this block, BEFORE any
    // network await — a denied connect returns here, never touching the network. ----
    let (server, bearer) = {
        let guard = db.lock().await;
        let server = guard.get_mcp_server(server_id)?;

        // http -> the pre-existing `connect`/URL-fingerprint gate (unchanged, spec D10); stdio ->
        // the distinct `stdio_exec` process-spawn gate (spec §6/D6/D10, task T16, closes BL-22).
        // `super::connect_action` is the SAME helper `invoke::call_tool` uses for its own
        // per-call reconnect, so a stdio spawn can never bypass this gate via either path.
        let action = super::connect_action(&server);
        let decision = trust::authorize(&guard, &action)?;
        if matches!(decision, Decision::Deny { .. }) {
            return Err(OrchdMcpError::ConsentRequired);
        }

        let bearer = resolve_bearer(&server).map_err(|e| OrchdMcpError::Secret(e.to_string()))?;
        (server, bearer)
    };
    let server_id_owned = server.id.clone();
    // (BL-89, spec D5): bound BOTH the connect/`initialize` handshake and the follow-up
    // `list_tools` by `server.timeout_ms` — mirrors `invoke::call_tool`'s D12 wrap. Without this,
    // the explicit `McpConnect` verb hangs this connection's dispatch task forever on a peer that
    // accepts the socket but never answers (dead peer, silent firewall drop, overloaded stdio
    // child), and since the app holds a single shared orchd connection that would wedge the whole
    // pipeline. An elapsed handshake maps to the SAME `McpError::Timeout` the `tools/call` path
    // produces.
    let timeout = super::effective_timeout(server.timeout_ms);

    // ---- Phase 2: network. NO `Db`/`MutexGuard` reference is alive here. ----
    let session = match tokio::time::timeout(timeout, connect_fn(server, bearer)).await {
        Ok(result) => result,
        Err(_elapsed) => Err(bpa_mcp::McpError::Timeout),
    }
    .map_err(OrchdMcpError::Mcp)?;

    let tools = match tokio::time::timeout(timeout, session.list_tools()).await {
        Ok(result) => result,
        Err(_elapsed) => Err(bpa_mcp::McpError::Timeout),
    }
    .map_err(OrchdMcpError::Mcp)?;
    let protocol_version = session.protocol_version();
    let tool_count = tools.len() as i64;

    // ---- Phase 3: lock -> write. Guard dropped at the end of this block. ----
    {
        let guard = db.lock().await;
        guard.upsert_mcp_tools(&server_id_owned, cache::to_new_tools(&tools))?;
    }

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
    use tokio::sync::Mutex;

    use super::*;
    use crate::mcp::test_support::FakeSession;
    use crate::mcp::{McpAuthKind, McpScope, McpServerPatch, McpTransport, NewMcpServer};

    /// `connect` now takes the SHARED `Arc<Mutex<Db>>` (it locks internally in phases around the
    /// network await — T6 review fix), so tests build one and lock it themselves for setup /
    /// assertions (`db.lock().await.<method>`), exactly as `socket_server::dispatch` does with
    /// `deps.db`.
    fn new_db() -> Arc<Mutex<Db>> {
        Arc::new(Mutex::new(Db::open_in_memory().unwrap()))
    }

    async fn add_server(db: &Arc<Mutex<Db>>) -> McpServerRow {
        db.lock()
            .await
            .add_mcp_server(NewMcpServer {
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
        let server = add_server(&db).await;

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

        assert!(db
            .lock()
            .await
            .list_mcp_tools(&server.id)
            .unwrap()
            .is_empty());
        assert_eq!(
            touched.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "a denied connect must never call connect_fn"
        );

        let denied: i64 = db
            .lock()
            .await
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
        let server = add_server(&db).await;
        {
            let guard = db.lock().await;
            guard
                .grant_consent(&server.id, "connect", &server.url.clone().unwrap())
                .unwrap();
            guard
                .update_mcp_server(
                    &server.id,
                    McpServerPatch {
                        url: Some("https://evil.example.com/mcp".to_string()),
                        ..Default::default()
                    },
                )
                .unwrap();
        }

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
        let server = add_server(&db).await;
        db.lock()
            .await
            .grant_consent(&server.id, "connect", &server.url.clone().unwrap())
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

        let tools = db.lock().await.list_mcp_tools(&server.id).unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "search");
        assert_eq!(tools[0].title.as_deref(), Some("Search"));
        assert!(tools[0].enabled, "freshly cached tools default to enabled");

        let allowed: i64 = db
            .lock()
            .await
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
        let server = add_server(&db).await;
        {
            let guard = db.lock().await;
            guard
                .grant_consent(&server.id, "connect", &server.url.clone().unwrap())
                .unwrap();
            // pre-seed a stale cached tool that the fresh connect must replace, not append to.
            guard
                .upsert_mcp_tools(
                    &server.id,
                    vec![crate::mcp::NewMcpTool {
                        name: "stale".to_string(),
                        title: None,
                        description: None,
                        input_schema_json: "{}".to_string(),
                    }],
                )
                .unwrap();
        }

        let connect_fn = move |_server: McpServerRow, _bearer: Option<String>| async move {
            Ok::<FakeSession, McpError>(FakeSession::new(vec![], Arc::new(AtomicUsize::new(0))))
        };

        let report = connect(&db, &server.id, connect_fn).await.unwrap();
        assert_eq!(report.tool_count, 0);
        assert!(db
            .lock()
            .await
            .list_mcp_tools(&server.id)
            .unwrap()
            .is_empty());
    }

    // ---- stdio-exec consent gate (S-EXT §6/D6/D10, task T16, closes BL-22) ----

    async fn add_stdio_server(db: &Arc<Mutex<Db>>, command: &str) -> McpServerRow {
        db.lock()
            .await
            .add_mcp_server(NewMcpServer {
                name: "local-mcp".to_string(),
                transport: McpTransport::Stdio,
                url: None,
                command: Some(command.to_string()),
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
    async fn stdio_connect_without_stdio_exec_consent_is_denied_and_never_spawns() {
        let db = new_db();
        let server = add_stdio_server(&db, "/nonexistent/mcp-server").await;

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
            "a denied stdio connect must never call connect_fn (never spawn the process)"
        );

        let denied: i64 = db
            .lock()
            .await
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM audit_log WHERE action='stdio_spawn' AND decision='deny' \
                 AND reason='stdio_exec_required'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(denied, 1);
    }

    #[tokio::test]
    async fn stdio_connect_after_stdio_exec_consent_caches_tools_and_spawns() {
        let db = new_db();
        let server = add_stdio_server(&db, "/nonexistent/mcp-server").await;
        let fingerprint = crate::trust::stdio_exec_fingerprint("/nonexistent/mcp-server", &[]);
        db.lock()
            .await
            .grant_consent(&server.id, "stdio_exec", &fingerprint)
            .unwrap();

        let touched = Arc::new(AtomicUsize::new(0));
        let touched_for_closure = touched.clone();
        let tool = McpTool {
            name: "run".to_string(),
            title: None,
            description: None,
            input_schema: json!({"type": "object"}),
        };
        let connect_fn = move |_server: McpServerRow, _bearer: Option<String>| {
            touched_for_closure.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let tool = tool.clone();
            async move {
                Ok::<FakeSession, McpError>(FakeSession::new(
                    vec![tool],
                    Arc::new(AtomicUsize::new(0)),
                ))
            }
        };

        let report = connect(&db, &server.id, connect_fn).await.unwrap();
        assert_eq!(report.tool_count, 1);
        assert_eq!(
            touched.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "a granted stdio connect must call connect_fn exactly once"
        );

        let allowed: i64 = db
            .lock()
            .await
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM audit_log WHERE action='stdio_spawn' AND decision='allow'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(allowed, 1);
    }

    #[tokio::test]
    async fn stdio_connect_after_command_change_denies_and_never_spawns() {
        // Mirrors the http url-repointing exploit test above: consent granted for command A,
        // the server row is repointed to command B, then a connect attempt. connect_fn must NOT
        // run — a stale grant for command A must not authorize spawning command B.
        let db = new_db();
        let server = add_stdio_server(&db, "/bin/original-tool").await;
        {
            let guard = db.lock().await;
            let fp_a = crate::trust::stdio_exec_fingerprint("/bin/original-tool", &[]);
            guard
                .grant_consent(&server.id, "stdio_exec", &fp_a)
                .unwrap();
            guard
                .update_mcp_server(
                    &server.id,
                    McpServerPatch {
                        command: Some("/bin/swapped-tool".to_string()),
                        ..Default::default()
                    },
                )
                .unwrap();
        }

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
            "connect_fn must not run: a grant for command A must not authorize spawning command B"
        );
    }

    // ---- handshake timeout (BL-89, spec D5) ----

    /// An http server with a short `timeout_ms`, so a never-resolving handshake trips the timeout
    /// quickly. Consent is granted separately by the test so the trust gate lets it reach the
    /// network step.
    async fn add_server_with_timeout(db: &Arc<Mutex<Db>>, timeout_ms: i64) -> McpServerRow {
        db.lock()
            .await
            .add_mcp_server(NewMcpServer {
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
                timeout_ms,
                max_retries: 2,
            })
            .unwrap()
    }

    /// A `ToolCaller` whose `list_tools()` never resolves — models a peer that completes the
    /// connect handshake but then goes silent on the tool listing. `call_tool` is unreachable in
    /// the connect path.
    struct NeverListTools;

    impl ToolCaller for NeverListTools {
        async fn list_tools(&self) -> Result<Vec<McpTool>, McpError> {
            std::future::pending().await
        }

        async fn call_tool(
            &self,
            _name: &str,
            _args: serde_json::Value,
        ) -> Result<bpa_mcp::McpToolResult, McpError> {
            unreachable!("call_tool is not exercised by the connect path")
        }

        fn protocol_version(&self) -> String {
            "2025-11-25".to_string()
        }
    }

    #[tokio::test]
    async fn connect_times_out_when_connect_fn_never_resolves() {
        let db = new_db();
        let server = add_server_with_timeout(&db, 50).await;
        db.lock()
            .await
            .grant_consent(&server.id, "connect", &server.url.clone().unwrap())
            .unwrap();

        // The peer accepts the connection but the handshake future never resolves.
        let connect_fn = move |_server: McpServerRow, _bearer: Option<String>| {
            std::future::pending::<Result<FakeSession, McpError>>()
        };

        // Outer guard: a regression that drops the timeout would hang HERE, not the whole suite.
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            connect(&db, &server.id, connect_fn),
        )
        .await
        .expect("connect must return within the server timeout, not hang");

        assert!(
            matches!(result, Err(OrchdMcpError::Mcp(McpError::Timeout))),
            "a never-resolving connect handshake must map to McpError::Timeout, got {result:?}"
        );
    }

    #[tokio::test]
    async fn connect_times_out_when_list_tools_never_resolves() {
        let db = new_db();
        let server = add_server_with_timeout(&db, 50).await;
        db.lock()
            .await
            .grant_consent(&server.id, "connect", &server.url.clone().unwrap())
            .unwrap();

        // The connect succeeds, but the session's list_tools() never returns.
        let connect_fn = move |_server: McpServerRow, _bearer: Option<String>| async move {
            Ok::<NeverListTools, McpError>(NeverListTools)
        };

        let result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            connect(&db, &server.id, connect_fn),
        )
        .await
        .expect("connect must return within the server timeout, not hang");

        assert!(
            matches!(result, Err(OrchdMcpError::Mcp(McpError::Timeout))),
            "a never-resolving list_tools must map to McpError::Timeout, got {result:?}"
        );
    }
}
