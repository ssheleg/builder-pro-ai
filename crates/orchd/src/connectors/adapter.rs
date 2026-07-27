//! Direct-API connector adapter + `ConnectorInvoke` (S-EXT spec §6/§7, D5/D10, task T12).
//!
//! [`ConnectorAdapter`] is the seam a provider-specific "social"/REST integration implements
//! (spec §7: `{provider, list_ops, invoke}`). [`GenericRestAdapter`] is the ONE reference adapter
//! this slice ships (`provider = "generic-rest"`, ops `get`/`post` against an arbitrary
//! caller-supplied URL). [`invoke`] is `ConnectorInvoke`'s implementation — routed through the
//! SAME `crate::trust::authorize` choke-point AND the same `mcp_invocation`/`mcp_artifact`
//! persistence path as `mcp::invoke::call_tool` (spec §6: "passes through `trust::authorize`
//! IDENTICALLY to `McpCallTool`"; §7: "same retry/timeout/artifact/audit path as MCP calls").
//!
//! # Durable artifact persistence (spec §6/§7/D9)
//!
//! A successful `ConnectorInvoke` persists a durable `mcp_invocation` row AND a durable
//! `mcp_artifact` row (`is_untrusted=1`), exactly like `McpCallTool` — the result survives an
//! orchd restart and is returned/listed by `McpGetArtifact`/`McpListArtifacts` (spec D9: "always
//! true for external tool output", §6: "every `mcp_artifact` from `McpCallTool` AND
//! `ConnectorInvoke` is `is_untrusted=1`"). Both rows are keyed by `account_id` (the connector
//! account) with `server_id` NULL — the schema's `server_id`/`account_id` XOR (T12 review: the
//! unreleased v3 `mcp_invocation`/`mcp_artifact` DDL was corrected in place to make `server_id`
//! nullable and add `account_id`, rather than forcing a synthetic `mcp_server` row that would
//! leak into `McpListServers`). A failed adapter call records an `ok=0` `mcp_invocation` (with
//! `error_kind`) and NO artifact — same shape as `mcp::invoke::record_failed_invocation`.

use std::sync::Arc;
use std::time::{Duration, Instant};

use bpa_orchd_proto::{ConnectorOp, McpCallResult};
use sha2::{Digest, Sha256};
use tokio::sync::Mutex as TokioMutex;

use super::accounts::{AccountToken, ConnectorError, ConnectorsState};
use crate::persistence::{now_ms, Db, NewArtifact, NewInvocation};
use crate::trust::{self, Action, Decision};

/// Bounded per-request timeout for [`GenericRestAdapter`] (spec §7: "Bounded timeout ...
/// (honest degradation)"). No per-account/per-op override exists yet — the `account` table has
/// no `timeout_ms` column the way `mcp_server` does (spec §4) — so a fixed constant is the
/// honest v1 shape; promoting this to a configurable field is a natural follow-up once a second
/// adapter needs different bounds.
const GENERIC_REST_TIMEOUT: Duration = Duration::from_secs(30);

/// Hard cap on a connector response body (B1): 8 MiB. A connector result is untrusted-class
/// (`is_untrusted=1`), and `reqwest` imposes no default body limit, so `.json()` on an arbitrarily
/// large / chunked response would OOM the whole orchd process. The body is streamed and rejected
/// (`ConnectorError::OversizedBody`) the instant it crosses this cap — generous for real JSON API
/// responses, small enough that a hostile endpoint can't exhaust memory.
const MAX_CONNECTOR_BODY: usize = 8 * 1024 * 1024;

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
/// «invoke» op form (spec §8 UI) for an account THEY explicitly added. Reaching an
/// owner-specified arbitrary URL is the entire point of a generic REST connector, not a bug to
/// guard against.
///
/// Even so, the account bearer is sent ONLY to the literal `args["url"]` the owner typed: this
/// client disables redirect-following (`redirect::Policy::none()`), so a redirecting endpoint
/// surfaces honestly as `ConnectorError::UpstreamStatus(3xx)` instead of silently forwarding the
/// `Authorization: Bearer` to a target the owner did NOT type (M2). reqwest's default policy
/// already strips `Authorization` on cross-HOST redirects but KEEPS it on same-host redirects —
/// a compromised owner-chosen endpoint that 302s within its own host would otherwise receive the
/// live bearer at the redirect target. The owner can always invoke the final URL directly.
pub struct GenericRestAdapter {
    client: reqwest::Client,
}

