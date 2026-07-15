//! Trust choke-point (S-EXT spec §6, D10, task T5): the single pre-dispatch gate every MCP
//! connect / tool-call passes through before `mcp::lifecycle::connect`/`mcp::invoke::call_tool`
//! do anything else. [`authorize`] ALWAYS writes an `audit_log` row — allow AND deny — before
//! returning: a caller can never observe a [`Decision`] without a corresponding audit trail
//! (spec §6: "every connect / spawn / call / connector_invoke / consent / deny appends an
//! `audit_log` row"). That row NEVER carries secrets or tool-call arguments — only the fixed
//! action/decision/reason vocabulary below (request content lives in `mcp_invocation.
//! request_hash`, a sha256, never here).
//!
//! Phase-1 policy scope (task-5 brief): consent-gated connect + per-tool allowlist on
//! `tool_call`. Spend/rate caps (the `policy` table) are S-EXT T18 — not implemented here.

use crate::persistence::{Db, NewAudit, OrchdPersistError};

/// The action being authorized (spec §6).
#[derive(Debug, Clone)]
pub enum Action {
    /// First (or repeat) connect to an MCP server. `fingerprint` is the server's URL at the
    /// time of the attempt (spec D10: "fingerprint = URL" for the http transport this Phase 1
    /// slice ships — spec D6). It is threaded through so a future task can compare it against
    /// the granted `consent_grant.fingerprint` and re-prompt on mismatch (spec D10: "re-prompt
    /// if the URL changes") — [`authorize`]'s own check today is existence-only
    /// ([`Db::has_consent`]), NOT fingerprint-comparing; that re-prompt behavior is not
    /// implemented by this task.
    Connect {
        server_id: String,
        fingerprint: String,
    },
    /// One `tools/call` attempt.
    ToolCall {
        server_id: String,
        tool_name: String,
        project_id: Option<String>,
    },
}

/// The choke-point's verdict (spec §6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny { reason: String },
}

/// `consent_grant`/`audit_log` reason literal for a `Connect` with no matching consent grant
/// (spec §4 `consent_grant.kind = 'connect'`).
const REASON_CONSENT_REQUIRED: &str = "consent_required";
/// `audit_log` reason literal for a `ToolCall` on a tool that is not an explicit per-server
/// allowlist member (spec §4 `mcp_tool.enabled` comment: "per-tool allowlist"). Also used for a
/// `tool_name` this server has no cached row for at all — an unrecognized tool is, by
/// definition, not on the allowlist either (fail closed, never fail open on an unknown name).
const REASON_TOOL_DISABLED: &str = "tool_disabled";

const AUDIT_ACTION_CONNECT: &str = "connect";
const AUDIT_ACTION_TOOL_CALL: &str = "tool_call";
const AUDIT_DECISION_ALLOW: &str = "allow";
const AUDIT_DECISION_DENY: &str = "deny";

/// Single choke-point: every connect / tool-call passes through here before dispatch (spec
/// D10). ALWAYS writes an `audit_log` row, allow or deny, before returning.
pub fn authorize(db: &Db, action: &Action) -> Result<Decision, OrchdPersistError> {
    let decision = evaluate(db, action)?;
    write_audit(db, action, &decision)?;
    Ok(decision)
}

fn evaluate(db: &Db, action: &Action) -> Result<Decision, OrchdPersistError> {
    match action {
        Action::Connect { server_id, .. } => {
            if db.has_consent(server_id, "connect")? {
                Ok(Decision::Allow)
            } else {
                Ok(Decision::Deny {
                    reason: REASON_CONSENT_REQUIRED.to_string(),
                })
            }
        }
        Action::ToolCall {
            server_id,
            tool_name,
            ..
        } => {
            let tools = db.list_mcp_tools(server_id)?;
            let enabled = tools.iter().any(|t| &t.name == tool_name && t.enabled);
            if enabled {
                Ok(Decision::Allow)
            } else {
                Ok(Decision::Deny {
                    reason: REASON_TOOL_DISABLED.to_string(),
                })
            }
        }
    }
}

