//! Direct-API connector adapter + `ConnectorInvoke` (S-EXT spec §6/§7, D5/D10, task T12).
//!
//! [`ConnectorAdapter`] is the seam a provider-specific "social"/REST integration implements
//! (spec §7: `{provider, list_ops, invoke}`). [`GenericRestAdapter`] is the ONE reference adapter
//! this slice ships (`provider = "generic-rest"`, ops `get`/`post` against an arbitrary
//! caller-supplied URL). [`invoke`] is `ConnectorInvoke`'s implementation — routed through the
//! SAME `crate::trust::authorize` choke-point as `mcp::invoke::call_tool` (spec §6: "passes
//! through `trust::authorize` IDENTICALLY to `McpCallTool`").
//!
//! # Artifact-persistence decision (v1, this task)
//!
//! The spec's `McpCallResult` doc comment says a call's "JSON result ... is already persisted as
//! a durable artifact row" (`crates/orchd-proto/src/lib.rs`), and §9's DoD list says
//! `ConnectorInvoke` "reuses the artifact/invocation path". In practice this is blocked by the
//! frozen v3 schema (spec §4 DDL, `persistence.rs`'s `migrate_v3`, NOT touched by this task):
//! both `mcp_invocation.server_id` AND `mcp_artifact.server_id` are `TEXT NOT NULL REFERENCES
//! mcp_server(id) ON DELETE CASCADE` — a connector invocation has no `mcp_server` row to
//! reference. Two ways around that were considered:
//!
//! 1. **A synthetic `mcp_server` row** representing the connector-adapter provider (e.g.
//!    `transport='http', enabled=0`), satisfying the FK with no schema change. Rejected: THIS
//!    task's own reading of `mcp::registry::list_mcp_servers` shows it has no
//!    enabled/synthetic-row filter — `McpListServers` would return the sentinel row to the
//!    frontend's «Серверы» tab as if it were a real, addable MCP server. Papering over that
//!    would need either a second filter convention (which nothing else in `registry.rs` has) or
//!    UI-side special-casing — both push complexity onto a LATER task for a problem this task
//!    created.
//! 2. **Audit-only (chosen)**: `invoke` below writes NO `mcp_invocation`/`mcp_artifact` row.
//!    The `connector_invoke` `audit_log` row (spec D10, always written — see `crate::trust`) IS
//!    the durable record that the call happened, WHO (`account_id`, in the audit row's
//!    `server_id` column — see `trust::write_audit`'s doc comment), and the authorize verdict.
//!    The returned [`McpCallResult`]'s `content_json` is still exactly the untrusted adapter
//!    result (spec D9's semantics: this data must be treated as untrusted, same as any MCP tool
//!    result), but it is EPHEMERAL — it does NOT survive an orchd restart, and
//!    `McpGetArtifact`/`McpListArtifacts` will never resolve `artifact_id`/`invocation_id` below
//!    (both a fixed, obviously-not-a-uuid [`UNPERSISTED_SENTINEL`], not a freshly-minted id that
//!    could be mistaken for a real row).
//!
//! This is a genuine, documented capability gap versus `McpCallTool` (no invocation-log/artifact
//! history for connector calls yet), not a silent omission — see the task-12 report for the
//! backlog framing (a schema v4 with a connector-shaped invocation/artifact table, or a
//! relaxed/nullable `mcp_server` FK, is the natural follow-up once a second real adapter needs
//! it).

use std::sync::Arc;
use std::time::Duration;

use bpa_orchd_proto::{ConnectorOp, McpCallResult};
use tokio::sync::Mutex as TokioMutex;

use super::accounts::{AccountToken, ConnectorError, ConnectorsState};
use crate::persistence::Db;
use crate::trust::{self, Action, Decision};

/// Sentinel `McpCallResult.artifact_id`/`invocation_id` for a `ConnectorInvoke` result (see this
/// module's doc comment: artifact persistence is deferred, audit-only in v1). Deliberately NOT a
/// uuid-v4-shaped string — `McpGetArtifact{id}`/`McpListInvocations` will never resolve this, and
/// this shape makes that obvious to anyone inspecting a result rather than looking like a real,
/// just-unlucky lookup miss.
const UNPERSISTED_SENTINEL: &str = "unpersisted-v1-connector-invoke";