impl GenericRestAdapter {
    /// `account.provider` value this adapter answers for (spec §7: `provider="generic-rest"`).
    pub const PROVIDER: &'static str = "generic-rest";

    pub fn new() -> Result<Self, ConnectorError> {
        let client = reqwest::Client::builder()
            .timeout(GENERIC_REST_TIMEOUT)
            // M2: never follow redirects — the bearer goes ONLY to the literal `args["url"]`. A
            // redirecting endpoint surfaces as `UpstreamStatus(3xx)` (handled by the `!is_success`
            // branch in `invoke`) instead of forwarding `Authorization` to an untyped target.
            .redirect(reqwest::redirect::Policy::none())
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
        // Read the body with a hard byte cap (B1): a connector result is untrusted-class, and
        // `reqwest` imposes no default limit — `.json()` would buffer an arbitrarily large (or
        // chunked, no Content-Length) body wholesale and OOM the whole orchd process. Stream chunks
        // and fail fast the instant the accumulated size crosses the cap; never allocate the body.
        let mut response = response;
        let mut buf: Vec<u8> = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|e| ConnectorError::Request(e.to_string()))?
        {
            if buf.len() + chunk.len() > MAX_CONNECTOR_BODY {
                return Err(ConnectorError::OversizedBody(MAX_CONNECTOR_BODY));
            }
            buf.extend_from_slice(&chunk);
        }
        serde_json::from_slice::<serde_json::Value>(&buf)
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
    /// `trust::authorize` denied the `ConnectorInvoke` action — a spend/rate policy-cap breach
    /// (task T18, spec §6/BL-22: "connector_invoke passes through trust::authorize IDENTICALLY
    /// to McpCallTool — same policy scope"). Carries the reason literal
    /// (`rate_limit_exceeded`/`spend_cap_exceeded`) straight through to the wire error message
    /// (see `map_connector_invoke_err`).
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

/// `ConnectorListOps` (spec §5, task T13a): resolves `provider`'s adapter and returns its op list.
/// Read-only — no network, no DB, no trust choke-point involvement (mirrors `McpListTools`
/// reading straight from the cache rather than going through `trust::authorize`; listing what a
/// provider CAN do is not itself a dispatch/egress action).
pub fn list_ops(provider: &str) -> Result<Vec<ConnectorOp>, ConnectorError> {
    Ok(resolve_adapter(provider)?.list_ops())
}

/// `ConnectorInvoke` (spec §5/§6, task T12): trust-gated, adapter-dispatched direct-API call.
///
/// Trust-gated identically to `mcp::invoke::call_tool` (spec §6: "passes through `trust::
/// authorize` IDENTICALLY to `McpCallTool`") — [`trust::authorize`] ALWAYS writes a
/// `connector_invoke` `audit_log` row (allow or deny) before this function does
/// anything else network-shaped.
///
/// Takes the SHARED `Arc<tokio::sync::Mutex<Db>>` (the exact type `socket_server::ServerDeps.db`
/// holds) and locks it in phases with the network round-trip (bearer resolution — which may hit
/// an OAuth refresh endpoint, see `accounts::ConnectorsState::token_for` — plus the adapter's own
/// HTTP call) sandwiched BETWEEN, holding NO `Db` guard across either await (same T6 review-fix
/// discipline `mcp::invoke::call_tool`/`accounts::token_for`/`accounts::complete_oauth` all follow
/// — holding the single daemon-wide `Db` mutex across a network round-trip stalls every other
/// orchd connection for the duration of that call).
///
/// Persists a durable `mcp_invocation` on every dispatched attempt (success OR terminal failure,
/// spec D8) plus a durable `mcp_artifact` (`is_untrusted=1`, spec D9) on success — keyed by
/// `account_id` with `server_id` NULL — so a connector result survives an orchd restart and is
/// returned/listed by `McpGetArtifact`/`McpListArtifacts`, exactly like an `McpCallTool` result
/// (see this module's doc comment).
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

