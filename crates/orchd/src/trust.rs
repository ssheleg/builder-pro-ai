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
//!
//! Task T16 adds [`Action::StdioSpawn`] (spec §6/D6/D10, closes BL-22): spawning a stdio MCP
//! server's process is code execution, so it is gated by a DISTINCT `stdio_exec` consent grant
//! rather than reusing `Connect`'s `'connect'` kind — a grant for a remote HTTP server's URL must
//! never double as authorization to run an arbitrary local binary. Both `mcp::lifecycle::connect`
//! (the explicit `McpConnect` verb) AND `mcp::invoke::call_tool` (Phase-1's per-call reconnect —
//! there is no persisted session to check once) authorize through this SAME action before ever
//! invoking a stdio `connect_fn`, so a spawn can never bypass the gate via either path.

use crate::persistence::{Db, NewAudit, OrchdPersistError};

/// `consent_grant.kind` (spec §4) for a remote HTTP MCP server's first-connect consent.
pub(crate) const CONSENT_KIND_CONNECT: &str = "connect";
/// `consent_grant.kind` (spec §4) for a stdio MCP server's process-spawn consent — distinct from
/// [`CONSENT_KIND_CONNECT`] because spawning a local process is code execution (spec §6, BL-22),
/// not merely dialing a remote endpoint.
pub(crate) const CONSENT_KIND_STDIO_EXEC: &str = "stdio_exec";