/// Bounded per-request timeout for [`GenericRestAdapter`] (spec §7: "Bounded timeout ...
/// (honest degradation)"). No per-account/per-op override exists yet — the `account` table has
/// no `timeout_ms` column the way `mcp_server` does (spec §4) — so a fixed constant is the
/// honest v1 shape; promoting this to a configurable field is a natural follow-up once a second
/// adapter needs different bounds.
const GENERIC_REST_TIMEOUT: Duration = Duration::from_secs(30);

// ================================================================================
// ---- ConnectorAdapter trait (spec §7) ----
// ================================================================================

/// Direct-API connector adapter (S-EXT spec §7, D5, task T12): the seam a provider-specific
/// integration implements to expose typed "ops" against an [`AccountToken`]'s resolved bearer,
/// routed through the SAME trust choke-point + untrusted-result contract as an MCP `tools/call`
/// (see [`invoke`]).
///
/// Native `async fn` in trait (stable since Rust 1.75; this workspace pins 1.92), NOT the
/// `async-trait` crate — mirrors `crate::mcp::ToolCaller`'s own documented choice (see that
/// trait's doc comment for the full rationale). This makes `ConnectorAdapter` NOT
/// `dyn`-compatible; [`resolve_adapter`] dispatches on `provider` via a plain `match` rather than
/// a trait-object registry — deliberately, since v1 ships exactly one concrete adapter (spec §7:
/// "One reference adapter ships ... A named social adapter ... is a follow-up backlog item"). A
/// real multi-adapter registry (needed once a second concrete adapter exists) would have to
/// either box every `invoke` future or drop the native-async-fn convention this crate has used
/// consistently since T5 — a call better made when there is a second adapter to design against,
/// not speculatively here.
///
/// `#[allow(async_fn_in_trait)]`: same rationale as `ToolCaller` — this trait is not re-exported
/// outside this crate, so the "implementors can't add `Send` to the returned future" lint concern
/// doesn't apply.
#[allow(async_fn_in_trait)]
pub trait ConnectorAdapter {
    /// The `account.provider` string this adapter answers for (spec §4/§7).
    fn provider(&self) -> &str;
    /// The ops this adapter exposes (`ConnectorListOps` wire verb, spec §5) — T10's
    /// `ConnectorOp {name, description}` proto entity.
    fn list_ops(&self) -> Vec<ConnectorOp>;
    /// Invokes `op` with `args` using `token`'s bearer. Implementations must never log `token`,
    /// `args`, or the result (spec §6: "NEVER contain secrets or tool args").
    async fn invoke(
        &self,
        token: &AccountToken,
        op: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, ConnectorError>;
}

// ================================================================================
// ---- GenericRestAdapter (spec §7's one reference adapter) ----
// ================================================================================

/// The v1 reference [`ConnectorAdapter`] (spec §7): `get`/`post` against an ARBITRARY URL
/// supplied per-call in `args["url"]` — NOT a per-account base URL (the `account` table has no
/// such column, spec §4; the owner names the target on every call) — using the account's bearer
/// as `Authorization: Bearer <token>` (`RequestBuilder::bearer_auth`, spec §7).
///
/// **Trust boundary (spec §7: "SSRF-consideration ... redirect policy can be default").** Unlike
/// `accounts::ssrf_guarded_http_client` (the OAuth token-exchange client, which disables
/// redirects because the target URL is the IdP's own metadata-resolved endpoint — the OWNER never
/// typed it), a `generic-rest` call's target URL IS exactly what the owner typed into the
/// «вызвать» op form (spec §8 UI) for an account THEY explicitly added. Reaching an
/// owner-specified arbitrary URL is the entire point of a generic REST connector, not a bug to
/// guard against — so this client keeps reqwest's default (bounded, following) redirect policy,
/// same as any other user-driven outbound call in this app. The bearer is still only ever sent to
/// wherever `args["url"]` (and any redirect target `args["url"]` itself points at) resolves to —
/// the owner's own choice of target, at both hops.
pub struct GenericRestAdapter {
    client: reqwest::Client,
}

impl GenericRestAdapter {
    /// `account.provider` value this adapter answers for (spec §7: `provider="generic-rest"`).
    pub const PROVIDER: &'static str = "generic-rest";

