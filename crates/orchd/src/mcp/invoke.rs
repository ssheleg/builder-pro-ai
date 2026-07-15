//! `tools/call` invocation (S-EXT spec §4/§6/§7, D7/D8/D9, task T5): trust-gated, timeout +
//! transport-only retry, `mcp_invocation` + `mcp_artifact` persistence. See [`super::ToolCaller`]
//! for the test/production session seam.

use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bpa_orchd_proto::McpCallResult;
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;

use super::{resolve_bearer, McpServerRow, OrchdMcpError, ToolCaller};
use crate::persistence::{now_ms, Db, NewArtifact, NewInvocation};
use crate::trust::{self, Action, Decision};

/// Invoke `tool_name` on `server_id` with `args_json` (raw JSON text — the exact bytes
/// `request_hash` is computed over, spec §4: "sha256 of args, NOT the args themselves").
///
/// Trust-gated (spec D10): a disabled/unrecognized tool (`mcp_tool.enabled=0`, or no cached row
/// at all) is rejected BEFORE any network call, args parsing, or bearer resolution — no
/// `mcp_invocation`/`mcp_artifact` row is written for a denial.
///
/// Every dispatched attempt — success or terminal failure — writes exactly one `mcp_invocation`
/// row (spec D8). A successful RPC ALSO writes an `mcp_artifact` row (`is_untrusted=1`, spec
/// D9), even when the tool's own result reports `is_error=true`: the RPC itself still completed
/// (that's a *tool-level* failure inside a successful call, not a transport/protocol failure) —
/// `McpCallResult.is_error` mirrors the tool's own verdict back to the caller.
///
/// `connect_fn` builds the live [`ToolCaller`] session (production: [`super::connect_session`];
/// tests: a fake factory). Phase-1 connects ONCE per call (task-5 brief: "connect-per-call is
/// fine") — retries (`server.max_retries`, spec D7) apply only to the `tools/call` RPC itself,
/// never to reconnecting, and NEVER to a `ToolError` (a tool that ran and failed must not be
/// blindly re-invoked).
///
/// Takes the SHARED `Arc<Mutex<Db>>` (the exact type `socket_server::ServerDeps.db` holds) rather
/// than a bare `&Db`, and locks it in THREE phases with the network round-trip (connect + the
/// timeout/retry `tools/call` loop) sandwiched BETWEEN, holding NO `Db` guard across any await
/// (T6 review fix, S-EXT §6 — same rationale as [`super::lifecycle::connect`]: holding the single
/// daemon DB mutex across the MCP round-trip would stall every other orchd connection for up to
/// `(1+max_retries)×timeout`, and was the sole reason `Db` previously needed a hand-written `Sync`
/// bound). The per-attempt `mcp_invocation` row (spec D8) is still written exactly once — in phase
/// 3, after the network phase decides the final outcome, on BOTH the success and every failure
/// path (connect failure, terminal call failure) — just no longer while a guard is alive across
/// the network.
pub async fn call_tool<F, Fut, S>(
    db: &Arc<Mutex<Db>>,
    server_id: &str,
    tool_name: &str,
    args_json: &str,
    project_id: Option<String>,
    connect_fn: F,
) -> Result<McpCallResult, OrchdMcpError>
where
    F: FnOnce(McpServerRow, Option<String>) -> Fut,
    Fut: Future<Output = Result<S, bpa_mcp::McpError>>,
    S: ToolCaller,
{
    // ---- Phase 1: lock -> read. Guard dropped at the end of this block, BEFORE any network
    // await — a disabled/unrecognized tool returns here, never touching the network, never
    // writing an invocation/artifact row (`trust::authorize`'s deny audit row is written under
    // this same guard). ----
    let (server, bearer) = {
        let guard = db.lock().await;
        let server = guard.get_mcp_server(server_id)?;

        let decision = trust::authorize(
            &guard,
            &Action::ToolCall {
                server_id: server.id.clone(),
                tool_name: tool_name.to_string(),
                project_id: project_id.clone(),
            },
        )?;
        // (task T18, spec §6/BL-22): the allowlist denial keeps its existing `ToolDisabled`
        // shape verbatim (`reason == trust::REASON_TOOL_DISABLED`); any OTHER deny reason at this
        // point is a spend/rate POLICY-CAP breach (`trust::check_policy_caps`, gated ONLY on the
        // Allow path an enabled tool already took) — carried through as `PolicyCapExceeded` so
        // the wire error message names WHICH cap tripped instead of reusing "tool disabled" for
        // an unrelated reason.
        if let Decision::Deny { reason } = decision {
            return Err(if reason == trust::REASON_TOOL_DISABLED {
                OrchdMcpError::ToolDisabled
            } else {
                OrchdMcpError::PolicyCapExceeded(reason)
            });
        }

        // (S-EXT §6/D6/D10, task T16): Phase-1 has no persisted session — EVERY call reconnects
        // (doc comment above: "connect-per-call is fine"), and for a stdio server "reconnect"
        // means spawning a NEW local process. The per-tool allowlist check above says nothing
        // about whether spawning is currently authorized, so a stdio server needs its OWN
        // `stdio_exec` gate here too — otherwise `McpCallTool` would be a second, ungated spawn
        // path bypassing the one `McpConnect`/`lifecycle::connect` enforces. `super::
        // connect_action` is the SAME helper `lifecycle::connect` uses, so both paths agree on
        // exactly what's required. HTTP is unchanged (no new gate: `connect_action` only matters
        // for `McpTransport::Stdio` below).
        if server.transport == super::McpTransport::Stdio {
            let spawn_decision = trust::authorize(&guard, &super::connect_action(&server))?;
            if matches!(spawn_decision, Decision::Deny { .. }) {
                return Err(OrchdMcpError::ConsentRequired);
            }
        }

        let bearer = resolve_bearer(&server).map_err(|e| OrchdMcpError::Secret(e.to_string()))?;
        (server, bearer)
    };

    let args: serde_json::Value = serde_json::from_str(args_json).map_err(|e| {
        OrchdMcpError::Mcp(bpa_mcp::McpError::Protocol(format!(
            "McpCallTool.args_json is not valid JSON: {e}"
        )))
    })?;
    let request_hash = sha256_hex(args_json.as_bytes());

    let timeout = Duration::from_millis(server.timeout_ms.max(0) as u64);
    let max_retries = server.max_retries.max(0) as u32;
    let server_id_owned = server.id.clone();

    let started_at = now_ms();
    let start = Instant::now();

    // ---- Phase 2: network. NO `Db`/`MutexGuard` reference is alive here. ----
    //
    // (S-IDEA D12): the connect/`initialize` handshake is bounded by `server.timeout_ms` exactly
    // like the `tools/call` RPC below — a peer that accepts the connection but never completes
    // `initialize` (dead peer, silent firewall drop, overloaded stdio child) must not hang this
    // task forever. An elapsed connect maps to the SAME `McpError::Timeout` the `tools/call`
    // timeout branch produces, so `classify_error_kind` reports `"timeout"` either way.
    let connect_result = match tokio::time::timeout(timeout, connect_fn(server, bearer)).await {
        Ok(result) => result,
        Err(_elapsed) => Err(bpa_mcp::McpError::Timeout),
    };
    let session = match connect_result {
        Ok(session) => session,
        Err(e) => {
            let new = failed_invocation(
                &server_id_owned,
                tool_name,
                project_id,
                &request_hash,
                start.elapsed(),
                started_at,
                &e,
            );
            record_failed_invocation(db, new, &e).await?;
            return Err(OrchdMcpError::Mcp(e));
        }
    };

    let mut retries_used = 0u32;
    let outcome = loop {
        let attempt =
            match tokio::time::timeout(timeout, session.call_tool(tool_name, args.clone())).await {
                Ok(result) => result,
                Err(_elapsed) => Err(bpa_mcp::McpError::Timeout),
            };
        match attempt {
            Ok(result) => break Ok(result),
            Err(e) if retries_used < max_retries && is_retryable(&e) => {
                retries_used += 1;
                continue;
            }
            Err(e) => break Err(e),
        }
    };

    let elapsed = start.elapsed();

    // ---- Phase 3: lock -> write. ----
    match outcome {
        Ok(tool_result) => {
            let usage = tool_result.usage;
            let is_error = tool_result.is_error;
            // Serialization/flattening are pure — done BEFORE re-locking so the guard's scope
            // stays minimal (just the two inserts).
            let content_json = serde_json::to_string(&tool_result.content).map_err(|e| {
                OrchdMcpError::Mcp(bpa_mcp::McpError::Protocol(format!(
                    "failed to serialize tool result content: {e}"
                )))
            })?;
            let content_text = extract_text(&tool_result.content);

            let (artifact_id, invocation_id) = {
                let guard = db.lock().await;
                let invocation = guard.insert_invocation(NewInvocation {
                    server_id: Some(server_id_owned.clone()),
                    account_id: None,
                    tool_name: tool_name.to_string(),
                    project_id: project_id.clone(),
                    request_hash,
                    ok: true,
                    error_kind: None,
                    latency_ms: elapsed.as_millis() as i64,
                    cost_usd: usage.and_then(|u| u.cost_usd),
                    input_tokens: usage.and_then(|u| u.input_tokens),
                    output_tokens: usage.and_then(|u| u.output_tokens),
                    started_at,
                })?;

                let artifact = guard.insert_artifact(NewArtifact {
                    invocation_id: invocation.id.clone(),
                    server_id: Some(server_id_owned.clone()),
                    account_id: None,
                    tool_name: tool_name.to_string(),
                    project_id,
                    content_json: content_json.clone(),
                    content_text,
                })?;
                (artifact.id, invocation.id)
            };

            tracing::info!(
                server_id = %server_id_owned,
                tool_name,
                ok = true,
                latency_ms = elapsed.as_millis() as i64,
                "mcp: tool call completed"
            );

            Ok(McpCallResult {
                artifact_id,
                invocation_id,
                content_json,
                is_error,
            })
        }
        Err(e) => {
            let new = failed_invocation(
                &server_id_owned,
                tool_name,
                project_id,
                &request_hash,
                elapsed,
                started_at,
                &e,
            );
            record_failed_invocation(db, new, &e).await?;
            Err(OrchdMcpError::Mcp(e))
        }
    }
}

