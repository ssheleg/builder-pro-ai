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
//! `tool_call`.
//!
//! Task T16 adds [`Action::StdioSpawn`] (spec §6/D6/D10, closes BL-22): spawning a stdio MCP
//! server's process is code execution, so it is gated by a DISTINCT `stdio_exec` consent grant
//! rather than reusing `Connect`'s `'connect'` kind — a grant for a remote HTTP server's URL must
//! never double as authorization to run an arbitrary local binary. Both `mcp::lifecycle::connect`
//! (the explicit `McpConnect` verb) AND `mcp::invoke::call_tool` (Phase-1's per-call reconnect —
//! there is no persisted session to check once) authorize through this SAME action before ever
//! invoking a stdio `connect_fn`, so a spawn can never bypass the gate via either path.
//!
//! Task T18 adds spend/rate policy caps (spec §6, BL-22): an ADDITIONAL gate on the `ToolCall`/
//! `ConnectorInvoke` Allow path, checked AFTER the pre-existing per-tool-allowlist check (which
//! still denies first, unchanged) — [`check_policy_caps`] resolves the effective [`Policy`] for
//! the attempt ([`resolve_policy`]: server scope overrides project scope overrides the single
//! global scope, spec §6 "MOST-SPECIFIC-wins") and, if either configured cap is already met or
//! exceeded over the trailing [`POLICY_WINDOW_MS`] window, denies with `rate_limit_exceeded` or
//! `spend_cap_exceeded` — audited under the DISTINCT `policy_deny` action (spec §4's own
//! `audit_log.action` literal set already names it), never under `tool_call`/`connector_invoke`,
//! so an audit-log reader can tell "this call was rejected" apart from "a cap was breached" at a
//! glance (mirrors `StdioSpawn`'s own distinct-action precedent above). **Honest degradation**:
//! `mcp_invocation.cost_usd` is `NULL` unless the MCP/connector server itself reports usage (spec
//! §4) — [`Db::sum_cost_since`] coalesces that to `0.0`, so the spend cap binds ONLY once a
//! server actually reports cost. A server that never reports cost can never trip the cap; that is
//! the honest v1 behavior, not a bug — there is no reliable way to estimate an unreported cost
//! before the call completes.

use bpa_orchd_proto::{Policy, PolicyScope};

use crate::persistence::{now_ms, Db, NewAudit, OrchdPersistError};

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
/// `pub(crate)`: `mcp::invoke::call_tool` (T18) compares a `ToolCall` deny's reason against this
/// SAME literal to tell "the allowlist denied it" apart from "a policy cap denied it" — see that
/// function's own doc comment.
pub(crate) const REASON_TOOL_DISABLED: &str = "tool_disabled";
/// `audit_log` reason literal for a `ToolCall`/`ConnectorInvoke` denied because the resolved
/// effective [`Policy`]'s `rate_per_min` cap has already been met over the trailing
/// [`POLICY_WINDOW_MS`] window (task T18, spec §6, BL-22).
const REASON_RATE_LIMIT_EXCEEDED: &str = "rate_limit_exceeded";
/// `audit_log` reason literal for a `ToolCall`/`ConnectorInvoke` denied because the resolved
/// effective [`Policy`]'s `spend_cap_usd` cap has already been met over the trailing
/// [`POLICY_WINDOW_MS`] window (task T18, spec §6, BL-22). See this module's own doc comment for
/// the NULL-cost honesty note this check's binding depends on.
const REASON_SPEND_CAP_EXCEEDED: &str = "spend_cap_exceeded";

