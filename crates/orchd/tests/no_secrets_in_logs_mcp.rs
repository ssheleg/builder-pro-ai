//! No-secrets-in-logs test for the MCP bearer surface (S-EXT spec §9: "no-secrets-in-logs for
//! MCP/connector surface (planted bearer/token never in logs)"; task-9 brief's coverage/security
//! gap check — T5/T6's own bearer-leak coverage was flagged "synthetic, not e2e" (task-5
//! report)). Mirrors `no_secrets_in_logs.rs`/`no_secrets_in_logs_graph.rs`'s shape (plant a
//! secret, drive real code against a real tracing log sink, assert it's absent) but the secret
//! here is a REAL Keychain-backed MCP bearer token, planted via the exact same two-step
//! Keychain-then-DB write `socket_server::dispatch`'s `McpSetServerBearer` arm performs
//! (`bpa_secrets::set` + `Db::set_mcp_server_secret_ref` + `auth_kind = Bearer`), then driven
//! through the REAL `mcp::lifecycle::connect` + `mcp::invoke::call_tool` production paths
//! (`mcp::connect_session`, the exact factory `socket_server` passes in production) against a
//! real loopback rmcp Streamable-HTTP stub server.
//!
//! Non-vacuous by construction (T6 review-fix precedent: "mutation-verified, not just structural
//! shape"): the stub server's router is wrapped in a small middleware layer that CAPTURES the
//! `Authorization` header off the real HTTP request, and this test asserts that captured value
//! equals `Bearer <planted token>` — i.e. it proves the bearer genuinely flowed out of Keychain,
//! through `resolve_bearer`, through `bpa_mcp::connect`'s `auth_header`, and onto the wire, at the
//! exact same time it asserts the same token text is nowhere in the daemon's own log output. A
//! broken `resolve_bearer` (returns `None`/wrong value) would fail the header assertion, not
//! silently pass either check.
//!
//! Keychain-touching (unlike `no_secrets_in_logs.rs`/`no_secrets_in_logs_graph.rs`, which are
//! Keychain-free): probes the login keychain first and skips (loud `eprintln!`, never a silent
//! vacuous pass) when unavailable, mirroring `bpa-secrets`'s own test-suite convention (task T1,
//! `crates/secrets/src/lib.rs`'s `tests::keychain_available`) — that helper is `#[cfg(test)]`-
//! private to its own crate, so this file carries its own equivalent probe rather than reaching
//! into `bpa_secrets`'s internals.

use std::fs;
use std::io::Read;
use std::sync::{Arc, Mutex as StdMutex};

use axum::extract::{Request, State};
use axum::middleware::{self, Next};
use axum::response::Response;
use bpa_orchd::mcp::{self, McpAuthKind, McpScope, McpServerPatch, McpTransport, NewMcpServer};
use bpa_orchd::persistence::Db;
use tokio::sync::Mutex as TokioMutex;

#[derive(Debug, serde::Deserialize, rmcp::schemars::JsonSchema)]
struct EchoRequest {
    msg: String,
}

/// Trivial stub MCP server — a single `echo` tool. Mirrors `dispatch_integration.rs`'s
/// `EchoServer` (same macro shape); duplicated here rather than shared because Rust integration-
/// test files (`tests/*.rs`) are each their own separate crate with no cross-file visibility.
#[derive(Debug, Clone, Copy, Default)]
struct EchoServer;

#[rmcp::tool_router(server_handler)]
impl EchoServer {
    #[rmcp::tool(description = "Echo the given message back")]
    fn echo(
        &self,
        rmcp::handler::server::wrapper::Parameters(EchoRequest { msg }): rmcp::handler::server::wrapper::Parameters<
            EchoRequest,
        >,
    ) -> String {
        msg
    }
}

/// Captures the `Authorization` header off every HTTP request the stub server receives, so the
/// test below can assert the real value that hit the wire (not just that the call "succeeded").
async fn capture_auth_header(
    State(captured): State<Arc<StdMutex<Option<String>>>>,
    request: Request,
    next: Next,
) -> Response {
    if let Some(value) = request.headers().get(axum::http::header::AUTHORIZATION) {
        if let Ok(s) = value.to_str() {
            *captured.lock().unwrap() = Some(s.to_string());
        }
    }
    next.run(request).await
}