fn failed_invocation(
    server_id: &str,
    tool_name: &str,
    project_id: Option<String>,
    request_hash: &str,
    elapsed: Duration,
    started_at: i64,
    err: &bpa_mcp::McpError,
) -> NewInvocation {
    NewInvocation {
        server_id: Some(server_id.to_string()),
        account_id: None,
        tool_name: tool_name.to_string(),
        project_id,
        request_hash: request_hash.to_string(),
        ok: false,
        error_kind: Some(classify_error_kind(err).to_string()),
        latency_ms: elapsed.as_millis() as i64,
        cost_usd: None,
        input_tokens: None,
        output_tokens: None,
        started_at,
    }
}

/// Writes the failed `mcp_invocation` row and a structured warn-level trace — server id/tool
/// name/error kind only, never the error's own message text (which could, for `ToolError` /
/// `Auth`, echo server-supplied content derived from the call). Locks the shared `Db` for the
/// single sync insert only (T6 review fix): the `MutexGuard` never outlives this call and is
/// never held across an await.
async fn record_failed_invocation(
    db: &Arc<Mutex<Db>>,
    new: NewInvocation,
    err: &bpa_mcp::McpError,
) -> Result<(), OrchdMcpError> {
    let error_kind = classify_error_kind(err);
    tracing::warn!(
        // Always `Some` on the MCP path (server_id is now `Option` only because a
        // connector_invoke shares this row shape — see `NewInvocation`); `.as_deref()` keeps the
        // trace field non-secret and Display-free.
        server_id = new.server_id.as_deref(),
        tool_name = %new.tool_name,
        ok = false,
        error_kind,
        "mcp: tool call failed"
    );
    db.lock().await.insert_invocation(new)?;
    Ok(())
}