const AUDIT_ACTION_CONNECT: &str = "connect";
/// (task T16, spec §4 `audit_log.action` literal set already anticipates this exact literal) —
/// `StdioSpawn`'s audit action, distinct from `AUDIT_ACTION_CONNECT` so an stdio process spawn is
/// never indistinguishable from an http connect in the audit trail.
const AUDIT_ACTION_STDIO_SPAWN: &str = "stdio_spawn";
const AUDIT_ACTION_TOOL_CALL: &str = "tool_call";
/// (task T12, spec §4 `audit_log.action` literal set) — `ConnectorInvoke`'s audit action.
const AUDIT_ACTION_CONNECTOR_INVOKE: &str = "connector_invoke";
/// (task T18, spec §4 `audit_log.action` literal set already anticipates this exact literal) — a
/// spend/rate policy-cap denial's audit action, OVERRIDING `AUDIT_ACTION_TOOL_CALL`/
/// `AUDIT_ACTION_CONNECTOR_INVOKE` in [`write_audit`] regardless of which `Action` triggered it,
/// so a cap breach is never indistinguishable from an ordinary tool-disabled/allowed row in the
/// audit trail (mirrors `AUDIT_ACTION_STDIO_SPAWN`'s own distinct-action precedent).
const AUDIT_ACTION_POLICY_DENY: &str = "policy_deny";
const AUDIT_DECISION_ALLOW: &str = "allow";
const AUDIT_DECISION_DENY: &str = "deny";

/// Rolling window (spec §6, task T18): BOTH the rate-limit check ("count `mcp_invocation` rows
/// ... in the last 60s") and the spend-cap check ("sum `cost_usd` ... over the window") count
/// over this SAME trailing window, so the two checks can never silently drift onto different
/// windows. One documented value, per the task brief's own "pick a clear ... window" guidance.
pub(crate) const POLICY_WINDOW_MS: i64 = 60_000;

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
            project_id,
        } => {
            let tools = db.list_mcp_tools(server_id)?;
            let enabled = tools.iter().any(|t| &t.name == tool_name && t.enabled);
            if !enabled {
                return Ok(Decision::Deny {
                    reason: REASON_TOOL_DISABLED.to_string(),
                });
            }
            // (task T18) The per-tool allowlist above is UNCHANGED and still denies first — caps
            // are an ADDITIONAL gate on the Allow path (spec §6), never a replacement for it.
            check_policy_caps(db, project_id.as_deref(), Some(server_id))
        }
        // (task T12) "same policy scope as ToolCall" (task brief) — but unlike ToolCall there is
        // no per-account-op allowlist table to consult yet (no `account_op`/equivalent to
        // `mcp_tool.enabled` exists in the spec §4 schema), so there is no allowlist-style deny
        // to check first here — every `ConnectorInvoke` goes straight to the SAME spend/rate cap
        // gate `ToolCall` uses (task T18, spec §6: "connector_invoke passes through
        // trust::authorize IDENTICALLY to McpCallTool — same policy scope"). An account has no
        // `server_id` (only `account_id`, spec §4's `mcp_invocation`/`mcp_artifact` XOR) — a
        // `ConnectorInvoke` can therefore only ever resolve a project- or global-scope policy,
        // never a server-scope one (`resolve_policy`'s own doc comment covers this).
        Action::ConnectorInvoke { project_id, .. } => {
            check_policy_caps(db, project_id.as_deref(), None)
        }
    }
}

/// Resolve the effective [`Policy`] for a `ToolCall`/`ConnectorInvoke` attempt (spec §6, task
/// T18, BL-22): MOST-SPECIFIC-wins — a server-scope policy row (when `server_id` is `Some` AND a
/// row is configured for it) wins outright over a project-scope row (when `project_id` is `Some`
/// AND a row is configured for it), which wins outright over the single global-scope row (if
/// configured at all).
///
/// "Wins outright" means the WHOLE row, not a per-field merge: if the winning row leaves
/// `rate_per_min` unset, that dimension is unlimited for THIS call even if a less-specific scope
/// sets one — a per-field merge would make the effective cap depend on which OTHER scopes happen
/// to have rows configured, a much less predictable rule for an owner configuring caps in the UI
/// than "the most specific row that exists governs everything about this call."
///
/// `server_id: None` (every `ConnectorInvoke`, spec §4: an account has no server_id) simply skips
/// the server-scope check and falls through to project/global — the SAME resolution order, one
/// tier shorter. Returns `None` when no policy is configured at ANY applicable scope — the
/// honest "unbounded by default" starting state (spec §4: caps start `null` = unlimited).
fn resolve_policy(
    db: &Db,
    project_id: Option<&str>,
    server_id: Option<&str>,
) -> Result<Option<Policy>, OrchdPersistError> {
    if let Some(server_id) = server_id {
        if let Some(row) = db.get_policy(PolicyScope::Server, Some(server_id))? {
            return Ok(Some(row));
        }
    }
    if let Some(project_id) = project_id {
        if let Some(row) = db.get_policy(PolicyScope::Project, Some(project_id))? {
            return Ok(Some(row));
        }
    }
    db.get_policy(PolicyScope::Global, None)
}