    // args_json parse happens BEFORE any network dispatch (mirrors `mcp::invoke::call_tool`: a
    // malformed request never becomes a dispatched invocation row). `request_hash` is the sha256
    // of the exact args bytes — NEVER the args themselves (spec §4/§6).
    let args: serde_json::Value = serde_json::from_str(args_json).map_err(|e| {
        ConnectorInvokeError::Adapter(ConnectorError::InvalidArgs(format!(
            "args_json is not valid JSON: {e}"
        )))
    })?;
    let request_hash = sha256_hex(args_json.as_bytes());
    let started_at = now_ms();
    let start = Instant::now();

    // ---- Phase 2: network + local adapter resolution. NO `Db`/`MutexGuard` reference is alive
    // here. `token_for` may hit an OAuth refresh endpoint; `adapter.invoke` always hits the
    // third-party API. Any failure funnels to the failed-invocation recorder below. ----
    let outcome = async {
        let adapter = resolve_adapter(&account.provider)?;
        let token = connectors.token_for(db, account_id).await?;
        adapter.invoke(&token, op, args).await
    }
    .await;

    let elapsed_ms = start.elapsed().as_millis() as i64;

    // ---- Phase 3: lock -> write. ----
    match outcome {
        Ok(result) => {
            let content_json = serde_json::to_string(&result).map_err(|e| {
                ConnectorInvokeError::Adapter(ConnectorError::InvalidArgs(format!(
                    "failed to serialize connector result: {e}"
                )))
            })?;

            let (artifact, invocation_id) = {
                let guard = db.lock().await;
                let invocation = guard.insert_invocation(NewInvocation {
                    server_id: None,
                    account_id: Some(account_id.to_string()),
                    tool_name: op.to_string(),
                    project_id: project_id.clone(),
                    request_hash,
                    ok: true,
                    error_kind: None,
                    latency_ms: elapsed_ms,
                    // A generic-REST connector reports no token/cost accounting (spec D8: these
                    // stay null when the source doesn't report usage — honestly).
                    cost_usd: None,
                    input_tokens: None,
                    output_tokens: None,
                    started_at,
                })?;
                let artifact = guard.insert_artifact(NewArtifact {
                    invocation_id: invocation.id.clone(),
                    server_id: None,
                    account_id: Some(account_id.to_string()),
                    tool_name: op.to_string(),
                    project_id,
                    content_json,
                    // No MCP-content-block shape to flatten for an arbitrary REST JSON result;
                    // the full result lives in content_json. (A future adapter that returns a
                    // known text shape could populate this.)
                    content_text: None,
                })?;
                (artifact, invocation.id)
            };

            // Structured tracing — account id/provider/op only, NEVER the bearer/args/result
            // content (spec §6, mirrors `mcp::invoke::call_tool`'s own "tool call completed").
            tracing::info!(
                account_id,
                provider = %account.provider,
                op,
                ok = true,
                latency_ms = elapsed_ms,
                "connector: invoke completed"
            );

            // BL-120: reply with the STORED content (identical unless the artifact write-time
            // cap truncated it — then the truncated prefix plus `truncated: Some(true)`), so
            // this response frame can never exceed `MAX_FRAME_LEN` and drop the connection.
            Ok(McpCallResult {
                artifact_id: artifact.id,
                invocation_id,
                content_json: artifact.content_json,
                is_error: false,
                truncated: artifact.truncated,
            })
        }
        Err(e) => {
            let error_kind = connector_error_kind(&e);
            // A terminal failure records an ok=0 invocation (spec D8) and NO artifact — same
            // shape as `mcp::invoke::record_failed_invocation`. Trace carries the error KIND
            // only, never the error's own message text (which could echo upstream content).
            {
                let guard = db.lock().await;
                guard.insert_invocation(NewInvocation {
                    server_id: None,
                    account_id: Some(account_id.to_string()),
                    tool_name: op.to_string(),
                    project_id,
                    request_hash,
                    ok: false,
                    error_kind: Some(error_kind.to_string()),
                    latency_ms: elapsed_ms,
                    cost_usd: None,
                    input_tokens: None,
                    output_tokens: None,
                    started_at,
                })?;
            }
            tracing::warn!(
                account_id,
                provider = %account.provider,
                op,
                ok = false,
                error_kind,
                "connector: invoke failed"
            );
            Err(ConnectorInvokeError::Adapter(e))
        }
    }
}