fn write_audit(db: &Db, action: &Action, decision: &Decision) -> Result<(), OrchdPersistError> {
    let (audit_action, server_id, tool_name, project_id): (
        &str,
        Option<String>,
        Option<String>,
        Option<String>,
    ) = match action {
        Action::Connect { server_id, .. } => {
            (AUDIT_ACTION_CONNECT, Some(server_id.clone()), None, None)
        }
        Action::ToolCall {
            server_id,
            tool_name,
            project_id,
        } => (
            AUDIT_ACTION_TOOL_CALL,
            Some(server_id.clone()),
            Some(tool_name.clone()),
            project_id.clone(),
        ),
    };
    let (decision_text, reason) = match decision {
        Decision::Allow => (AUDIT_DECISION_ALLOW, None),
        Decision::Deny { reason } => (AUDIT_DECISION_DENY, Some(reason.clone())),
    };

    // Structured tracing on every decision (task-5 brief: "server id, tool name, decision — but
    // NEVER the bearer, args, or result content"). Every field logged here is one of those
    // safe, non-secret identifiers.
    tracing::info!(
        action = audit_action,
        server_id = server_id.as_deref(),
        tool_name = tool_name.as_deref(),
        decision = decision_text,
        reason = reason.as_deref(),
        "trust: authorize"
    );

    db.insert_audit(NewAudit {
        action: audit_action.to_string(),
        server_id,
        tool_name,
        project_id,
        decision: decision_text.to_string(),
        reason,
        invocation_id: None,
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::{McpAuthKind, McpScope, McpServerRow, McpTransport, NewMcpServer, NewMcpTool};

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
            timeout_ms: 30_000,
            max_retries: 2,
        })
        .unwrap()
    }

    fn audit_count(db: &Db, server_id: &str, decision: &str, reason: Option<&str>) -> i64 {
        db.conn()
            .query_row(
                "SELECT COUNT(*) FROM audit_log
                 WHERE server_id = ?1 AND decision = ?2
                   AND ((?3 IS NULL AND reason IS NULL) OR reason = ?3)",
                rusqlite::params![server_id, decision, reason],
                |r| r.get(0),
            )
            .unwrap()
    }

    // ---- Connect ----

    #[test]
    fn connect_without_consent_denies_and_audits() {
        let db = new_db();
        let server = add_server(&db);

        let decision = authorize(
            &db,
            &Action::Connect {
                server_id: server.id.clone(),
                fingerprint: server.url.clone().unwrap(),
            },
        )
        .unwrap();

        assert_eq!(
            decision,
            Decision::Deny {
                reason: REASON_CONSENT_REQUIRED.to_string()
            }
        );
        assert_eq!(
            audit_count(&db, &server.id, "deny", Some("consent_required")),
            1
        );
    }

    #[test]
    fn connect_after_consent_is_allowed_and_audits() {
        let db = new_db();
        let server = add_server(&db);
        db.grant_consent(&server.id, "connect", &server.url.clone().unwrap())
            .unwrap();

        let decision = authorize(
            &db,
            &Action::Connect {
                server_id: server.id.clone(),
                fingerprint: server.url.clone().unwrap(),
            },
        )
        .unwrap();

        assert_eq!(decision, Decision::Allow);
        assert_eq!(audit_count(&db, &server.id, "allow", None), 1);
    }

    // ---- ToolCall ----

    #[test]
    fn tool_call_on_enabled_tool_is_allowed() {
        let db = new_db();
        let server = add_server(&db);
        db.upsert_mcp_tools(
            &server.id,
            vec![NewMcpTool {
                name: "search".to_string(),
                title: None,
                description: None,
                input_schema_json: "{}".to_string(),
            }],
        )
        .unwrap();

        let decision = authorize(
            &db,
            &Action::ToolCall {
                server_id: server.id.clone(),
                tool_name: "search".to_string(),
                project_id: None,
            },
        )
        .unwrap();

        assert_eq!(decision, Decision::Allow);
    }

    #[test]
    fn tool_call_on_disabled_tool_denies_and_audits() {
        let db = new_db();
        let server = add_server(&db);
        db.upsert_mcp_tools(
            &server.id,
            vec![NewMcpTool {
                name: "search".to_string(),
                title: None,
                description: None,
                input_schema_json: "{}".to_string(),
            }],
        )
        .unwrap();
        let tool = db.list_mcp_tools(&server.id).unwrap().remove(0);
        db.set_mcp_tool_enabled(&tool.id, false).unwrap();

        let decision = authorize(
            &db,
            &Action::ToolCall {
                server_id: server.id.clone(),
                tool_name: "search".to_string(),
                project_id: None,
            },
        )
        .unwrap();

        assert_eq!(
            decision,
            Decision::Deny {
                reason: REASON_TOOL_DISABLED.to_string()
            }
        );
        assert_eq!(
            audit_count(&db, &server.id, "deny", Some("tool_disabled")),
            1
        );
    }

    #[test]
    fn tool_call_on_unrecognized_tool_name_denies_as_tool_disabled() {
        let db = new_db();
        let server = add_server(&db);
        // no upsert_mcp_tools at all — the server was never connected/cached.

        let decision = authorize(
            &db,
            &Action::ToolCall {
                server_id: server.id.clone(),
                tool_name: "does-not-exist".to_string(),
                project_id: None,
            },
        )
        .unwrap();

        assert_eq!(
            decision,
            Decision::Deny {
                reason: REASON_TOOL_DISABLED.to_string()
            }
        );
    }

    #[test]
    fn every_authorize_call_writes_exactly_one_audit_row() {
        let db = new_db();
        let server = add_server(&db);

        authorize(
            &db,
            &Action::Connect {
                server_id: server.id.clone(),
                fingerprint: server.url.clone().unwrap(),
            },
        )
        .unwrap();
        db.grant_consent(&server.id, "connect", &server.url.clone().unwrap())
            .unwrap();
        authorize(
            &db,
            &Action::Connect {
                server_id: server.id.clone(),
                fingerprint: server.url.clone().unwrap(),
            },
        )
        .unwrap();

        let total: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM audit_log", [], |r| r.get(0))
            .unwrap();
        assert_eq!(total, 2, "one deny + one allow, no missing/duplicate rows");
    }
}