/// Checks the resolved effective policy's rate/spend caps for one `ToolCall`/`ConnectorInvoke`
/// attempt, pre-dispatch (spec §6, task T18). `Decision::Allow` when no policy applies (spec §4:
/// unconfigured = unlimited) or neither configured cap is already met/exceeded; otherwise the
/// FIRST breached cap wins as a `Decision::Deny` — rate checked before spend (an arbitrary but
/// fixed order, matters only when BOTH caps would breach in the same window, so the resulting
/// audit row always cites one deterministic reason, never a coin flip between the two).
fn check_policy_caps(
    db: &Db,
    project_id: Option<&str>,
    server_id: Option<&str>,
) -> Result<Decision, OrchdPersistError> {
    let Some(policy) = resolve_policy(db, project_id, server_id)? else {
        return Ok(Decision::Allow);
    };
    let since_ms = now_ms() - POLICY_WINDOW_MS;

    if let Some(rate_per_min) = policy.rate_per_min {
        let count =
            db.count_invocations_since(policy.scope.clone(), policy.ref_id.as_deref(), since_ms)?;
        if count >= rate_per_min {
            return Ok(Decision::Deny {
                reason: REASON_RATE_LIMIT_EXCEEDED.to_string(),
            });
        }
    }
    if let Some(spend_cap_usd) = policy.spend_cap_usd {
        let spent = db.sum_cost_since(policy.scope.clone(), policy.ref_id.as_deref(), since_ms)?;
        if spent >= spend_cap_usd {
            return Ok(Decision::Deny {
                reason: REASON_SPEND_CAP_EXCEEDED.to_string(),
            });
        }
    }
    Ok(Decision::Allow)
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

    // (task T18, spec §6/BL-22): a spend/rate POLICY-CAP denial is audited under the DISTINCT
    // `policy_deny` action — never `tool_call`/`connector_invoke` — regardless of which `Action`
    // variant triggered it, so a reader can tell "this call itself was rejected" (tool_call/
    // connector_invoke + deny + tool_disabled) apart from "a cap on this scope was breached"
    // (policy_deny) at a glance. `Connect`/`StdioSpawn` denials are never reached here (their own
    // reason literals are `consent_required`/`stdio_exec_required`, never one of these two), so
    // this override can never misfire on a consent denial.
    let audit_action = if matches!(
        reason.as_deref(),
        Some(REASON_RATE_LIMIT_EXCEEDED) | Some(REASON_SPEND_CAP_EXCEEDED)
    ) {
        AUDIT_ACTION_POLICY_DENY
    } else {
        audit_action
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
pub(crate) fn stdio_exec_fingerprint(
    command: &str,
    args: &[String],
    env: &std::collections::BTreeMap<String, String>,
) -> String {
    // SEC-2: the fingerprint MUST cover command + args + env (and the resolved binary's bytes when
    // available). Previously the `bin:` scheme hashed ONLY the binary bytes — so `McpUpdateServer`
    // could rewrite `args` (e.g. `["-c","<payload>"]`) or inject `env` (`NODE_OPTIONS`, `PYTHONPATH`,
    // `BASH_ENV`…) AFTER a grant and re-use it verbatim → arbitrary code execution under a stale
    // consent. A BTreeMap iterates in sorted key order, so the hash is deterministic regardless of
    // insertion order. The `"bin:"`/`"cmd:"` prefix still distinguishes "binary resolved" from
    // "fallback to command string" and the two never collide by construction.
    let mut buf = command.as_bytes().to_vec();
    for a in args {
        buf.push(0);
        buf.extend_from_slice(a.as_bytes());
    }
    for (k, v) in env {
        buf.push(0);
        buf.extend_from_slice(k.as_bytes());
        buf.push(b'=');
        buf.extend_from_slice(v.as_bytes());
    }
    match read_resolved_binary(command) {
        Some(bytes) => {
            buf.push(0);
            buf.extend_from_slice(&bytes);
            format!("bin:{}", sha256_hex(&buf))
        }
        None => format!("cmd:{}", sha256_hex(&buf)),
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
        let fingerprint = stdio_exec_fingerprint("/nonexistent/foo", &[], &Default::default());

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
        let fingerprint = stdio_exec_fingerprint(
            "/nonexistent/foo",
            &["--flag".to_string()],
            &Default::default(),
        );
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
        let fingerprint = stdio_exec_fingerprint("/nonexistent/foo", &[], &Default::default());
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
        let fp_a = stdio_exec_fingerprint("/bin/original-tool", &[], &Default::default());
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
        let fp_b = stdio_exec_fingerprint("/bin/swapped-tool", &[], &Default::default());
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
        let a = stdio_exec_fingerprint("/nonexistent/foo", &["x".to_string()], &Default::default());
        let a_again =
            stdio_exec_fingerprint("/nonexistent/foo", &["x".to_string()], &Default::default());
        assert_eq!(
            a, a_again,
            "same command+args must hash to the same fingerprint"
        );

        let different_command =
            stdio_exec_fingerprint("/nonexistent/bar", &["x".to_string()], &Default::default());
        assert_ne!(a, different_command);

        let different_args =
            stdio_exec_fingerprint("/nonexistent/foo", &["y".to_string()], &Default::default());
        assert_ne!(a, different_args);

        // NUL-separated join: ["ab"],["c"] must not collide with ["a"],["bc"].
        let split_ab_c = stdio_exec_fingerprint(
            "cmd",
            &["ab".to_string(), "c".to_string()],
            &Default::default(),
        );
        let split_a_bc = stdio_exec_fingerprint(
            "cmd",
            &["a".to_string(), "bc".to_string()],
            &Default::default(),
        );
        assert_ne!(split_ab_c, split_a_bc);
    }

    #[test]
    fn stdio_exec_fingerprint_uses_cmd_fallback_prefix_for_an_unresolvable_command() {
        let fp = stdio_exec_fingerprint(
            "/definitely/not/a/real/path/bpa-test",
            &[],
            &Default::default(),
        );
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

        let fp_before = stdio_exec_fingerprint(command, &[], &Default::default());
        assert!(
            fp_before.starts_with("bin:"),
            "a readable file at an absolute path must use the resolved-binary scheme: {fp_before}"
        );

        std::fs::write(&path, b"swapped binary bytes (attacker payload)").unwrap();
        let fp_after = stdio_exec_fingerprint(command, &[], &Default::default());

        assert_ne!(
            fp_before, fp_after,
            "swapping the binary's bytes at the SAME path must change the fingerprint"
        );
    }

    #[test]
    fn stdio_exec_fingerprint_covers_args_and_env_so_a_stale_grant_cannot_arbitrary_exec() {
        // SEC-2: a consent grant must NOT survive a change to `args` or `env`. Previously the `bin:`
        // scheme hashed ONLY the resolved binary's bytes, so `McpUpdateServer{args:["-c","<payload>"]}`
        // (or an env injection like `NODE_OPTIONS=…`) AFTER a grant re-used it → arbitrary code
        // execution under a stale consent. Now args+env are part of every fingerprint.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp-server-bin");
        std::fs::write(&path, b"binary bytes").unwrap();
        let command = path.to_str().unwrap();

        let base = stdio_exec_fingerprint(command, &[], &Default::default());

        // An args change must change the fingerprint.
        let with_args = stdio_exec_fingerprint(
            command,
            &["-c".to_string(), "rm -rf /".to_string()],
            &Default::default(),
        );
        assert_ne!(base, with_args, "an args change must re-prompt (SEC-2)");

        // An env change must change the fingerprint (a different value AND a different key).
        let mut env_inject = std::collections::BTreeMap::new();
        env_inject.insert(
            "NODE_OPTIONS".to_string(),
            "--require /tmp/evil".to_string(),
        );
        let with_env = stdio_exec_fingerprint(command, &[], &env_inject);
        assert_ne!(base, with_env, "an env change must re-prompt (SEC-2)");

        // Two different env values for the same key must NOT collide.
        let mut env_a = std::collections::BTreeMap::new();
        env_a.insert("K".to_string(), "a".to_string());
        let mut env_b = std::collections::BTreeMap::new();
        env_b.insert("K".to_string(), "b".to_string());
        assert_ne!(
            stdio_exec_fingerprint(command, &[], &env_a),
            stdio_exec_fingerprint(command, &[], &env_b),
        );
    }

    // ---- Policy caps (task T18, spec §6, BL-22) ----

    use crate::connectors::{AccountAuthKind, NewAccount};
    use crate::persistence::{NewInvocation, NewPolicy};

    /// Inserts a real `account` row (pure DB, no Keychain — `secret_ref` is a plain fake ref
    /// string, mirrors `persistence::trust_persistence_tests::add_account`) with the given `id`,
    /// so `seed_connector_invocation`'s `mcp_invocation.account_id` FK has a row to reference.
    fn add_account(db: &Db, id: &str) {
        db.insert_account(NewAccount {
            id: id.to_string(),
            provider: "generic-rest".to_string(),
            label: "Test REST".to_string(),
            auth_kind: AccountAuthKind::Apikey,
            secret_ref: format!("{id}:apikey"),
            scopes: vec![],
            expires_at: None,
            refresh_ref: None,
        })
        .unwrap();
    }

    /// Registers `name` on `server_id` and enables it (the allowlist gate every `ToolCall` in
    /// this section must clear BEFORE the policy-cap gate is ever reached).
    fn enable_tool(db: &Db, server_id: &str, name: &str) {
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
    }

    /// Seeds one `ok=true` `mcp_invocation` row `started_at` `ms_ago` milliseconds before now,
    /// with the given `cost_usd` (`None` mirrors "the server never reported usage", spec §4) —
    /// the exact shape `check_policy_caps`'s `count_invocations_since`/`sum_cost_since` queries
    /// count/sum over.
    fn seed_invocation(
        db: &Db,
        server_id: &str,
        project_id: Option<&str>,
        cost_usd: Option<f64>,
        ms_ago: i64,
    ) {
        db.insert_invocation(NewInvocation {
            server_id: Some(server_id.to_string()),
            account_id: None,
            tool_name: "search".to_string(),
            project_id: project_id.map(str::to_string),
            request_hash: "deadbeef".to_string(),
            ok: true,
            error_kind: None,
            latency_ms: 10,
            cost_usd,
            input_tokens: None,
            output_tokens: None,
            started_at: now_ms() - ms_ago,
        })
        .unwrap();
    }

    /// Seeds a connector_invoke-shaped invocation (`account_id` set, `server_id` null — spec §4
    /// XOR), for the `ConnectorInvoke`-is-capped tests below.
    fn seed_connector_invocation(
        db: &Db,
        account_id: &str,
        project_id: Option<&str>,
        cost_usd: Option<f64>,
        ms_ago: i64,
    ) {
        db.insert_invocation(NewInvocation {
            server_id: None,
            account_id: Some(account_id.to_string()),
            tool_name: "get".to_string(),
            project_id: project_id.map(str::to_string),
            request_hash: "deadbeef".to_string(),
            ok: true,
            error_kind: None,
            latency_ms: 10,
            cost_usd,
            input_tokens: None,
            output_tokens: None,
            started_at: now_ms() - ms_ago,
        })
        .unwrap();
    }

    // ---- spend-cap breach (ToolCall) ----

    #[test]
    fn tool_call_spend_cap_breach_denies_with_audit_and_no_dispatch_implication() {
        let db = new_db();
        let server = add_server(&db);
        enable_tool(&db, &server.id, "search");
        db.upsert_policy(NewPolicy {
            scope: PolicyScope::Server,
            ref_id: Some(server.id.clone()),
            spend_cap_usd: Some(1.0),
            rate_per_min: None,
        })
        .unwrap();
        // Two prior calls this server, this window, summing to >= the $1.00 cap.
        seed_invocation(&db, &server.id, None, Some(0.6), 1_000);
        seed_invocation(&db, &server.id, None, Some(0.5), 2_000);

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
                reason: REASON_SPEND_CAP_EXCEEDED.to_string()
            }
        );
        // Audited under the DISTINCT `policy_deny` action, not `tool_call`.
        let row: (String, String, String) = db
            .conn()
            .query_row(
                "SELECT action, decision, reason FROM audit_log WHERE server_id = ?1",
                rusqlite::params![server.id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            row,
            (
                "policy_deny".to_string(),
                "deny".to_string(),
                "spend_cap_exceeded".to_string()
            )
        );
    }

    #[test]
    fn tool_call_under_spend_cap_is_allowed() {
        let db = new_db();
        let server = add_server(&db);
        enable_tool(&db, &server.id, "search");
        db.upsert_policy(NewPolicy {
            scope: PolicyScope::Server,
            ref_id: Some(server.id.clone()),
            spend_cap_usd: Some(10.0),
            rate_per_min: None,
        })
        .unwrap();
        seed_invocation(&db, &server.id, None, Some(1.0), 1_000);

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

    // ---- rate-limit (ToolCall) ----

    #[test]
    fn tool_call_rate_limit_breach_denies_with_audit() {
        let db = new_db();
        let server = add_server(&db);
        enable_tool(&db, &server.id, "search");
        db.upsert_policy(NewPolicy {
            scope: PolicyScope::Server,
            ref_id: Some(server.id.clone()),
            spend_cap_usd: None,
            rate_per_min: Some(3),
        })
        .unwrap();
        // 3 prior calls this window == the cap -> the NEXT attempt must deny.
        for i in 1..=3 {
            seed_invocation(&db, &server.id, None, None, i * 1_000);
        }

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
                reason: REASON_RATE_LIMIT_EXCEEDED.to_string()
            }
        );
        let denied: i64 = db
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

    #[test]
    fn tool_call_under_rate_limit_is_allowed() {
        let db = new_db();
        let server = add_server(&db);
        enable_tool(&db, &server.id, "search");
        db.upsert_policy(NewPolicy {
            scope: PolicyScope::Server,
            ref_id: Some(server.id.clone()),
            spend_cap_usd: None,
            rate_per_min: Some(5),
        })
        .unwrap();
        for i in 1..=2 {
            seed_invocation(&db, &server.id, None, None, i * 1_000);
        }

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
    fn tool_call_rate_limit_only_counts_calls_inside_the_window() {
        let db = new_db();
        let server = add_server(&db);
        enable_tool(&db, &server.id, "search");
        db.upsert_policy(NewPolicy {
            scope: PolicyScope::Server,
            ref_id: Some(server.id.clone()),
            spend_cap_usd: None,
            rate_per_min: Some(1),
        })
        .unwrap();
        // Outside the 60s window -> must NOT count toward the cap.
        seed_invocation(&db, &server.id, None, None, POLICY_WINDOW_MS + 5_000);

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
            Decision::Allow,
            "a call outside the rolling window must not count toward the rate cap"
        );
    }

    // ---- effective-policy resolution: server > project > global (task T18) ----

    #[test]
    fn effective_policy_prefers_server_scope_over_project_and_global() {
        let db = new_db();
        let server = add_server(&db);
        enable_tool(&db, &server.id, "search");
        // Global and project both set a rate cap of 1 (would deny after 1 prior call); the
        // server-scope row sets a MUCH higher cap of 100 and must win outright.
        db.upsert_policy(NewPolicy {
            scope: PolicyScope::Global,
            ref_id: None,
            spend_cap_usd: None,
            rate_per_min: Some(1),
        })
        .unwrap();
        db.upsert_policy(NewPolicy {
            scope: PolicyScope::Project,
            ref_id: Some("proj-1".to_string()),
            spend_cap_usd: None,
            rate_per_min: Some(1),
        })
        .unwrap();
        db.upsert_policy(NewPolicy {
            scope: PolicyScope::Server,
            ref_id: Some(server.id.clone()),
            spend_cap_usd: None,
            rate_per_min: Some(100),
        })
        .unwrap();
        seed_invocation(&db, &server.id, Some("proj-1"), None, 1_000);

        let decision = authorize(
            &db,
            &Action::ToolCall {
                server_id: server.id.clone(),
                tool_name: "search".to_string(),
                project_id: Some("proj-1".to_string()),
            },
        )
        .unwrap();
        assert_eq!(
            decision,
            Decision::Allow,
            "the server-scope cap (100) must win outright over project/global (1 each)"
        );
    }

    #[test]
    fn effective_policy_prefers_project_scope_over_global_when_no_server_scope_exists() {
        let db = new_db();
        let server = add_server(&db);
        enable_tool(&db, &server.id, "search");
        db.upsert_policy(NewPolicy {
            scope: PolicyScope::Global,
            ref_id: None,
            spend_cap_usd: None,
            rate_per_min: Some(100),
        })
        .unwrap();
        db.upsert_policy(NewPolicy {
            scope: PolicyScope::Project,
            ref_id: Some("proj-1".to_string()),
            spend_cap_usd: None,
            rate_per_min: Some(1),
        })
        .unwrap();
        seed_invocation(&db, &server.id, Some("proj-1"), None, 1_000);

        let decision = authorize(
            &db,
            &Action::ToolCall {
                server_id: server.id.clone(),
                tool_name: "search".to_string(),
                project_id: Some("proj-1".to_string()),
            },
        )
        .unwrap();
        assert_eq!(
            decision,
            Decision::Deny {
                reason: REASON_RATE_LIMIT_EXCEEDED.to_string()
            },
            "with no server-scope row, the project-scope cap (1, already met) must win over the \
             much higher global cap (100)"
        );
    }

    #[test]
    fn effective_policy_falls_back_to_global_when_no_server_or_project_scope_exists() {
        let db = new_db();
        let server = add_server(&db);
        enable_tool(&db, &server.id, "search");
        db.upsert_policy(NewPolicy {
            scope: PolicyScope::Global,
            ref_id: None,
            spend_cap_usd: None,
            rate_per_min: Some(1),
        })
        .unwrap();
        seed_invocation(&db, &server.id, None, None, 1_000);

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
                reason: REASON_RATE_LIMIT_EXCEEDED.to_string()
            }
        );
    }

    #[test]
    fn no_configured_policy_at_any_scope_is_allowed() {
        let db = new_db();
        let server = add_server(&db);
        enable_tool(&db, &server.id, "search");
        // No policy row at any scope -> unbounded (spec §4: caps default to unlimited).
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

    // ---- ConnectorInvoke is capped identically (task T18, spec §6) ----

    #[test]
    fn connector_invoke_spend_cap_breach_denies_with_policy_deny_audit() {
        let db = new_db();
        add_account(&db, "acct-1");
        db.upsert_policy(NewPolicy {
            scope: PolicyScope::Project,
            ref_id: Some("proj-1".to_string()),
            spend_cap_usd: Some(1.0),
            rate_per_min: None,
        })
        .unwrap();
        seed_connector_invocation(&db, "acct-1", Some("proj-1"), Some(1.5), 1_000);

        let decision = authorize(
            &db,
            &Action::ConnectorInvoke {
                account_id: "acct-1".to_string(),
                op: "get".to_string(),
                project_id: Some("proj-1".to_string()),
            },
        )
        .unwrap();

        assert_eq!(
            decision,
            Decision::Deny {
                reason: REASON_SPEND_CAP_EXCEEDED.to_string()
            }
        );
        let row: (String, String) = db
            .conn()
            .query_row(
                "SELECT action, reason FROM audit_log WHERE decision='deny'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            row,
            ("policy_deny".to_string(), "spend_cap_exceeded".to_string())
        );
    }

    #[test]
    fn connector_invoke_rate_limit_breach_denies() {
        let db = new_db();
        add_account(&db, "acct-1");
        db.upsert_policy(NewPolicy {
            scope: PolicyScope::Global,
            ref_id: None,
            spend_cap_usd: None,
            rate_per_min: Some(1),
        })
        .unwrap();
        seed_connector_invocation(&db, "acct-1", None, None, 1_000);

        let decision = authorize(
            &db,
            &Action::ConnectorInvoke {
                account_id: "acct-1".to_string(),
                op: "get".to_string(),
                project_id: None,
            },
        )
        .unwrap();
        assert_eq!(
            decision,
            Decision::Deny {
                reason: REASON_RATE_LIMIT_EXCEEDED.to_string()
            }
        );
    }

    #[test]
    fn connector_invoke_under_caps_is_allowed() {
        let db = new_db();
        add_account(&db, "acct-1");
        db.upsert_policy(NewPolicy {
            scope: PolicyScope::Global,
            ref_id: None,
            spend_cap_usd: Some(10.0),
            rate_per_min: Some(5),
        })
        .unwrap();
        seed_connector_invocation(&db, "acct-1", None, Some(1.0), 1_000);

        let decision = authorize(
            &db,
            &Action::ConnectorInvoke {
                account_id: "acct-1".to_string(),
                op: "get".to_string(),
                project_id: None,
            },
        )
        .unwrap();
        assert_eq!(decision, Decision::Allow);
    }

    // ---- NULL-cost honesty (task T18 brief: "caps bind only when the server reports cost") ----

    #[test]
    fn null_cost_invocations_never_trip_the_spend_cap() {
        let db = new_db();
        let server = add_server(&db);
        enable_tool(&db, &server.id, "search");
        db.upsert_policy(NewPolicy {
            scope: PolicyScope::Server,
            ref_id: Some(server.id.clone()),
            // Strictly positive so a true (COALESCE'd) zero sum does NOT trip it — proves a NULL
            // sum reads as "genuinely $0 spent", not as "meets a $0 cap" (see the companion test
            // right below for the latter, deliberately DIFFERENT case).
            spend_cap_usd: Some(0.01),
            rate_per_min: None,
        })
        .unwrap();
        // Many prior calls, NONE of which reported a cost (spec §4: cost_usd null unless the
        // server reports usage) — the honest degradation this task's brief calls out explicitly.
        for i in 1..=10 {
            seed_invocation(&db, &server.id, None, None, i * 1_000);
        }

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
            Decision::Allow,
            "NULL cost_usd must sum to 0.0, never trip a spend cap on its own — honest \
             degradation, not a bug"
        );
    }

    #[test]
    fn a_reported_cost_can_trip_even_the_tightest_zero_dollar_cap() {
        // Companion to the NULL-cost test above: proves the $0.00 cap ISN'T simply inert — it
        // trips the instant ANY cost is actually reported, distinguishing "no policy signal yet"
        // from "a real cap of zero".
        let db = new_db();
        let server = add_server(&db);
        enable_tool(&db, &server.id, "search");
        db.upsert_policy(NewPolicy {
            scope: PolicyScope::Server,
            ref_id: Some(server.id.clone()),
            spend_cap_usd: Some(0.0),
            rate_per_min: None,
        })
        .unwrap();
        seed_invocation(&db, &server.id, None, Some(0.0001), 1_000);

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
                reason: REASON_SPEND_CAP_EXCEEDED.to_string()
            }
        );
    }

    // ---- ToolCall's per-tool allowlist still denies FIRST, unaffected by policy caps (spec §6:
    // caps are an ADDITIONAL gate on the Allow path, never a replacement) ----

    #[test]
    fn a_disabled_tool_still_denies_as_tool_disabled_even_under_a_generous_policy() {
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
        db.upsert_policy(NewPolicy {
            scope: PolicyScope::Server,
            ref_id: Some(server.id.clone()),
            spend_cap_usd: Some(1_000_000.0),
            rate_per_min: Some(1_000_000),
        })
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
        assert_eq!(
            decision,
            Decision::Deny {
                reason: REASON_TOOL_DISABLED.to_string()
            },
            "the allowlist denial must win regardless of how generous the configured policy is"
        );
        // The audit action must stay `tool_call` (the allowlist denial), NEVER `policy_deny` —
        // `write_audit`'s policy_deny override only fires for the two policy-cap reasons.
        let action: String = db
            .conn()
            .query_row(
                "SELECT action FROM audit_log WHERE server_id = ?1",
                rusqlite::params![server.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(action, "tool_call");
    }
}