/// Spec D7: "retries ONLY when ... the failure is a transport-level pre-dispatch error (never
/// blind re-invoke of a possibly-side-effecting tool)". `ToolError`/`Protocol`/`Auth` are all
/// terminal — retrying an `Auth` failure with the same (already-resolved) bearer would just fail
/// identically, and `Protocol`/`ToolError` mean the server rejected THIS call, not a transient
/// link issue.
fn is_retryable(err: &bpa_mcp::McpError) -> bool {
    matches!(
        err,
        bpa_mcp::McpError::Transport(_) | bpa_mcp::McpError::Timeout
    )
}

fn classify_error_kind(err: &bpa_mcp::McpError) -> &'static str {
    match err {
        bpa_mcp::McpError::Transport(_) => "transport",
        bpa_mcp::McpError::Protocol(_) => "protocol",
        bpa_mcp::McpError::Timeout => "timeout",
        bpa_mcp::McpError::ToolError(_) => "tool_error",
        bpa_mcp::McpError::Auth(_) => "auth",
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

/// Best-effort flatten of a tool result's content into a preview/search string (spec §4
/// `mcp_artifact.content_text`: "flattened text for preview/search"). `content` is the
/// JSON-serialized form of `bpa_mcp::McpToolResult.content` (a `Vec<ContentBlock>` on the wire,
/// rendered as `[{"type":"text","text":"..."}, ...]` for text blocks) — concatenates every
/// `"text"` field found, `None` when nothing textual is extractable (e.g. an image-only result).
fn extract_text(content: &serde_json::Value) -> Option<String> {
    let items = content.as_array()?;
    let parts: Vec<&str> = items
        .iter()
        .filter_map(|item| item.get("text").and_then(|t| t.as_str()))
        .collect();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use bpa_mcp::{McpError, McpToolResult};
    use serde_json::json;
    use tokio::sync::Mutex;

    use super::*;
    use crate::mcp::test_support::{FakeCallOutcome, FakeSession};
    use crate::mcp::{McpAuthKind, McpScope, McpToolRow, McpTransport, NewMcpServer, NewMcpTool};

    /// `call_tool` now takes the SHARED `Arc<Mutex<Db>>` (it locks internally in phases around the
    /// network await — T6 review fix), so tests build one and lock it themselves for setup /
    /// assertions, exactly as `socket_server::dispatch` does with `deps.db`.
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

    async fn add_server_with_timeout_ms(db: &Arc<Mutex<Db>>, timeout_ms: i64) -> McpServerRow {
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

    async fn add_tool(db: &Arc<Mutex<Db>>, server_id: &str, name: &str) -> McpToolRow {
        let guard = db.lock().await;
        guard
            .upsert_mcp_tools(
                server_id,
                vec![NewMcpTool {
                    name: name.to_string(),
                    title: None,
                    description: None,
                    input_schema_json: "{}".to_string(),
                }],
            )
            .unwrap();
        guard
            .list_mcp_tools(server_id)
            .unwrap()
            .into_iter()
            .find(|t| t.name == name)
            .unwrap()
    }

    fn sample_result() -> McpToolResult {
        McpToolResult {
            content: json!([{"type": "text", "text": "hi"}]),
            structured: None,
            is_error: false,
            usage: None,
        }
    }

    // ---- enabled tool: success ----

    #[tokio::test]
    async fn call_tool_on_enabled_tool_writes_invocation_and_artifact() {
        let db = new_db();
        let server = add_server(&db).await;
        add_tool(&db, &server.id, "search").await;

        let call_count = Arc::new(AtomicUsize::new(0));
        let call_count_for_closure = call_count.clone();
        let connect_fn = move |_server: McpServerRow, _bearer: Option<String>| {
            let call_count = call_count_for_closure.clone();
            async move {
                Ok::<FakeSession, McpError>(
                    FakeSession::new(vec![], call_count)
                        .with_outcomes(vec![FakeCallOutcome::Ok(sample_result())]),
                )
            }
        };

        let result = call_tool(&db, &server.id, "search", "{}", None, connect_fn)
            .await
            .unwrap();

        assert!(!result.is_error);
        assert!(!result.artifact_id.is_empty());
        assert!(!result.invocation_id.is_empty());

        let invocations = db
            .lock()
            .await
            .list_invocations(Some(&server.id), None, None)
            .unwrap();
        assert_eq!(invocations.len(), 1);
        assert!(invocations[0].ok);
        assert_eq!(invocations[0].error_kind, None);
        assert_eq!(
            invocations[0].request_hash,
            sha256_hex(b"{}"),
            "request_hash is sha256(args_json), never the raw args"
        );

        let artifacts = db
            .lock()
            .await
            .list_artifacts(None, Some(&server.id), None)
            .unwrap();
        assert_eq!(artifacts.len(), 1);
        assert!(artifacts[0].is_untrusted);
        assert_eq!(artifacts[0].content_json, result.content_json);
        assert_eq!(artifacts[0].content_text.as_deref(), Some("hi"));

        assert_eq!(call_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn call_tool_ok_result_with_is_error_true_still_writes_invocation_and_artifact() {
        let db = new_db();
        let server = add_server(&db).await;
        add_tool(&db, &server.id, "search").await;

        let call_count = Arc::new(AtomicUsize::new(0));
        let call_count_for_closure = call_count.clone();
        let connect_fn = move |_server: McpServerRow, _bearer: Option<String>| {
            let call_count = call_count_for_closure.clone();
            async move {
                Ok::<FakeSession, McpError>(FakeSession::new(vec![], call_count).with_outcomes(
                    vec![FakeCallOutcome::Ok(McpToolResult {
                        content: json!([{"type": "text", "text": "boom"}]),
                        structured: None,
                        is_error: true,
                        usage: None,
                    })],
                ))
            }
        };

        let result = call_tool(&db, &server.id, "search", "{}", None, connect_fn)
            .await
            .unwrap();
        assert!(
            result.is_error,
            "tool-level failure propagates on an otherwise-successful RPC"
        );

        let invocations = db
            .lock()
            .await
            .list_invocations(Some(&server.id), None, None)
            .unwrap();
        assert!(invocations[0].ok, "the RPC itself succeeded");
        let artifacts = db
            .lock()
            .await
            .list_artifacts(None, Some(&server.id), None)
            .unwrap();
        assert_eq!(
            artifacts.len(),
            1,
            "a tool-level error result is still a durable artifact"
        );
    }

    // ---- disabled tool: denied before dispatch ----

    #[tokio::test]
    async fn call_tool_on_disabled_tool_is_denied_with_no_invocation_or_artifact() {
        let db = new_db();
        let server = add_server(&db).await;
        let tool = add_tool(&db, &server.id, "search").await;
        db.lock()
            .await
            .set_mcp_tool_enabled(&tool.id, false)
            .unwrap();

        let call_count = Arc::new(AtomicUsize::new(0));
        let call_count_for_closure = call_count.clone();
        let connect_fn = move |_server: McpServerRow, _bearer: Option<String>| {
            let call_count = call_count_for_closure.clone();
            async move { Ok::<FakeSession, McpError>(FakeSession::new(vec![], call_count)) }
        };

        let err = call_tool(&db, &server.id, "search", "{}", None, connect_fn)
            .await
            .unwrap_err();
        assert!(matches!(err, OrchdMcpError::ToolDisabled));

        assert!(db
            .lock()
            .await
            .list_invocations(Some(&server.id), None, None)
            .unwrap()
            .is_empty());
        assert!(db
            .lock()
            .await
            .list_artifacts(None, Some(&server.id), None)
            .unwrap()
            .is_empty());
        assert_eq!(
            call_count.load(Ordering::SeqCst),
            0,
            "a denied call must never dispatch to the session"
        );

        let denied: i64 = db
            .lock()
            .await
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM audit_log WHERE action='tool_call' AND decision='deny' \
                 AND reason='tool_disabled'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(denied, 1);
    }

    // ---- spend/rate policy-cap denial (task T18, spec §6/BL-22): a DIFFERENT `OrchdMcpError`
    // variant than the allowlist denial above, so the wire error message can name WHICH cap
    // tripped instead of reusing "tool disabled" ----

    #[tokio::test]
    async fn call_tool_on_a_rate_capped_server_is_denied_as_policy_cap_exceeded_with_no_dispatch() {
        let db = new_db();
        let server = add_server(&db).await;
        add_tool(&db, &server.id, "search").await;
        {
            let guard = db.lock().await;
            guard
                .upsert_policy(crate::persistence::NewPolicy {
                    scope: bpa_orchd_proto::PolicyScope::Server,
                    ref_id: Some(server.id.clone()),
                    spend_cap_usd: None,
                    rate_per_min: Some(0),
                })
                .unwrap();
        }

        let call_count = Arc::new(AtomicUsize::new(0));
        let call_count_for_closure = call_count.clone();
        let connect_fn = move |_server: McpServerRow, _bearer: Option<String>| {
            let call_count = call_count_for_closure.clone();
            async move { Ok::<FakeSession, McpError>(FakeSession::new(vec![], call_count)) }
        };

        let err = call_tool(&db, &server.id, "search", "{}", None, connect_fn)
            .await
            .unwrap_err();
        match err {
            OrchdMcpError::PolicyCapExceeded(reason) => {
                assert_eq!(reason, "rate_limit_exceeded")
            }
            other => panic!("expected PolicyCapExceeded, got {other:?}"),
        }
        assert_eq!(
            call_count.load(Ordering::SeqCst),
            0,
            "a policy-cap-denied call must never dispatch to the session"
        );
        assert!(db
            .lock()
            .await
            .list_invocations(Some(&server.id), None, None)
            .unwrap()
            .is_empty());

        let denied: i64 = db
            .lock()
            .await
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM audit_log WHERE action='policy_deny' AND decision='deny' \
                 AND reason='rate_limit_exceeded'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(denied, 1);
    }

    // ---- transport error: terminal, no artifact ----

    #[tokio::test]
    async fn call_tool_transport_error_records_failed_invocation_no_artifact() {
        let db = new_db();
        let server = add_server(&db).await; // max_retries = 2 -> 3 total attempts
        add_tool(&db, &server.id, "search").await;

        let call_count = Arc::new(AtomicUsize::new(0));
        let call_count_for_closure = call_count.clone();
        let connect_fn = move |_server: McpServerRow, _bearer: Option<String>| {
            let call_count = call_count_for_closure.clone();
            async move {
                Ok::<FakeSession, McpError>(FakeSession::new(vec![], call_count).with_outcomes(
                    vec![
                        FakeCallOutcome::Err(McpError::Transport("boom".into())),
                        FakeCallOutcome::Err(McpError::Transport("boom".into())),
                        FakeCallOutcome::Err(McpError::Transport("boom".into())),
                    ],
                ))
            }
        };

        let err = call_tool(&db, &server.id, "search", "{}", None, connect_fn)
            .await
            .unwrap_err();
        assert!(matches!(err, OrchdMcpError::Mcp(McpError::Transport(_))));

        let invocations = db
            .lock()
            .await
            .list_invocations(Some(&server.id), None, None)
            .unwrap();
        assert_eq!(invocations.len(), 1);
        assert!(!invocations[0].ok);
        assert_eq!(invocations[0].error_kind.as_deref(), Some("transport"));

        assert!(db
            .lock()
            .await
            .list_artifacts(None, Some(&server.id), None)
            .unwrap()
            .is_empty());
        assert_eq!(
            call_count.load(Ordering::SeqCst),
            3,
            "1 initial attempt + max_retries(2) retries"
        );
    }

    // ---- retry semantics ----

    #[tokio::test]
    async fn call_tool_retries_transport_error_then_succeeds() {
        let db = new_db();
        let server = add_server(&db).await;
        add_tool(&db, &server.id, "search").await;

        let call_count = Arc::new(AtomicUsize::new(0));
        let call_count_for_closure = call_count.clone();
        let connect_fn = move |_server: McpServerRow, _bearer: Option<String>| {
            let call_count = call_count_for_closure.clone();
            async move {
                Ok::<FakeSession, McpError>(FakeSession::new(vec![], call_count).with_outcomes(
                    vec![
                        FakeCallOutcome::Err(McpError::Transport("boom".into())),
                        FakeCallOutcome::Ok(sample_result()),
                    ],
                ))
            }
        };

        let result = call_tool(&db, &server.id, "search", "{}", None, connect_fn)
            .await
            .unwrap();
        assert!(!result.is_error);
        assert_eq!(call_count.load(Ordering::SeqCst), 2);

        let invocations = db
            .lock()
            .await
            .list_invocations(Some(&server.id), None, None)
            .unwrap();
        assert_eq!(invocations.len(), 1, "one row for the final outcome only");
        assert!(invocations[0].ok);
    }

    #[tokio::test]
    async fn call_tool_tool_error_is_not_retried() {
        let db = new_db();
        let server = add_server(&db).await;
        add_tool(&db, &server.id, "search").await;

        let call_count = Arc::new(AtomicUsize::new(0));
        let call_count_for_closure = call_count.clone();
        let connect_fn = move |_server: McpServerRow, _bearer: Option<String>| {
            let call_count = call_count_for_closure.clone();
            async move {
                Ok::<FakeSession, McpError>(FakeSession::new(vec![], call_count).with_outcomes(
                    vec![
                        FakeCallOutcome::Err(McpError::ToolError("unknown tool".into())),
                        // would succeed if (incorrectly) retried
                        FakeCallOutcome::Ok(sample_result()),
                    ],
                ))
            }
        };

        let err = call_tool(&db, &server.id, "search", "{}", None, connect_fn)
            .await
            .unwrap_err();
        assert!(matches!(err, OrchdMcpError::Mcp(McpError::ToolError(_))));
        assert_eq!(
            call_count.load(Ordering::SeqCst),
            1,
            "a ToolError must not be retried"
        );

        let invocations = db
            .lock()
            .await
            .list_invocations(Some(&server.id), None, None)
            .unwrap();
        assert_eq!(invocations.len(), 1);
        assert!(!invocations[0].ok);
        assert_eq!(invocations[0].error_kind.as_deref(), Some("tool_error"));
    }

    // ---- connect-step failure also records a failed invocation ----

    #[tokio::test]
    async fn call_tool_connect_failure_records_failed_invocation() {
        let db = new_db();
        let server = add_server(&db).await;
        add_tool(&db, &server.id, "search").await;

        let connect_fn = |_server: McpServerRow, _bearer: Option<String>| async move {
            Err::<FakeSession, McpError>(McpError::Auth("expired credentials".into()))
        };

        let err = call_tool(&db, &server.id, "search", "{}", None, connect_fn)
            .await
            .unwrap_err();
        assert!(matches!(err, OrchdMcpError::Mcp(McpError::Auth(_))));

        let invocations = db
            .lock()
            .await
            .list_invocations(Some(&server.id), None, None)
            .unwrap();
        assert_eq!(invocations.len(), 1);
        assert!(!invocations[0].ok);
        assert_eq!(invocations[0].error_kind.as_deref(), Some("auth"));
        assert!(db
            .lock()
            .await
            .list_artifacts(None, Some(&server.id), None)
            .unwrap()
            .is_empty());
    }

    // ---- connect handshake never resolves (S-IDEA D12): a stalled peer that accepts the
    // connection but never completes `initialize` must not hang the caller forever — the connect
    // step is bounded by `server.timeout_ms`, exactly like the `tools/call` RPC already is. ----

    #[tokio::test]
    async fn call_tool_connect_that_never_resolves_times_out_not_hangs() {
        let db = new_db();
        let server = add_server_with_timeout_ms(&db, 50).await;
        add_tool(&db, &server.id, "search").await;

        // A `connect_fn` whose future never resolves — models a peer that accepts the TCP/stdio
        // connection but stalls forever inside the MCP `initialize` round-trip.
        let connect_fn = |_server: McpServerRow, _bearer: Option<String>| {
            std::future::pending::<Result<FakeSession, McpError>>()
        };

        let err = call_tool(&db, &server.id, "search", "{}", None, connect_fn)
            .await
            .unwrap_err();
        assert!(
            matches!(err, OrchdMcpError::Mcp(McpError::Timeout)),
            "a connect that never resolves must time out as McpError::Timeout, got: {err:?}"
        );

        let invocations = db
            .lock()
            .await
            .list_invocations(Some(&server.id), None, None)
            .unwrap();
        assert_eq!(
            invocations.len(),
            1,
            "a connect-timeout is still a terminal failed attempt — exactly one invocation row"
        );
        assert!(!invocations[0].ok);
        assert_eq!(
            invocations[0].error_kind.as_deref(),
            Some("timeout"),
            "classify_error_kind must map the connect-timeout the same as a tools/call timeout"
        );
        assert!(
            db.lock()
                .await
                .list_artifacts(None, Some(&server.id), None)
                .unwrap()
                .is_empty(),
            "a connect that never produced a session must never write an artifact"
        );
    }

    // ---- stdio-exec consent gate on the PER-CALL reconnect (S-EXT §6/D6/D10, task T16, closes
    // BL-22): Phase-1 has no persisted session, so `McpCallTool` reconnects on every call — for a
    // stdio server that means a NEW process spawn per call, which must be gated exactly like the
    // explicit `McpConnect` path (`mcp::lifecycle::connect`'s own tests cover that path). ----

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
    async fn stdio_call_tool_without_stdio_exec_consent_is_denied_and_never_spawns() {
        let db = new_db();
        let server = add_stdio_server(&db, "/nonexistent/mcp-server").await;
        add_tool(&db, &server.id, "run").await;

        let call_count = Arc::new(AtomicUsize::new(0));
        let call_count_for_closure = call_count.clone();
        let connect_fn = move |_server: McpServerRow, _bearer: Option<String>| {
            let call_count = call_count_for_closure.clone();
            async move { Ok::<FakeSession, McpError>(FakeSession::new(vec![], call_count)) }
        };

        let err = call_tool(&db, &server.id, "run", "{}", None, connect_fn)
            .await
            .unwrap_err();
        assert!(
            matches!(err, OrchdMcpError::ConsentRequired),
            "an un-consented stdio tool call must deny with ConsentRequired, not dispatch: {err:?}"
        );
        assert_eq!(
            call_count.load(Ordering::SeqCst),
            0,
            "a denied stdio call must never spawn the process (connect_fn must not run)"
        );
        assert!(
            db.lock()
                .await
                .list_invocations(Some(&server.id), None, None)
                .unwrap()
                .is_empty(),
            "a spawn-denied call writes no mcp_invocation row"
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
    async fn stdio_call_tool_after_stdio_exec_consent_spawns_and_succeeds() {
        let db = new_db();
        let server = add_stdio_server(&db, "/nonexistent/mcp-server").await;
        add_tool(&db, &server.id, "run").await;
        let fingerprint = crate::trust::stdio_exec_fingerprint("/nonexistent/mcp-server", &[]);
        db.lock()
            .await
            .grant_consent(&server.id, "stdio_exec", &fingerprint)
            .unwrap();

        let call_count = Arc::new(AtomicUsize::new(0));
        let call_count_for_closure = call_count.clone();
        let connect_fn = move |_server: McpServerRow, _bearer: Option<String>| {
            let call_count = call_count_for_closure.clone();
            async move {
                Ok::<FakeSession, McpError>(
                    FakeSession::new(vec![], call_count)
                        .with_outcomes(vec![FakeCallOutcome::Ok(sample_result())]),
                )
            }
        };

        let result = call_tool(&db, &server.id, "run", "{}", None, connect_fn)
            .await
            .unwrap();
        assert!(!result.is_error);
        assert_eq!(call_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn stdio_call_tool_http_server_is_unaffected_no_extra_gate() {
        // Regression (task T16 brief: "an http server's 'connect' consent path stays
        // unchanged") — an http server's per-call reconnect has never been gated by a Connect
        // check and still isn't; only the enabled-tool allowlist applies.
        let db = new_db();
        let server = add_server(&db).await;
        add_tool(&db, &server.id, "search").await;

        let call_count = Arc::new(AtomicUsize::new(0));
        let call_count_for_closure = call_count.clone();
        let connect_fn = move |_server: McpServerRow, _bearer: Option<String>| {
            let call_count = call_count_for_closure.clone();
            async move {
                Ok::<FakeSession, McpError>(
                    FakeSession::new(vec![], call_count)
                        .with_outcomes(vec![FakeCallOutcome::Ok(sample_result())]),
                )
            }
        };

        let result = call_tool(&db, &server.id, "search", "{}", None, connect_fn)
            .await
            .unwrap();
        assert!(!result.is_error);
        assert_eq!(
            call_count.load(Ordering::SeqCst),
            1,
            "an http tool call needs no consent grant at all — unchanged behavior"
        );
    }

    // ---- helper unit tests ----

    #[test]
    fn sha256_hex_matches_known_vector() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn extract_text_flattens_text_blocks_none_when_no_text() {
        assert_eq!(
            extract_text(&json!([{"type":"text","text":"a"}, {"type":"text","text":"b"}])),
            Some("a\nb".to_string())
        );
        assert_eq!(extract_text(&json!([{"type":"image","data":"..."}])), None);
        assert_eq!(extract_text(&json!(null)), None);
    }
}