    pub fn new() -> Result<Self, ConnectorError> {
        let client = reqwest::Client::builder()
            .timeout(GENERIC_REST_TIMEOUT)
            .build()
            .map_err(|e| ConnectorError::Http(e.to_string()))?;
        Ok(Self { client })
    }
}

impl ConnectorAdapter for GenericRestAdapter {
    fn provider(&self) -> &str {
        Self::PROVIDER
    }

    fn list_ops(&self) -> Vec<ConnectorOp> {
        vec![
            ConnectorOp {
                name: "get".to_string(),
                description: Some(
                    "HTTP GET args.url with the account bearer as Authorization: Bearer <token>"
                        .to_string(),
                ),
            },
            ConnectorOp {
                name: "post".to_string(),
                description: Some(
                    "HTTP POST args.url with JSON args.body and the account bearer as \
                     Authorization: Bearer <token>"
                        .to_string(),
                ),
            },
        ]
    }

    async fn invoke(
        &self,
        token: &AccountToken,
        op: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, ConnectorError> {
        let url = args
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ConnectorError::InvalidArgs("missing \"url\" field".to_string()))?;

        let builder = match op {
            "get" => self.client.get(url),
            "post" => {
                let body = args.get("body").cloned().unwrap_or(serde_json::Value::Null);
                self.client.post(url).json(&body)
            }
            other => return Err(ConnectorError::UnknownOp(other.to_string())),
        };

        let response = builder
            .bearer_auth(&token.bearer)
            .send()
            .await
            .map_err(classify_reqwest_error)?;

        let status = response.status();
        if !status.is_success() {
            // Status code only — NEVER the response body (spec §6: never log tool/op result
            // content; an error body from an arbitrary owner-chosen URL could itself be
            // attacker-influenced content this layer must not echo further than necessary).
            return Err(ConnectorError::UpstreamStatus(status.as_u16()));
        }
        response
            .json::<serde_json::Value>()
            .await
            .map_err(|e| ConnectorError::Request(e.to_string()))
    }
}

fn classify_reqwest_error(e: reqwest::Error) -> ConnectorError {
    if e.is_timeout() {
        ConnectorError::Timeout
    } else {
        ConnectorError::Request(e.to_string())
    }
}

// ================================================================================
// ---- ConnectorInvoke (spec §5/§6, task T12) ----
// ================================================================================

/// Domain error from [`invoke`] (S-EXT spec §6/§7, task T12) — mirrors
/// `crate::mcp::OrchdMcpError`'s shape for the connector-invoke flow. NOT a wire type; mapping
/// this onto `bpa_orchd_proto::OrchdErrorCode` is the caller's job (mirrors `OrchdMcpError`'s own
/// doc comment).
#[derive(Debug)]
pub enum ConnectorInvokeError {
    /// `trust::authorize` denied the `ConnectorInvoke` action. Phase 1 (`trust::evaluate`, this
    /// task) always evaluates `Action::ConnectorInvoke` to `Decision::Allow` — spend/rate caps
    /// land in T18 — so this variant has no live caller today, but the match arm exists so this
    /// flow already has a typed home for that future denial, exactly like `OrchdMcpError::
    /// ConsentRequired`/`ToolDisabled` did before their own gates existed.
    Denied(String),
    /// The adapter itself failed (unknown provider/op, bad args, transport/timeout/HTTP-status
    /// error) — see [`ConnectorError`] for the specific cause.
    Adapter(ConnectorError),
    /// Underlying persistence failure (looking up the account row, or `trust::authorize`'s own
    /// audit write).
    Persist(crate::persistence::OrchdPersistError),
}

impl std::fmt::Display for ConnectorInvokeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConnectorInvokeError::Denied(reason) => write!(f, "connector invoke denied: {reason}"),
            ConnectorInvokeError::Adapter(e) => write!(f, "connector adapter error: {e}"),
            ConnectorInvokeError::Persist(e) => write!(f, "persistence error: {e}"),
        }
    }
}

impl std::error::Error for ConnectorInvokeError {}

impl From<crate::persistence::OrchdPersistError> for ConnectorInvokeError {
    fn from(e: crate::persistence::OrchdPersistError) -> Self {
        ConnectorInvokeError::Persist(e)
    }
}