/// Short, non-secret `mcp_invocation.error_kind` label for a terminal [`ConnectorError`] (mirrors
/// `mcp::invoke::classify_error_kind`). Never the error's own message text.
fn connector_error_kind(err: &ConnectorError) -> &'static str {
    match err {
        ConnectorError::NoAdapter(_) => "no_adapter",
        ConnectorError::UnknownOp(_) => "unknown_op",
        ConnectorError::InvalidArgs(_) => "invalid_args",
        ConnectorError::Request(_) => "request",
        ConnectorError::Timeout => "timeout",
        ConnectorError::UpstreamStatus(_) => "upstream_status",
        ConnectorError::OversizedBody(_) => "oversized_body",
        ConnectorError::SecretNotUtf8 => "secret",
        ConnectorError::Secret(_) => "secret",
        ConnectorError::TokenExchange(_) => "token_exchange",
        ConnectorError::Http(_) => "http",
        ConnectorError::UnknownProvider(_) => "unknown_provider",
        ConnectorError::UnknownState => "unknown_state",
        ConnectorError::InvalidConfig(_) => "invalid_config",
        ConnectorError::Persist(_) => "persist",
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
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
        // Hang-proof bounded probe (BL-107) — see `accounts::keychain_available` for the rationale:
        // an inline round-trip wedges the whole test binary on a Keychain authorization prompt this
        // binary was never approved for, and the shared `bpa_secrets::keychain_available` bounds it
        // on a worker thread into a loud SKIP instead.
        bpa_secrets::keychain_available(std::time::Duration::from_secs(3))
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

    /// M2: returns 302 → `/get`, so a test can prove the adapter does NOT follow the redirect (the
    /// bearer would otherwise be re-sent to the untyped target).
    async fn redirect_handler() -> (
        axum::http::StatusCode,
        [(axum::http::HeaderName, &'static str); 1],
    ) {
        (
            axum::http::StatusCode::FOUND,
            [(axum::http::header::LOCATION, "/get")],
        )
    }

    /// Returns a 200 body larger than `MAX_CONNECTOR_BODY` so the adapter's capped read must reject
    /// it (B1) rather than buffer it whole.
    async fn big_handler() -> String {
        "a".repeat(MAX_CONNECTOR_BODY + 1024)
    }

    async fn spawn_rest_stub() -> (String, StdArc<StdMutex<CapturedRequest>>) {
        let captured = StdArc::new(StdMutex::new(CapturedRequest::default()));
        let router = axum::Router::new()
            .route("/get", axum::routing::get(get_handler))
            .route("/post", axum::routing::post(post_handler))
            .route("/error", axum::routing::get(error_handler))
            .route("/big", axum::routing::get(big_handler))
            .route("/redirect", axum::routing::get(redirect_handler))
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
    async fn generic_rest_adapter_rejects_oversized_body_without_buffering_it() {
        let (base, _captured) = spawn_rest_stub().await;
        let adapter = GenericRestAdapter::new().unwrap();

        let err = adapter
            .invoke(
                &token("test-bearer"),
                "get",
                json!({"url": format!("{base}/big")}),
            )
            .await
            .expect_err("an over-cap response body must be a typed error, not buffered");
        assert!(
            matches!(err, ConnectorError::OversizedBody(cap) if cap == MAX_CONNECTOR_BODY),
            "expected OversizedBody(MAX_CONNECTOR_BODY), got {err:?}"
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

    // M2: the adapter MUST NOT follow redirects — the account bearer goes only to the literal
    // `args["url"]`, never to an (attacker-influenced) Location target. reqwest's default policy
    // keeps `Authorization` on same-host redirects; `Policy::none()` surfaces the 3xx honestly.
    #[tokio::test]
    async fn generic_rest_adapter_does_not_follow_redirects_with_the_bearer() {
        let (base, captured) = spawn_rest_stub().await;
        let adapter = GenericRestAdapter::new().unwrap();
        let err = adapter
            .invoke(
                &token("leak-canary"),
                "get",
                json!({"url": format!("{base}/redirect")}),
            )
            .await
            .expect_err("a redirect must surface as an error, not be followed");
        match err {
            ConnectorError::UpstreamStatus(code) => {
                assert!(
                    (300..400).contains(&code),
                    "expected a 3xx surfaced (not followed), got {code}"
                );
            }
            other => panic!("expected UpstreamStatus(3xx), got {other:?}"),
        }
        // `/get` (the redirect target) was never hit, so its handler captured no Authorization.
        let captured_lock = captured.lock().unwrap();
        let target_auth = captured_lock.auth_header.clone();
        assert!(
            target_auth.is_none(),
            "the bearer leaked to the redirect target's handler: {target_auth:?}"
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
    async fn connector_invoke_happy_path_authorizes_audits_and_persists_untrusted_artifact() {
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
        // real (resolvable) ids, not a sentinel.
        assert!(!result.artifact_id.is_empty());
        assert!(!result.invocation_id.is_empty());

        let guard = db.lock().await;

        // trust::authorize wrote exactly one connector_invoke/allow audit row (spec §6/D10),
        // carrying account_id/op in the reused server_id/tool_name columns (trust::write_audit's
        // doc comment).
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

        // D9/§6: the connector result persists as a durable untrusted artifact, keyed by
        // account_id (server_id null), returned by get_artifact/list_artifacts.
        let artifact = guard.get_artifact(&result.artifact_id).unwrap();
        assert!(
            artifact.is_untrusted,
            "connector artifact must be untrusted (D9)"
        );
        assert_eq!(artifact.account_id.as_deref(), Some(account.id.as_str()));
        assert_eq!(artifact.server_id, None);
        assert_eq!(artifact.tool_name, "get");
        assert_eq!(artifact.content_json, result.content_json);
        assert_eq!(artifact.invocation_id, result.invocation_id);

        // it shows up in an unfiltered McpListArtifacts (the same query the wire verb runs).
        let listed = guard.list_artifacts(None, None, None).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, result.artifact_id);

        // and a durable ok=1 invocation row keyed by account_id (server_id null).
        let invocation = guard.list_invocations(None, None, None).unwrap();
        assert_eq!(invocation.len(), 1);
        assert!(invocation[0].ok);
        assert_eq!(
            invocation[0].account_id.as_deref(),
            Some(account.id.as_str())
        );
        assert_eq!(invocation[0].server_id, None);
        assert_eq!(
            invocation[0].request_hash,
            sha256_hex(args.as_bytes()),
            "request_hash is sha256(args_json), never the raw args"
        );
    }

    #[tokio::test]
    async fn connector_invoke_adapter_error_records_failed_invocation_and_no_artifact() {
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

        let guard = db.lock().await;

        // authorize() ran (and audited) in Phase 1, BEFORE the failing network call in Phase 2 —
        // the audit row exists even though the adapter call itself failed downstream.
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

        // D8: a terminal failure still records an ok=0 invocation (keyed by account_id), but NO
        // artifact.
        let invocations = guard.list_invocations(None, None, None).unwrap();
        assert_eq!(invocations.len(), 1);
        assert!(!invocations[0].ok);
        assert_eq!(
            invocations[0].error_kind.as_deref(),
            Some("upstream_status")
        );
        assert_eq!(
            invocations[0].account_id.as_deref(),
            Some(account.id.as_str())
        );
        assert_eq!(invocations[0].server_id, None);
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
