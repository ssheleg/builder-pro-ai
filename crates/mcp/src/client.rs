//! [`McpSession`]: a thin wrapper over one `rmcp` `RunningService<RoleClient, ()>` (S-EXT §3,
//! D2). This file — together with [`crate::transport`] — is the only place in `bpa-mcp` that
//! touches `rmcp::service`/`rmcp::ServiceExt` directly. Everything downstream (orchd, T5+) only
//! ever sees [`crate::TransportConfig`], [`McpSession`], [`crate::McpTool`],
//! [`crate::McpToolResult`] and [`crate::McpError`].
//!
//! The unit `()` client handler (`RunningService<RoleClient, ()>`) means every `rmcp`
//! `ClientHandler` callback — including `on_tool_list_changed` — is the trait's no-op default;
//! no server-pushed `notifications/tools/list_changed` is ever observed here. This is DELIBERATE
//! for Phase 1's connect-per-call architecture, not an oversight — see
//! `bpa_orchd::mcp::cache`'s own module doc comment (task T18) for the full honest rationale and
//! the tracked follow-up (BL-70).

use rmcp::model::CallToolRequestParams;
use rmcp::service::RunningService;
use rmcp::{RoleClient, ServiceExt};
use serde_json::Value;

use crate::error::{map_connect_err, map_service_err, map_spawn_err, McpError};
use crate::transport::{build_http_transport, build_stdio_transport, TransportConfig};
use crate::types::{map_call_result, map_tool, McpTool, McpToolResult};

/// A live connection to one MCP server (spec §3). The only ways to obtain one are [`connect`]
/// (production: Streamable HTTP) and, behind the `test-support` feature, `connect_duplex` (an
/// in-memory `tokio::io::duplex()` pair, used by this crate's own stub-server tests).
pub struct McpSession {
    inner: RunningService<RoleClient, ()>,
}

impl McpSession {
    fn from_running(inner: RunningService<RoleClient, ()>) -> Self {
        Self { inner }
    }

    /// List every tool the connected server currently advertises. Paginates internally
    /// (`rmcp`'s `list_all_tools`) so callers always get the full set in one call.
    pub async fn list_tools(&self) -> Result<Vec<McpTool>, McpError> {
        let tools = self.inner.list_all_tools().await.map_err(map_service_err)?;
        Ok(tools.into_iter().map(map_tool).collect())
    }

    /// Invoke `name` with `args`. `args` must be a JSON object (the tool's keyword arguments)
    /// or `Value::Null` for a tool that takes none; any other JSON shape is rejected locally
    /// before it ever reaches the server.
    pub async fn call_tool(&self, name: &str, args: Value) -> Result<McpToolResult, McpError> {
        let mut params = CallToolRequestParams::new(name.to_string());
        match args {
            Value::Null => {}
            Value::Object(map) => params = params.with_arguments(map),
            other => {
                return Err(McpError::Protocol(format!(
                    "tools/call arguments must be a JSON object (or null for no arguments), \
                     got: {other}"
                )));
            }
        }

        let result = self
            .inner
            .call_tool(params)
            .await
            .map_err(map_service_err)?;
        map_call_result(result)
    }

    /// The MCP protocol version negotiated with the server during `initialize` (spec D3).
    pub fn protocol_version(&self) -> String {
        self.inner
            .peer_info()
            .map(|info| info.protocol_version.to_string())
            .unwrap_or_else(|| rmcp::model::ProtocolVersion::LATEST.to_string())
    }

    /// Gracefully close the connection. Safe to call more than once.
    pub async fn close(&mut self) -> Result<(), McpError> {
        self.inner
            .close()
            .await
            .map(|_quit_reason| ())
            .map_err(|join_err| {
                McpError::Transport(format!("mcp session shutdown task panicked: {join_err}"))
            })
    }
}

/// Connect to an MCP server over `cfg`'s transport.
///
/// `bearer`, when present, is sent as the bare authorization token for [`TransportConfig::Http`]
/// (spec D2 — `rmcp` prepends the `Bearer ` scheme; never pass an already-prefixed value). It is
/// **ignored** for [`TransportConfig::Stdio`]: a local process server has no HTTP request to
/// attach a bearer to — if it needs credentials at all, they travel via `Stdio.env` instead
/// (caller's responsibility, same as any other env value).
pub async fn connect(cfg: TransportConfig, bearer: Option<String>) -> Result<McpSession, McpError> {
    match cfg {
        TransportConfig::Http { url } => {
            let transport = build_http_transport(&url, bearer.as_deref());
            let running = ().serve(transport).await.map_err(map_connect_err)?;
            Ok(McpSession::from_running(running))
        }
        TransportConfig::Stdio { command, args, env } => {
            let transport = build_stdio_transport(&command, &args, &env).map_err(map_spawn_err)?;
            let running = ().serve(transport).await.map_err(map_connect_err)?;
            Ok(McpSession::from_running(running))
        }
    }
}

/// Connect to an MCP server over an in-memory `tokio::io::duplex()` half — the transport this
/// crate's own stub-server tests use to prove the client path without any real HTTP I/O (spec
/// §9). Not part of the production surface: gated behind `test-support` so orchd, which only
/// ever calls [`connect`], cannot reach it by accident.
#[cfg(feature = "test-support")]
pub async fn connect_duplex(stream: tokio::io::DuplexStream) -> Result<McpSession, McpError> {
    let running = ().serve(stream).await.map_err(map_connect_err)?;
    Ok(McpSession::from_running(running))
}