impl From<ConnectorError> for ConnectorInvokeError {
    fn from(e: ConnectorError) -> Self {
        ConnectorInvokeError::Adapter(e)
    }
}

/// v1 has exactly one shipped [`ConnectorAdapter`] (spec §7). See [`ConnectorAdapter`]'s own doc
/// comment for why this is a plain `match` on `provider` rather than a `dyn`-based registry.
fn resolve_adapter(provider: &str) -> Result<GenericRestAdapter, ConnectorError> {
    match provider {
        GenericRestAdapter::PROVIDER => GenericRestAdapter::new(),
        other => Err(ConnectorError::NoAdapter(other.to_string())),
    }
}

/// `ConnectorInvoke` (spec §5/§6, task T12): trust-gated, adapter-dispatched direct-API call.
///
/// Trust-gated identically to `mcp::invoke::call_tool` (spec §6: "passes through `trust::
/// authorize` IDENTICALLY to `McpCallTool`") — [`trust::authorize`] ALWAYS writes a
/// `connector_invoke` `audit_log` row (allow or, once T18 lands, deny) before this function does
/// anything else network-shaped.
///
/// Takes the SHARED `Arc<tokio::sync::Mutex<Db>>` (the exact type `socket_server::ServerDeps.db`
/// holds) and locks it in TWO phases with the network round-trip (bearer resolution — which may
/// hit an OAuth refresh endpoint, see `accounts::ConnectorsState::token_for` — plus the adapter's
/// own HTTP call) sandwiched BETWEEN, holding NO `Db` guard across either await (same T6
/// review-fix discipline `mcp::invoke::call_tool`/`accounts::token_for`/`accounts::complete_oauth`
/// all follow — holding the single daemon-wide `Db` mutex across a network round-trip stalls
/// every other orchd connection for the duration of that call).
///
/// See this module's doc comment for why no `mcp_invocation`/`mcp_artifact` row is written (v1,
/// audit-only) and what `McpCallResult.artifact_id`/`invocation_id` mean instead.
pub async fn invoke(
    connectors: &ConnectorsState,
    db: &Arc<TokioMutex<Db>>,
    account_id: &str,
    op: &str,
    args_json: &str,
    project_id: Option<String>,
) -> Result<McpCallResult, ConnectorInvokeError> {
    // ---- Phase 1: lock -> read account + authorize (+ audit). Guard dropped at the end of this
    // block, BEFORE any network await. Mirrors `mcp::invoke::call_tool`'s own phase split: fetch
    // the resource row FIRST (an unknown account_id surfaces as NotFound here, before authorize
    // is even called — nothing meaningful to audit-log for a resource that doesn't exist), THEN
    // authorize. ----
    let (account, decision) = {
        let guard = db.lock().await;
        let account = guard.get_account(account_id)?;
        let decision = trust::authorize(
            &guard,
            &Action::ConnectorInvoke {
                account_id: account_id.to_string(),
                op: op.to_string(),
                project_id: project_id.clone(),
            },
        )?;
        (account, decision)
    };
    if let Decision::Deny { reason } = decision {
        return Err(ConnectorInvokeError::Denied(reason));
    }

    let args: serde_json::Value = serde_json::from_str(args_json).map_err(|e| {
        ConnectorInvokeError::Adapter(ConnectorError::InvalidArgs(format!(
            "args_json is not valid JSON: {e}"
        )))
    })?;

    // ---- Phase 2: network. NO `Db`/`MutexGuard` reference is alive here. `token_for` may hit an
    // OAuth refresh endpoint; `adapter.invoke` always hits the third-party API. ----
    let token = connectors.token_for(db, account_id).await?;
    let adapter = resolve_adapter(&account.provider)?;
    let result = adapter.invoke(&token, op, args).await?;

    let content_json = serde_json::to_string(&result).map_err(|e| {
        ConnectorInvokeError::Adapter(ConnectorError::InvalidArgs(format!(
            "failed to serialize connector result: {e}"
        )))
    })?;

    // Structured tracing — account id/provider/op only, NEVER the bearer/args/result content
    // (spec §6, mirrors `mcp::invoke::call_tool`'s own "tool call completed" trace).
    tracing::info!(
        account_id,
        provider = %account.provider,
        op,
        "connector: invoke completed"
    );

    Ok(McpCallResult {
        artifact_id: UNPERSISTED_SENTINEL.to_string(),
        invocation_id: UNPERSISTED_SENTINEL.to_string(),
        content_json,
        is_error: false,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc as StdArc, Mutex as StdMutex};

    use serde_json::json;

    use super::*;

    // ---- Keychain skip-guard (mirrors `accounts.rs`'s own equivalent — see that file's test
    // module doc comment: `bpa_secrets`'s precise 4-OSStatus-code probe is `#[cfg(test)]`-private
    // to its own crate, so every OTHER crate's test suite that touches the real Keychain carries
    // this deliberately looser, but equally honest and always-loud, equivalent). Only the
    // `connectors::invoke` orchestration tests below need this — `GenericRestAdapter::invoke`
    // itself takes a plain in-memory [`AccountToken`], no Keychain involved. ----

    fn keychain_available() -> bool {
        let probe = bpa_secrets::account_ref("connectors-adapter-probe", "test");
        match bpa_secrets::set(&probe, b"probe") {
            Ok(()) => {
                let _ = bpa_secrets::delete(&probe);
                true
            }
            Err(e) => {
                eprintln!(
                    "SKIP connectors::adapter keychain-backed test: login keychain unavailable \
                     in this environment ({e}) — graceful skip, not a pass. Run locally with an \
                     unlocked login keychain to exercise the full assertion."
                );
                false
            }
        }
    }

    /// Best-effort teardown so a panicking test never leaves a stray real Keychain entry.
    struct DeleteAccountSecretsOnDrop<'a> {
        account_id: &'a str,
        kinds: &'a [&'a str],
    }
    impl Drop for DeleteAccountSecretsOnDrop<'_> {
        fn drop(&mut self) {
            for kind in self.kinds {
                let _ = bpa_secrets::delete(&bpa_secrets::account_ref(self.account_id, kind));
            }
        }
    }

    fn new_db() -> Arc<TokioMutex<Db>> {
        Arc::new(TokioMutex::new(Db::open_in_memory().unwrap()))
    }

    fn token(bearer: &str) -> AccountToken {
        AccountToken {
            bearer: bearer.to_string(),
        }
    }

    // ---- loopback generic-REST target stub (mirrors `accounts.rs`'s `spawn_token_stub`/
    // `dispatch_integration.rs`'s `spawn_stub_mcp_server` axum/TcpListener wiring — same shape, a
    // different tiny protocol: captures the Authorization header + POST body it received so tests
    // can assert the bearer/body genuinely reached the wire). ----

    #[derive(Default)]
    struct CapturedRequest {
        auth_header: Option<String>,
        body: Option<serde_json::Value>,
    }

    async fn get_handler(
        axum::extract::State(captured): axum::extract::State<StdArc<StdMutex<CapturedRequest>>>,
        headers: axum::http::HeaderMap,
    ) -> axum::Json<serde_json::Value> {
        let auth = headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        captured.lock().unwrap().auth_header = auth;
        axum::Json(json!({"ok": true, "via": "get"}))
    }

    async fn post_handler(
        axum::extract::State(captured): axum::extract::State<StdArc<StdMutex<CapturedRequest>>>,
        headers: axum::http::HeaderMap,
        axum::Json(body): axum::Json<serde_json::Value>,
    ) -> axum::Json<serde_json::Value> {
        let auth = headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        {
            let mut c = captured.lock().unwrap();
            c.auth_header = auth;
            c.body = Some(body.clone());
        }
        axum::Json(json!({"ok": true, "via": "post", "echoed": body}))
    }

    async fn error_handler() -> axum::http::StatusCode {
        axum::http::StatusCode::INTERNAL_SERVER_ERROR
    }

    async fn spawn_rest_stub() -> (String, StdArc<StdMutex<CapturedRequest>>) {
        let captured = StdArc::new(StdMutex::new(CapturedRequest::default()));
        let router = axum::Router::new()
            .route("/get", axum::routing::get(get_handler))
            .route("/post", axum::routing::post(post_handler))
            .route("/error", axum::routing::get(error_handler))
            .with_state(captured.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind loopback rest stub");
        let addr = listener.local_addr().expect("stub local_addr");
        tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });
        (format!("http://{addr}"), captured)
    }

    // ================================================================================
    // ---- GenericRestAdapter ----
    // ================================================================================

    #[test]
    fn generic_rest_adapter_reports_provider_and_ops() {
        let adapter = GenericRestAdapter::new().unwrap();
        assert_eq!(adapter.provider(), "generic-rest");
        let ops: Vec<String> = adapter.list_ops().into_iter().map(|o| o.name).collect();
        assert_eq!(ops, vec!["get".to_string(), "post".to_string()]);
    }

    #[tokio::test]
    async fn generic_rest_adapter_get_sends_bearer_and_returns_stub_json() {
        let (base, captured) = spawn_rest_stub().await;
        let adapter = GenericRestAdapter::new().unwrap();

        let result = adapter
            .invoke(
                &token("test-bearer-abc123"),
                "get",
                json!({"url": format!("{base}/get")}),
            )
            .await
            .unwrap();

        assert_eq!(result, json!({"ok": true, "via": "get"}));
        assert_eq!(
            captured.lock().unwrap().auth_header.as_deref(),
            Some("Bearer test-bearer-abc123"),
            "GenericRestAdapter must send the account bearer as Authorization: Bearer <token>"
        );
    }

    #[tokio::test]
    async fn generic_rest_adapter_post_sends_json_body_and_bearer() {
        let (base, captured) = spawn_rest_stub().await;
        let adapter = GenericRestAdapter::new().unwrap();

        let result = adapter
            .invoke(
                &token("test-bearer-xyz789"),
                "post",
                json!({"url": format!("{base}/post"), "body": {"hello": "world"}}),
            )
            .await
            .unwrap();

        assert_eq!(result["echoed"], json!({"hello": "world"}));
        let c = captured.lock().unwrap();
        assert_eq!(c.auth_header.as_deref(), Some("Bearer test-bearer-xyz789"));
        assert_eq!(c.body, Some(json!({"hello": "world"})));
    }

    #[tokio::test]
    async fn generic_rest_adapter_upstream_500_returns_typed_error() {
        let (base, _captured) = spawn_rest_stub().await;
        let adapter = GenericRestAdapter::new().unwrap();

        let err = adapter
            .invoke(&token("t"), "get", json!({"url": format!("{base}/error")}))
            .await
            .unwrap_err();
        assert!(
            matches!(err, ConnectorError::UpstreamStatus(500)),
            "expected UpstreamStatus(500), got {err:?}"
        );
    }

    #[tokio::test]
    async fn generic_rest_adapter_unknown_op_returns_typed_error() {
        let adapter = GenericRestAdapter::new().unwrap();
        let err = adapter
            .invoke(
                &token("t"),
                "delete",
                json!({"url": "http://127.0.0.1:1/x"}),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ConnectorError::UnknownOp(op) if op == "delete"));
    }

    #[tokio::test]
    async fn generic_rest_adapter_missing_url_returns_typed_error() {
        let adapter = GenericRestAdapter::new().unwrap();
        let err = adapter
            .invoke(&token("t"), "get", json!({}))
            .await
            .unwrap_err();
        assert!(matches!(err, ConnectorError::InvalidArgs(_)));
    }

    // ================================================================================
    // ---- connectors::invoke orchestration ----
    // ================================================================================

    #[tokio::test]
    async fn connector_invoke_happy_path_authorizes_audits_and_returns_untrusted_result() {
        if !keychain_available() {
            return;
        }
        let (base, captured) = spawn_rest_stub().await;

        let db = new_db();
        let connectors = ConnectorsState::new();
        let account = {
            let guard = db.lock().await;
            connectors
                .add_apikey(&guard, "generic-rest", "My REST", "test-api-key-42")
                .unwrap()
        };
        let _cleanup = DeleteAccountSecretsOnDrop {
            account_id: &account.id,
            kinds: &["apikey"],
        };

        let args = json!({"url": format!("{base}/get")}).to_string();
        let result = invoke(&connectors, &db, &account.id, "get", &args, None)
            .await
            .unwrap();

        assert!(!result.is_error);
        assert_eq!(
            result.content_json,
            serde_json::to_string(&json!({"ok": true, "via": "get"})).unwrap()
        );
        assert_eq!(
            captured.lock().unwrap().auth_header.as_deref(),
            Some("Bearer test-api-key-42")
        );

        // trust::authorize wrote exactly one connector_invoke/allow audit row (spec §6/D10),
        // carrying account_id/op in the reused server_id/tool_name columns (trust::write_audit's
        // doc comment).
        let guard = db.lock().await;
        let audit_count: i64 = guard
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM audit_log WHERE action='connector_invoke' \
                 AND decision='allow' AND server_id=?1 AND tool_name='get'",
                rusqlite::params![account.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(audit_count, 1);

        // v1 artifact-persistence deferral (see this module's doc comment): no mcp_artifact row
        // is (or even validly could be, given the mcp_server FK) written for a connector
        // invocation — the connector_invoke audit row above is the durable record instead.
        assert!(guard.list_artifacts(None, None, None).unwrap().is_empty());
    }

    #[tokio::test]
    async fn connector_invoke_adapter_transport_error_still_audits_and_returns_typed_error() {
        if !keychain_available() {
            return;
        }
        let (base, _captured) = spawn_rest_stub().await;

        let db = new_db();
        let connectors = ConnectorsState::new();
        let account = {
            let guard = db.lock().await;
            connectors
                .add_apikey(&guard, "generic-rest", "My REST", "test-api-key-99")
                .unwrap()
        };
        let _cleanup = DeleteAccountSecretsOnDrop {
            account_id: &account.id,
            kinds: &["apikey"],
        };

        let args = json!({"url": format!("{base}/error")}).to_string();
        let err = invoke(&connectors, &db, &account.id, "get", &args, None)
            .await
            .unwrap_err();
        assert!(
            matches!(
                err,
                ConnectorInvokeError::Adapter(ConnectorError::UpstreamStatus(500))
            ),
            "expected Adapter(UpstreamStatus(500)), got {err:?}"
        );

        // authorize() ran (and audited) in Phase 1, BEFORE the failing network call in Phase 2 —
        // the audit row exists even though the adapter call itself failed downstream.
        let guard = db.lock().await;
        let audit_count: i64 = guard
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM audit_log WHERE action='connector_invoke' \
                 AND decision='allow'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            audit_count, 1,
            "the connector_invoke audit row is written on every invoke, success or failure"
        );
        assert!(
            guard.list_artifacts(None, None, None).unwrap().is_empty(),
            "a failed adapter call must never produce a result artifact"
        );
    }

    #[tokio::test]
    async fn connector_invoke_unknown_provider_returns_typed_error_after_authorizing() {
        if !keychain_available() {
            return;
        }
        let db = new_db();
        let connectors = ConnectorsState::new();
        let account = {
            let guard = db.lock().await;
            connectors
                .add_apikey(&guard, "some-other-provider", "Unrelated", "k")
                .unwrap()
        };
        let _cleanup = DeleteAccountSecretsOnDrop {
            account_id: &account.id,
            kinds: &["apikey"],
        };

        let err = invoke(&connectors, &db, &account.id, "get", "{}", None)
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            ConnectorInvokeError::Adapter(ConnectorError::NoAdapter(_))
        ));

        // Still authorized (and audited) before the provider lookup failed — trust::authorize
        // has no knowledge of, or dependency on, adapter resolution.
        let guard = db.lock().await;
        let audit_count: i64 = guard
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM audit_log WHERE action='connector_invoke' \
                 AND decision='allow'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(audit_count, 1);
    }

    #[tokio::test]
    async fn connector_invoke_unknown_account_returns_not_found_no_audit() {
        let db = new_db();
        let connectors = ConnectorsState::new();

        let err = invoke(&connectors, &db, "does-not-exist", "get", "{}", None)
            .await
            .unwrap_err();
        assert!(matches!(err, ConnectorInvokeError::Persist(_)));

        let guard = db.lock().await;
        let audit_count: i64 = guard
            .conn()
            .query_row("SELECT COUNT(*) FROM audit_log", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            audit_count, 0,
            "a request against a nonexistent account must never reach authorize/audit"
        );
    }
}