/// Spawns [`EchoServer`] on a loopback TCP port (same `axum`/`rmcp` wiring as
/// `dispatch_integration.rs::spawn_stub_mcp_server`), wrapped in [`capture_auth_header`]. Returns
/// the server's base url plus the shared captured-header slot.
async fn spawn_stub_mcp_server_capturing_auth() -> (String, Arc<StdMutex<Option<String>>>) {
    let session_manager = std::sync::Arc::new(
        rmcp::transport::streamable_http_server::session::local::LocalSessionManager::default(),
    );
    let service = rmcp::transport::streamable_http_server::StreamableHttpService::new(
        || Ok(EchoServer),
        session_manager,
        rmcp::transport::streamable_http_server::StreamableHttpServerConfig::default(),
    );
    let captured_auth: Arc<StdMutex<Option<String>>> = Arc::new(StdMutex::new(None));
    let router =
        axum::Router::new()
            .nest_service("/mcp", service)
            .layer(middleware::from_fn_with_state(
                captured_auth.clone(),
                capture_auth_header,
            ));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind loopback stub mcp server");
    let addr = listener.local_addr().expect("stub mcp server local_addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    (format!("http://{addr}/mcp"), captured_auth)
}

/// `OSStatus` codes meaning "no usable Keychain in this session" (mirrors `bpa-secrets`'s own
/// `tests::KEYCHAIN_UNAVAILABLE_CODES` doc — that constant is private to its crate, so this is a
/// deliberately looser but equally honest equivalent: `bpa_secrets::set`'s ONLY realistic failure
/// mode in a working wrapper is environment unavailability, so any `set` failure here is treated
/// as "skip", loudly, never silently).
fn keychain_available() -> bool {
    let probe = bpa_secrets::mcp_bearer_ref("no-secrets-in-logs-mcp-probe");
    match bpa_secrets::set(&probe, b"probe") {
        Ok(()) => {
            let _ = bpa_secrets::delete(&probe);
            true
        }
        Err(e) => {
            eprintln!(
                "SKIP no_secrets_in_logs_mcp: login keychain unavailable in this environment \
                 ({e}) — graceful skip, not a pass. Run locally with an unlocked login keychain \
                 (or on CI's macOS runner) to exercise the full assertion."
            );
            false
        }
    }
}

/// Best-effort Keychain teardown so a panic mid-test never leaves a stray real Keychain entry.
struct DeleteBearerOnDrop<'a>(&'a str);
impl Drop for DeleteBearerOnDrop<'_> {
    fn drop(&mut self) {
        let _ = bpa_secrets::delete(&bpa_secrets::mcp_bearer_ref(self.0));
    }
}

#[tokio::test]
async fn planted_mcp_bearer_never_appears_in_logs_but_genuinely_reaches_the_server() {
    if !keychain_available() {
        return;
    }

    let bearer_secret = "s3cr3t-MCP-BEARER-must-not-leak-9f21a4";

    let tmp = tempfile::tempdir().expect("tempdir");
    let log_path = tmp.path().join("orchd.test.log");
    let db_path = tmp.path().join("orchd.db");

    bpa_daemon_core::logging::init_to_file(&log_path).expect("init logging");

    let db: Arc<TokioMutex<Db>> = Arc::new(TokioMutex::new(Db::open(&db_path).expect("open db")));

    let (stub_url, captured_auth) = spawn_stub_mcp_server_capturing_auth().await;

    let server = {
        let guard = db.lock().await;
        guard
            .add_mcp_server(NewMcpServer {
                name: "Bearer Test Server".to_string(),
                transport: McpTransport::Http,
                url: Some(stub_url.clone()),
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
                max_retries: 1,
            })
            .expect("add mcp server")
    };
    // Teardown MUST reference the server id assigned above (Keychain account = server id, spec
    // §5's `mcp_bearer_ref`) — declared right after the id exists so a panic on any later line
    // still cleans up the real Keychain entry.
    let _cleanup = DeleteBearerOnDrop(&server.id);

    // ---- exact `McpSetServerBearer` two-step (socket_server.rs): Keychain write, THEN the DB
    // secret_ref + auth_kind patch ----
    bpa_secrets::set(
        &bpa_secrets::mcp_bearer_ref(&server.id),
        bearer_secret.as_bytes(),
    )
    .expect("plant bearer in keychain");
    {
        let guard = db.lock().await;
        guard
            .set_mcp_server_secret_ref(&server.id, &server.id)
            .expect("set secret_ref");
        guard
            .update_mcp_server(
                &server.id,
                McpServerPatch {
                    auth_kind: Some(McpAuthKind::Bearer),
                    ..Default::default()
                },
            )
            .expect("patch auth_kind to bearer");
    }

    {
        let guard = db.lock().await;
        guard
            .grant_consent(&server.id, "connect", &stub_url)
            .expect("grant connect consent");
    }

    // ---- drive the REAL connect + call_tool production paths (resolve_bearer -> bpa_mcp::connect
    // -> real HTTP request carrying the bearer) ----
    let report = mcp::lifecycle::connect(&db, &server.id, mcp::connect_session)
        .await
        .expect("connect must succeed with a valid bearer");
    assert!(report.tool_count >= 1, "stub advertises the echo tool");

    let args_json = serde_json::json!({"msg": "hi"}).to_string();
    let result = mcp::invoke::call_tool(
        &db,
        &server.id,
        "echo",
        &args_json,
        None,
        mcp::connect_session,
    )
    .await
    .expect("call_tool must succeed with a valid bearer");
    assert!(!result.is_error);

    // ---- mutation-verified: the bearer genuinely reached the wire ----
    let expected_header = format!("Bearer {bearer_secret}");
    assert_eq!(
        captured_auth.lock().unwrap().as_deref(),
        Some(expected_header.as_str()),
        "the stub server's Authorization header must carry the planted bearer — a broken \
         resolve_bearer (None/wrong value) must fail THIS assertion, not silently pass"
    );

    bpa_daemon_core::logging::flush();
    let mut contents = String::new();
    fs::File::open(&log_path)
        .expect("open log")
        .read_to_string(&mut contents)
        .expect("read log");

    assert!(
        !contents.trim().is_empty(),
        "log sink was empty — test would pass vacuously"
    );
    assert!(
        !contents.contains(bearer_secret),
        "planted MCP bearer token leaked into logs:\n{contents}"
    );
    assert!(
        !contents.contains(&expected_header),
        "planted MCP bearer token (as the literal Authorization header value) leaked into \
         logs:\n{contents}"
    );
}
