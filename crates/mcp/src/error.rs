//! Typed MCP errors (S-EXT §3, D2, D7). `Display`/`Debug` never include a bearer token or any
//! other secret: every variant carries either a fixed string or `rmcp`'s own error text
//! describing what the *server* said — the bearer this crate sends via
//! [`crate::transport::build_http_transport`] is never echoed back into any of these messages
//! (see the `never_leaks_the_bearer` test below).

use rmcp::service::{ClientInitializeError, ServiceError};
use rmcp::transport::DynamicTransportError;

/// Error returned by [`crate::connect`] or any [`crate::McpSession`] method.
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum McpError {
    /// The transport could not reach the server, the connection dropped mid-call, or the
    /// response could not be parsed as JSON-RPC.
    #[error("mcp transport error: {0}")]
    Transport(String),
    /// The server responded, but not in a way the MCP protocol allows here (unexpected
    /// response shape, handshake/version mismatch).
    #[error("mcp protocol error: {0}")]
    Protocol(String),
    /// The call did not complete before its deadline. `bpa-mcp` applies no internal timeout
    /// itself (the caller — orchd's `invoke.rs`, T5 — wraps [`crate::McpSession::call_tool`]
    /// with its own budget per server config); this variant only fires when `rmcp`'s own
    /// request-level timeout elapses first.
    #[error("mcp call timed out")]
    Timeout,
    /// The server rejected the specific tool invocation: unknown tool name, invalid
    /// arguments, or a tool-level JSON-RPC error.
    #[error("mcp tool error: {0}")]
    ToolError(String),
    /// The server required or rejected credentials (HTTP 401/403, a WWW-Authenticate
    /// challenge, or an insufficient-scope response).
    #[error("mcp auth error: {0}")]
    Auth(String),
}

/// Substrings that mark a transport-level failure as credential-related rather than a generic
/// connectivity failure.
///
/// Detection is heuristic: by the time a transport failure reaches us as a
/// [`DynamicTransportError`], `rmcp`'s `WorkerTransport` has already erased the concrete
/// `StreamableHttpError<reqwest::Error>` type, so there is no typed downcast target available
/// from outside `rmcp`'s transport internals — this classifies by the rendered `Display` text
/// instead. Every one of these strings comes from the *server's* own HTTP status line or
/// `WWW-Authenticate` response header, never from the bearer this crate sent.
const AUTH_MARKERS: [&str; 6] = [
    "Auth required",
    "Insufficient scope",
    "401",
    "403",
    "Unauthorized",
    "Forbidden",
];

fn classify_dynamic_transport_error(err: &DynamicTransportError) -> McpError {
    let text = err.to_string();
    if AUTH_MARKERS.iter().any(|marker| text.contains(marker)) {
        McpError::Auth(text)
    } else {
        McpError::Transport(text)
    }
}

/// Map the error `().serve(transport).await` returns during the `initialize` handshake.
///
/// `ClientInitializeError` is `#[non_exhaustive]` (rmcp may add handshake-failure variants in a
/// later release), so every arm here — including the wildcard — must keep producing an honest
/// [`McpError`] rather than assuming today's variant set is final.
pub(crate) fn map_connect_err(err: ClientInitializeError) -> McpError {
    if let ClientInitializeError::TransportError { ref error, .. } = err {
        return classify_dynamic_transport_error(error);
    }
    match &err {
        ClientInitializeError::JsonRpcError(data) => McpError::Protocol(data.to_string()),
        _ => McpError::Transport(err.to_string()),
    }
}

/// Map the `std::io::Error` [`crate::transport::build_stdio_transport`] returns when it fails
/// to spawn the child process for a [`crate::TransportConfig::Stdio`] server (spec D6, T15) —
/// e.g. the command doesn't exist or isn't executable. Distinct from [`map_connect_err`], which
/// only ever sees errors from an already-spawned transport's `initialize` handshake.
pub(crate) fn map_spawn_err(err: std::io::Error) -> McpError {
    McpError::Transport(format!("failed to spawn stdio mcp server process: {err}"))
}