/// The action being authorized (spec §6).
#[derive(Debug, Clone)]
pub enum Action {
    /// First (or repeat) connect to an MCP server. `fingerprint` is the server's URL at the
    /// time of the attempt (spec D10: "fingerprint = URL" for the http transport this Phase 1
    /// slice ships — spec D6). [`authorize`] compares it against the stored
    /// `consent_grant.fingerprint` ([`Db::get_consent`]) and re-prompts (denies with
    /// `consent_required`) on mismatch — spec D10: "re-prompt if the URL changes". This closes
    /// the credential-exfil path where a server row's `url` is repointed (via
    /// `Db::update_mcp_server`) AFTER consent was granted for a different URL: the stale existence
    /// check would otherwise still pass and `lifecycle::connect` would send the stored bearer to
    /// the new URL with no re-consent.
    Connect {
        server_id: String,
        fingerprint: String,
    },
    /// Spawning a stdio MCP server's local process (spec §6/D6/D10, task T16, closes BL-22):
    /// spawning a process from a registry entry is code execution, so it requires a DISTINCT
    /// `stdio_exec` consent grant rather than reusing `Connect`'s `'connect'` kind.
    /// `fingerprint` is [`stdio_exec_fingerprint`]'s output for the server's CURRENT
    /// `command`/`args` at the time of the attempt — [`authorize`] compares it against the
    /// stored `consent_grant.fingerprint` and re-prompts (denies) on mismatch, mirroring
    /// `Connect`'s own URL re-prompt: if the command changes (or, when the binary itself can be
    /// read, if its bytes change — a supply-chain swap at the same path) after consent was
    /// granted, the stale grant no longer authorizes the NEW spawn.
    StdioSpawn {
        server_id: String,
        fingerprint: String,
    },
    /// One `tools/call` attempt.
    ToolCall {
        server_id: String,
        tool_name: String,
        project_id: Option<String>,
    },
    /// One `ConnectorInvoke` attempt — a direct-API [`crate::connectors::adapter::ConnectorAdapter`]
    /// call (spec §6/§7, task T12). Passes through this SAME choke-point "IDENTICALLY to
    /// `McpCallTool`" (spec §6): same policy scope (spend/rate caps land in T18, exactly like
    /// `ToolCall`), a `connector_invoke` audit action, and a durable untrusted `mcp_artifact`
    /// (`is_untrusted=1`, spec D9) — `connectors::adapter::invoke` persists the invocation +
    /// artifact keyed by `account_id` (with `server_id` NULL, the schema `server_id`/`account_id`
    /// XOR), reusing the exact `mcp_invocation`/`mcp_artifact` path `McpCallTool` writes to.
    ConnectorInvoke {
        account_id: String,
        op: String,
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
/// `audit_log` reason literal for a `StdioSpawn` with no matching `stdio_exec` consent grant, or
/// a fingerprint mismatch (the command/binary changed since consent was granted) — kept distinct
/// from [`REASON_CONSENT_REQUIRED`] so an audit-log reader can tell "needs to approve a remote
/// connect" apart from "needs to approve running a local binary" at a glance.
const REASON_STDIO_EXEC_REQUIRED: &str = "stdio_exec_required";
/// `audit_log` reason literal for a `ToolCall` on a tool that is not an explicit per-server
/// allowlist member (spec §4 `mcp_tool.enabled` comment: "per-tool allowlist"). Also used for a
/// `tool_name` this server has no cached row for at all — an unrecognized tool is, by
/// definition, not on the allowlist either (fail closed, never fail open on an unknown name).
const REASON_TOOL_DISABLED: &str = "tool_disabled";

const AUDIT_ACTION_CONNECT: &str = "connect";
/// (task T16, spec §4 `audit_log.action` literal set already anticipates this exact literal) —
/// `StdioSpawn`'s audit action, distinct from `AUDIT_ACTION_CONNECT` so an stdio process spawn is
/// never indistinguishable from an http connect in the audit trail.
const AUDIT_ACTION_STDIO_SPAWN: &str = "stdio_spawn";
const AUDIT_ACTION_TOOL_CALL: &str = "tool_call";
/// (task T12, spec §4 `audit_log.action` literal set) — `ConnectorInvoke`'s audit action.
const AUDIT_ACTION_CONNECTOR_INVOKE: &str = "connector_invoke";
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
        Action::Connect {
            server_id,
            fingerprint,
        } => {
            // Consent is valid ONLY if a grant exists AND its stored fingerprint matches the
            // CURRENT connect fingerprint (the server's current URL). A grant for a different
            // URL — e.g. after the server row was repointed post-consent — is NOT valid consent
            // for this URL (spec D10: "re-prompt if the URL changes"). Same `consent_required`
            // reason either way, so dispatch maps both to the same re-prompt path.
            match db.get_consent(server_id, CONSENT_KIND_CONNECT)? {
                Some(grant) if &grant.fingerprint == fingerprint => Ok(Decision::Allow),
                _ => Ok(Decision::Deny {
                    reason: REASON_CONSENT_REQUIRED.to_string(),
                }),
            }
        }
        Action::StdioSpawn {
            server_id,
            fingerprint,
        } => {
            // Same shape as `Connect` above, but keyed on the DISTINCT `stdio_exec` consent kind
            // (spec §6/D6/D10, task T16) — a grant for this server's `connect` consent (if it
            // even has one; `scope`/`transport` make the two mutually exclusive in practice) does
            // NOT authorize spawning its process, and vice versa.
            match db.get_consent(server_id, CONSENT_KIND_STDIO_EXEC)? {
                Some(grant) if &grant.fingerprint == fingerprint => Ok(Decision::Allow),
                _ => Ok(Decision::Deny {
                    reason: REASON_STDIO_EXEC_REQUIRED.to_string(),
                }),
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
        // (task T12) "same policy scope as ToolCall" (task brief) — but unlike ToolCall there is
        // no per-account-op allowlist table to consult yet (no `account_op`/equivalent to
        // `mcp_tool.enabled` exists in the spec §4 schema): Phase 1 always allows, exactly like
        // `ToolCall` would if every tool were unconditionally enabled. Spend/rate caps (the
        // `policy` table) are T18, same as `ToolCall`'s own doc comment already states above.
        // Every call still writes the `connector_invoke` audit row below, allow or (once T18
        // lands) deny — the choke-point property this module exists for.
        Action::ConnectorInvoke { .. } => Ok(Decision::Allow),
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
        Action::StdioSpawn { server_id, .. } => (
            AUDIT_ACTION_STDIO_SPAWN,
            Some(server_id.clone()),
            None,
            None,
        ),
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
        // (task T12) `audit_log` (spec §4 DDL) has no dedicated `account_id`/`op` columns — only
        // `server_id`/`tool_name`, the two identity columns `Connect`/`ToolCall` already use. The
        // SAME DDL block that only defines those two columns also lists `'connector_invoke'` as a
        // legal `action` value, so the schema's own author already intended `server_id`/
        // `tool_name` to double as the generic "target id"/"operation name" pair for every action
        // kind, not just MCP ones. Reusing them here (account_id -> server_id, op -> tool_name)
        // is therefore the spec-intended shape, not a workaround for a frozen schema this task
        // isn't allowed to change.
        Action::ConnectorInvoke {
            account_id,
            op,
            project_id,
        } => (
            AUDIT_ACTION_CONNECTOR_INVOKE,
            Some(account_id.clone()),
            Some(op.clone()),
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

/// Compute the `stdio_exec` consent fingerprint for a stdio MCP server's `command`/`args` (spec
/// D10: "fingerprint = command + sha256 of the resolved binary"). Both the `TrustGrantConsent`
/// dispatch handler (grant time) and [`Action::StdioSpawn`]'s construction (authorize time, via
/// `mcp::connect_action`) call this SAME function on the SAME server row's CURRENT
/// `command`/`args`, so a grant and a later authorize check can never silently diverge.
///
/// Prefers hashing the ACTUAL RESOLVED BINARY's bytes (`command`, taken literally if it contains
/// a `/`, else searched for on `$PATH` — mirroring how the OS resolves a bare command name), not
/// just the command string: that's what makes this catch a supply-chain swap where the owner
/// consented to `/usr/local/bin/foo` and the file at that exact path later got replaced with a
/// different binary — the command string alone never changes, only the bytes do, and "re-prompt
/// on binary change" (spec D10) means the byte-level swap specifically, not merely the command
/// line.
///
/// Falls back to hashing `command` NUL-joined with `args` when the binary can't be resolved/read
/// at fingerprint-compute time (honest degradation, not a hard failure — e.g. consent is being
/// granted for a server whose binary isn't installed yet, or `command` is intentionally
/// PATH-relative and this daemon's `$PATH` differs between grant-time and connect-time). The
/// fallback still detects a command/args change; it just can't detect an in-place binary swap at
/// an unresolvable path. The `"bin:"`/`"cmd:"` prefix keeps the two schemes from ever colliding
/// with each other by construction.
pub(crate) fn stdio_exec_fingerprint(command: &str, args: &[String]) -> String {
    match read_resolved_binary(command) {
        Some(bytes) => format!("bin:{}", sha256_hex(&bytes)),
        None => {
            // NUL-separated so `["ab"], ["c"]` cannot collide with `["a"], ["bc"]` — a bare
            // concatenation could.
            let mut buf = command.as_bytes().to_vec();
            for a in args {
                buf.push(0);
                buf.extend_from_slice(a.as_bytes());
            }
            format!("cmd:{}", sha256_hex(&buf))
        }
    }
}

/// Resolve `command` to a file and read its bytes — `None` on any failure (not found, not a
/// regular file, permission denied, ...), which [`stdio_exec_fingerprint`] treats as "fall back
/// to the command-string scheme", never as a hard error (spec D7-style honest degradation: a
/// fingerprint must always be computable, never block the flow with an I/O error).
fn read_resolved_binary(command: &str) -> Option<Vec<u8>> {
    let path = if command.contains('/') {
        std::path::PathBuf::from(command)
    } else {
        let path_var = std::env::var_os("PATH")?;
        std::env::split_paths(&path_var)
            .map(|dir| dir.join(command))
            .find(|p| p.is_file())?
    };
    std::fs::read(&path).ok()
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::{
        McpAuthKind, McpScope, McpServerPatch, McpServerRow, McpTransport, NewMcpServer, NewMcpTool,
    };

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

    // ---- D10 fingerprint re-prompt (task-5 review: credential-exfil path) ----

    #[test]
    fn connect_after_url_change_denies_consent_required_and_audits() {
        // EXPLOIT: consent granted for url A, then the server row is repointed to url B via
        // update_mcp_server (a legitimately-patchable field). Without the fingerprint check this
        // would still Allow and lifecycle::connect would send the stored bearer to url B. With
        // the fix it must Deny{consent_required} (spec D10: re-prompt on URL change).
        let db = new_db();
        let server = add_server(&db);
        let url_a = server.url.clone().unwrap();
        db.grant_consent(&server.id, "connect", &url_a).unwrap();

        let mutated = db
            .update_mcp_server(
                &server.id,
                McpServerPatch {
                    url: Some("https://evil.example.com/mcp".to_string()),
                    ..Default::default()
                },
            )
            .unwrap();
        let url_b = mutated.url.clone().unwrap();
        assert_ne!(url_a, url_b);

        let decision = authorize(
            &db,
            &Action::Connect {
                server_id: server.id.clone(),
                fingerprint: url_b,
            },
        )
        .unwrap();

        assert_eq!(
            decision,
            Decision::Deny {
                reason: REASON_CONSENT_REQUIRED.to_string()
            },
            "consent for url A must not authorize a connect to url B"
        );
        assert_eq!(
            audit_count(&db, &server.id, "deny", Some("consent_required")),
            1
        );
    }

    #[test]
    fn connect_with_unchanged_url_after_consent_is_allowed() {
        let db = new_db();
        let server = add_server(&db);
        let url = server.url.clone().unwrap();
        db.grant_consent(&server.id, "connect", &url).unwrap();

        let decision = authorize(
            &db,
            &Action::Connect {
                server_id: server.id.clone(),
                fingerprint: url,
            },
        )
        .unwrap();
        assert_eq!(decision, Decision::Allow);
    }

    #[test]
    fn connect_after_re_grant_at_new_url_is_allowed_again() {
        // Re-consent restores access: after the owner grants consent for the NEW url, a connect
        // to that url is allowed. Relies on grant_consent's UPSERT (UNIQUE(server_id, kind)).
        let db = new_db();
        let server = add_server(&db);
        db.grant_consent(&server.id, "connect", &server.url.clone().unwrap())
            .unwrap();

        let mutated = db
            .update_mcp_server(
                &server.id,
                McpServerPatch {
                    url: Some("https://new.example.com/mcp".to_string()),
                    ..Default::default()
                },
            )
            .unwrap();
        let url_b = mutated.url.clone().unwrap();

        // still denied until re-consent
        assert_eq!(
            authorize(
                &db,
                &Action::Connect {
                    server_id: server.id.clone(),
                    fingerprint: url_b.clone(),
                },
            )
            .unwrap(),
            Decision::Deny {
                reason: REASON_CONSENT_REQUIRED.to_string()
            }
        );

        // owner re-grants for the new url
        db.grant_consent(&server.id, "connect", &url_b).unwrap();
        assert_eq!(
            authorize(
                &db,
                &Action::Connect {
                    server_id: server.id.clone(),
                    fingerprint: url_b,
                },
            )
            .unwrap(),
            Decision::Allow
        );
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

    // ---- ConnectorInvoke (task T12) ----

    #[test]
    fn connector_invoke_is_allowed_and_audits_with_account_id_and_op() {
        // Phase 1 has no per-account-op allowlist (spec §4 has no such table) — unconditional
        // Allow, same as `ToolCall` would be if every tool were enabled. The connector_invoke
        // audit row is the load-bearing assertion here (spec §6: "every ... connector_invoke ...
        // appends an audit_log row").
        let db = new_db();

        let decision = authorize(
            &db,
            &Action::ConnectorInvoke {
                account_id: "acct-123".to_string(),
                op: "get".to_string(),
                project_id: None,
            },
        )
        .unwrap();

        assert_eq!(decision, Decision::Allow);

        let row: (String, String, String, String) = db
            .conn()
            .query_row(
                "SELECT action, server_id, tool_name, decision FROM audit_log \
                 WHERE action = 'connector_invoke'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(
            row,
            (
                "connector_invoke".to_string(),
                "acct-123".to_string(),
                "get".to_string(),
                "allow".to_string(),
            ),
            "account_id/op are carried in the reused server_id/tool_name columns (see write_audit's doc comment)"
        );
    }

    #[test]
    fn connector_invoke_carries_project_id_into_the_audit_row() {
        let db = new_db();

        authorize(
            &db,
            &Action::ConnectorInvoke {
                account_id: "acct-456".to_string(),
                op: "post".to_string(),
                project_id: Some("proj-1".to_string()),
            },
        )
        .unwrap();

        let project_id: String = db
            .conn()
            .query_row(
                "SELECT project_id FROM audit_log WHERE action = 'connector_invoke'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(project_id, "proj-1");
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

    // ---- StdioSpawn (task T16, S-EXT §6/D6/D10, closes BL-22) ----

    fn add_stdio_server(db: &Db, command: &str, args: Vec<String>) -> McpServerRow {
        db.add_mcp_server(NewMcpServer {
            name: "local-mcp".to_string(),
            transport: McpTransport::Stdio,
            url: None,
            command: Some(command.to_string()),
            args,
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

    #[test]
    fn stdio_spawn_without_consent_denies_and_audits_as_stdio_spawn() {
        let db = new_db();
        let server = add_stdio_server(&db, "/nonexistent/foo", vec![]);
        let fingerprint = stdio_exec_fingerprint("/nonexistent/foo", &[]);

        let decision = authorize(
            &db,
            &Action::StdioSpawn {
                server_id: server.id.clone(),
                fingerprint,
            },
        )
        .unwrap();

        assert_eq!(
            decision,
            Decision::Deny {
                reason: REASON_STDIO_EXEC_REQUIRED.to_string()
            }
        );

        let row: (String, String) = db
            .conn()
            .query_row(
                "SELECT action, decision FROM audit_log WHERE server_id = ?1",
                rusqlite::params![server.id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            row,
            ("stdio_spawn".to_string(), "deny".to_string()),
            "the audit action for a stdio connect must read 'stdio_spawn', never 'connect'"
        );
    }

    #[test]
    fn stdio_spawn_after_stdio_exec_consent_is_allowed_and_audits() {
        let db = new_db();
        let server = add_stdio_server(&db, "/nonexistent/foo", vec!["--flag".to_string()]);
        let fingerprint = stdio_exec_fingerprint("/nonexistent/foo", &["--flag".to_string()]);
        db.grant_consent(&server.id, CONSENT_KIND_STDIO_EXEC, &fingerprint)
            .unwrap();

        let decision = authorize(
            &db,
            &Action::StdioSpawn {
                server_id: server.id.clone(),
                fingerprint,
            },
        )
        .unwrap();

        assert_eq!(decision, Decision::Allow);
        let allowed: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM audit_log WHERE action='stdio_spawn' AND decision='allow'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(allowed, 1);
    }

    #[test]
    fn a_connect_consent_grant_does_not_authorize_a_stdio_spawn() {
        // The two consent kinds are namespaced separately (spec §6/D10): granting 'connect' for
        // an (unrelated) fingerprint string must not satisfy a 'stdio_exec' check even if the
        // fingerprint text happened to match by coincidence.
        let db = new_db();
        let server = add_stdio_server(&db, "/nonexistent/foo", vec![]);
        let fingerprint = stdio_exec_fingerprint("/nonexistent/foo", &[]);
        db.grant_consent(&server.id, CONSENT_KIND_CONNECT, &fingerprint)
            .unwrap();

        let decision = authorize(
            &db,
            &Action::StdioSpawn {
                server_id: server.id.clone(),
                fingerprint,
            },
        )
        .unwrap();

        assert_eq!(
            decision,
            Decision::Deny {
                reason: REASON_STDIO_EXEC_REQUIRED.to_string()
            }
        );
    }

    #[test]
    fn stdio_spawn_after_command_change_denies_stdio_exec_required_reprompt() {
        // EXPLOIT mirror of the http url-repointing test above: consent granted for command A,
        // the server row is repointed to command B via update_mcp_server, then a spawn attempt
        // with command B's fingerprint. Without the fingerprint check this would still Allow and
        // a DIFFERENT binary would run under a stale grant.
        let db = new_db();
        let server = add_stdio_server(&db, "/bin/original-tool", vec![]);
        let fp_a = stdio_exec_fingerprint("/bin/original-tool", &[]);
        db.grant_consent(&server.id, CONSENT_KIND_STDIO_EXEC, &fp_a)
            .unwrap();

        db.update_mcp_server(
            &server.id,
            McpServerPatch {
                command: Some("/bin/swapped-tool".to_string()),
                ..Default::default()
            },
        )
        .unwrap();
        let fp_b = stdio_exec_fingerprint("/bin/swapped-tool", &[]);
        assert_ne!(fp_a, fp_b);

        let decision = authorize(
            &db,
            &Action::StdioSpawn {
                server_id: server.id.clone(),
                fingerprint: fp_b,
            },
        )
        .unwrap();

        assert_eq!(
            decision,
            Decision::Deny {
                reason: REASON_STDIO_EXEC_REQUIRED.to_string()
            },
            "consent for command A must not authorize a spawn of command B"
        );
    }

    // ---- stdio_exec_fingerprint (task T16) ----

    #[test]
    fn stdio_exec_fingerprint_fallback_is_deterministic_and_distinguishes_command_and_args() {
        let a = stdio_exec_fingerprint("/nonexistent/foo", &["x".to_string()]);
        let a_again = stdio_exec_fingerprint("/nonexistent/foo", &["x".to_string()]);
        assert_eq!(
            a, a_again,
            "same command+args must hash to the same fingerprint"
        );

        let different_command = stdio_exec_fingerprint("/nonexistent/bar", &["x".to_string()]);
        assert_ne!(a, different_command);

        let different_args = stdio_exec_fingerprint("/nonexistent/foo", &["y".to_string()]);
        assert_ne!(a, different_args);

        // NUL-separated join: ["ab"],["c"] must not collide with ["a"],["bc"].
        let split_ab_c = stdio_exec_fingerprint("cmd", &["ab".to_string(), "c".to_string()]);
        let split_a_bc = stdio_exec_fingerprint("cmd", &["a".to_string(), "bc".to_string()]);
        assert_ne!(split_ab_c, split_a_bc);
    }

    #[test]
    fn stdio_exec_fingerprint_uses_cmd_fallback_prefix_for_an_unresolvable_command() {
        let fp = stdio_exec_fingerprint("/definitely/not/a/real/path/bpa-test", &[]);
        assert!(
            fp.starts_with("cmd:"),
            "an unresolvable command must use the command-string fallback scheme: {fp}"
        );
    }

    #[test]
    fn stdio_exec_fingerprint_hashes_the_actual_resolved_binary_bytes() {
        // Proves the "ideal" scheme (spec D10: "sha256 of the resolved binary") actually detects
        // a supply-chain swap at the SAME path: same absolute path, different file content ⇒
        // different fingerprint, even though the command string never changed.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp-server-bin");
        std::fs::write(&path, b"original binary bytes").unwrap();
        let command = path.to_str().unwrap();

        let fp_before = stdio_exec_fingerprint(command, &[]);
        assert!(
            fp_before.starts_with("bin:"),
            "a readable file at an absolute path must use the resolved-binary scheme: {fp_before}"
        );

        std::fs::write(&path, b"swapped binary bytes (attacker payload)").unwrap();
        let fp_after = stdio_exec_fingerprint(command, &[]);

        assert_ne!(
            fp_before, fp_after,
            "swapping the binary's bytes at the SAME path must change the fingerprint"
        );
    }
}
