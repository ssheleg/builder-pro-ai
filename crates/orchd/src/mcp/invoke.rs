//! `tools/call` invocation (S-EXT spec §4/§6/§7, D7/D8/D9, task T5): trust-gated, timeout +
//! transport-only retry, `mcp_invocation` + `mcp_artifact` persistence. See [`super::ToolCaller`]
//! for the test/production session seam.

use std::future::Future;
use std::time::{Duration, Instant};

use bpa_orchd_proto::McpCallResult;
use sha2::{Digest, Sha256};

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
pub async fn call_tool<F, Fut, S>(
    db: &Db,
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
    let server = db.get_mcp_server(server_id)?;

    let decision = trust::authorize(
        db,
        &Action::ToolCall {
            server_id: server.id.clone(),
            tool_name: tool_name.to_string(),
            project_id: project_id.clone(),
        },
    )?;
    if matches!(decision, Decision::Deny { .. }) {
        return Err(OrchdMcpError::ToolDisabled);
    }

    let args: serde_json::Value = serde_json::from_str(args_json).map_err(|e| {
        OrchdMcpError::Mcp(bpa_mcp::McpError::Protocol(format!(
            "McpCallTool.args_json is not valid JSON: {e}"
        )))
    })?;
    let request_hash = sha256_hex(args_json.as_bytes());

    let bearer = resolve_bearer(&server).map_err(|e| OrchdMcpError::Secret(e.to_string()))?;
    let timeout = Duration::from_millis(server.timeout_ms.max(0) as u64);
    let max_retries = server.max_retries.max(0) as u32;
    let server_id_owned = server.id.clone();

    let started_at = now_ms();
    let start = Instant::now();

    let session = match connect_fn(server, bearer).await {
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
            record_failed_invocation(db, new, &e)?;
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

    match outcome {
        Ok(tool_result) => {
            let usage = tool_result.usage;
            let is_error = tool_result.is_error;
            let content_json = serde_json::to_string(&tool_result.content).map_err(|e| {
                OrchdMcpError::Mcp(bpa_mcp::McpError::Protocol(format!(
                    "failed to serialize tool result content: {e}"
                )))
            })?;
            let content_text = extract_text(&tool_result.content);

            let invocation = db.insert_invocation(NewInvocation {
                server_id: server_id_owned.clone(),
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

            let artifact = db.insert_artifact(NewArtifact {
                invocation_id: invocation.id.clone(),
                server_id: server_id_owned.clone(),
                tool_name: tool_name.to_string(),
                project_id,
                content_json: content_json.clone(),
                content_text,
            })?;

            tracing::info!(
                server_id = %server_id_owned,
                tool_name,
                ok = true,
                latency_ms = elapsed.as_millis() as i64,
                "mcp: tool call completed"
            );

            Ok(McpCallResult {
                artifact_id: artifact.id,
                invocation_id: invocation.id,
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
            record_failed_invocation(db, new, &e)?;
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
        server_id: server_id.to_string(),
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
/// `Auth`, echo server-supplied content derived from the call).
fn record_failed_invocation(
    db: &Db,
    new: NewInvocation,
    err: &bpa_mcp::McpError,
) -> Result<(), OrchdMcpError> {
    let error_kind = classify_error_kind(err);
    tracing::warn!(
        server_id = %new.server_id,
        tool_name = %new.tool_name,
        ok = false,
        error_kind,
        "mcp: tool call failed"
    );
    db.insert_invocation(new)?;
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

    use super::*;
    use crate::mcp::test_support::{FakeCallOutcome, FakeSession};
    use crate::mcp::{McpAuthKind, McpScope, McpToolRow, McpTransport, NewMcpServer, NewMcpTool};

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

    fn add_tool(db: &Db, server_id: &str, name: &str) -> McpToolRow {
        db.upsert_mcp_tools(
            server_id,
            vec![NewMcpTool {
                name: name.to_string(),
                title: None,
                description: None,
                input_schema_json: "{}".to_string(),
            }],
        )
        .unwrap();
        db.list_mcp_tools(server_id)
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
        let server = add_server(&db);
        add_tool(&db, &server.id, "search");

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

        let invocations = db.list_invocations(Some(&server.id), None, None).unwrap();
        assert_eq!(invocations.len(), 1);
        assert!(invocations[0].ok);
        assert_eq!(invocations[0].error_kind, None);
        assert_eq!(
            invocations[0].request_hash,
            sha256_hex(b"{}"),
            "request_hash is sha256(args_json), never the raw args"
        );

        let artifacts = db.list_artifacts(None, Some(&server.id), None).unwrap();
        assert_eq!(artifacts.len(), 1);
        assert!(artifacts[0].is_untrusted);
        assert_eq!(artifacts[0].content_json, result.content_json);
        assert_eq!(artifacts[0].content_text.as_deref(), Some("hi"));

        assert_eq!(call_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn call_tool_ok_result_with_is_error_true_still_writes_invocation_and_artifact() {
        let db = new_db();
        let server = add_server(&db);
        add_tool(&db, &server.id, "search");

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

        let invocations = db.list_invocations(Some(&server.id), None, None).unwrap();
        assert!(invocations[0].ok, "the RPC itself succeeded");
        let artifacts = db.list_artifacts(None, Some(&server.id), None).unwrap();
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
        let server = add_server(&db);
        let tool = add_tool(&db, &server.id, "search");
        db.set_mcp_tool_enabled(&tool.id, false).unwrap();

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
            .list_invocations(Some(&server.id), None, None)
            .unwrap()
            .is_empty());
        assert!(db
            .list_artifacts(None, Some(&server.id), None)
            .unwrap()
            .is_empty());
        assert_eq!(
            call_count.load(Ordering::SeqCst),
            0,
            "a denied call must never dispatch to the session"
        );

        let denied: i64 = db
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

    // ---- transport error: terminal, no artifact ----

    #[tokio::test]
    async fn call_tool_transport_error_records_failed_invocation_no_artifact() {
        let db = new_db();
        let server = add_server(&db); // max_retries = 2 -> 3 total attempts
        add_tool(&db, &server.id, "search");

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

        let invocations = db.list_invocations(Some(&server.id), None, None).unwrap();
        assert_eq!(invocations.len(), 1);
        assert!(!invocations[0].ok);
        assert_eq!(invocations[0].error_kind.as_deref(), Some("transport"));

        assert!(db
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
        let server = add_server(&db);
        add_tool(&db, &server.id, "search");

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

        let invocations = db.list_invocations(Some(&server.id), None, None).unwrap();
        assert_eq!(invocations.len(), 1, "one row for the final outcome only");
        assert!(invocations[0].ok);
    }

    #[tokio::test]
    async fn call_tool_tool_error_is_not_retried() {
        let db = new_db();
        let server = add_server(&db);
        add_tool(&db, &server.id, "search");

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

        let invocations = db.list_invocations(Some(&server.id), None, None).unwrap();
        assert_eq!(invocations.len(), 1);
        assert!(!invocations[0].ok);
        assert_eq!(invocations[0].error_kind.as_deref(), Some("tool_error"));
    }

    // ---- connect-step failure also records a failed invocation ----

    #[tokio::test]
    async fn call_tool_connect_failure_records_failed_invocation() {
        let db = new_db();
        let server = add_server(&db);
        add_tool(&db, &server.id, "search");

        let connect_fn = |_server: McpServerRow, _bearer: Option<String>| async move {
            Err::<FakeSession, McpError>(McpError::Auth("expired credentials".into()))
        };

        let err = call_tool(&db, &server.id, "search", "{}", None, connect_fn)
            .await
            .unwrap_err();
        assert!(matches!(err, OrchdMcpError::Mcp(McpError::Auth(_))));

        let invocations = db.list_invocations(Some(&server.id), None, None).unwrap();
        assert_eq!(invocations.len(), 1);
        assert!(!invocations[0].ok);
        assert_eq!(invocations[0].error_kind.as_deref(), Some("auth"));
        assert!(db
            .list_artifacts(None, Some(&server.id), None)
            .unwrap()
            .is_empty());
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