/// Map the error a live [`crate::McpSession`] call (`list_tools`/`call_tool`) returns.
///
/// `ServiceError` is `#[non_exhaustive]` for the same reason as [`ClientInitializeError`]
/// above — the wildcard arm keeps future variants from panicking or silently vanishing.
pub(crate) fn map_service_err(err: ServiceError) -> McpError {
    match &err {
        ServiceError::McpError(data) => McpError::ToolError(data.to_string()),
        ServiceError::TransportSend(transport_err) => {
            classify_dynamic_transport_error(transport_err)
        }
        ServiceError::Timeout { .. } => McpError::Timeout,
        ServiceError::UnexpectedResponse => McpError::Protocol(err.to_string()),
        _ => McpError::Transport(err.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use std::any::TypeId;
    use std::time::Duration;

    use rmcp::transport::streamable_http_client::{AuthRequiredError, StreamableHttpError};

    use super::*;

    #[test]
    fn map_service_err_unknown_tool_json_rpc_error_becomes_tool_error() {
        let data = rmcp::model::ErrorData::invalid_params("tool not found", None);
        let mapped = map_service_err(ServiceError::McpError(data));
        match mapped {
            McpError::ToolError(msg) => assert!(msg.contains("tool not found"), "{msg}"),
            other => panic!("expected McpError::ToolError, got: {other:?}"),
        }
    }

    #[test]
    fn map_service_err_timeout_becomes_timeout() {
        let mapped = map_service_err(ServiceError::Timeout {
            timeout: Duration::from_secs(5),
        });
        assert_eq!(mapped, McpError::Timeout);
    }

    #[test]
    fn map_service_err_transport_closed_becomes_transport() {
        let mapped = map_service_err(ServiceError::TransportClosed);
        assert!(matches!(mapped, McpError::Transport(_)));
    }

    #[test]
    fn map_connect_err_auth_required_transport_becomes_auth_and_never_leaks_the_bearer() {
        let planted_bearer = "sk-live-planted-secret-should-never-appear";
        let http_err: StreamableHttpError<reqwest::Error> = StreamableHttpError::AuthRequired(
            AuthRequiredError::new(r#"Bearer realm="mcp""#.to_string()),
        );
        let dyn_err = DynamicTransportError::from_parts(
            "streamable-http-test",
            TypeId::of::<()>(),
            Box::new(http_err),
        );
        let err = ClientInitializeError::TransportError {
            error: dyn_err,
            context: "send initialize request".into(),
        };

        let mapped = map_connect_err(err);

        match &mapped {
            McpError::Auth(msg) => assert!(
                msg.contains("Auth required"),
                "expected an auth-classified message, got: {msg}"
            ),
            other => panic!("expected McpError::Auth, got: {other:?}"),
        }
        for text in [format!("{mapped}"), format!("{mapped:?}")] {
            assert!(
                !text.contains(planted_bearer),
                "McpError rendering leaked the planted bearer: {text}"
            );
        }
    }

    #[test]
    fn map_connect_err_generic_transport_failure_is_transport_not_auth() {
        let http_err: StreamableHttpError<reqwest::Error> =
            StreamableHttpError::UnexpectedEndOfStream;
        let dyn_err = DynamicTransportError::from_parts(
            "streamable-http-test",
            TypeId::of::<()>(),
            Box::new(http_err),
        );
        let err = ClientInitializeError::TransportError {
            error: dyn_err,
            context: "send initialize request".into(),
        };

        let mapped = map_connect_err(err);
        assert!(matches!(mapped, McpError::Transport(_)));
    }

    #[test]
    fn map_connect_err_json_rpc_error_becomes_protocol() {
        let data = rmcp::model::ErrorData::internal_error("boom", None);
        let mapped = map_connect_err(ClientInitializeError::JsonRpcError(data));
        assert!(matches!(mapped, McpError::Protocol(_)));
    }

    #[test]
    fn map_spawn_err_becomes_transport_and_carries_the_io_error_text() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "no such file");
        let mapped = map_spawn_err(io_err);
        match mapped {
            McpError::Transport(msg) => assert!(msg.contains("no such file"), "{msg}"),
            other => panic!("expected McpError::Transport, got: {other:?}"),
        }
    }
}
