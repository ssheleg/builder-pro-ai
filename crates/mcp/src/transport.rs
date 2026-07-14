//! Transport construction (S-EXT §3, D2, D6). Phase 1 ships only the Streamable HTTP transport
//! (`prowl.chat` is remote — spec D6); `Stdio` lands in a later phase behind an
//! execution-consent gate (spawning a local process is code-exec; BL-22), so
//! [`TransportConfig`] is `#[non_exhaustive]` today rather than pretending to be final.

use std::sync::Arc;

use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::StreamableHttpClientTransport;

/// How to reach an MCP server. Only [`TransportConfig::Http`] exists in Phase 1.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportConfig {
    /// Streamable HTTP transport (spec D6 — the Phase-1 DoD path). `url` is the server's MCP
    /// endpoint (e.g. `https://prowl.chat/mcp`).
    Http { url: String },
}

/// Build the reqwest-backed Streamable HTTP client transport for `url`, applying `bearer` as
/// the authorization header when present.
///
/// `bearer` MUST be the bare token: [`StreamableHttpClientTransportConfig::auth_header`]
/// prepends the `Bearer ` scheme itself (spec D2) — passing an already-prefixed value would
/// send `Authorization: Bearer Bearer <token>`.
pub(crate) fn build_http_transport(
    url: &str,
    bearer: Option<&str>,
) -> StreamableHttpClientTransport<reqwest::Client> {
    match bearer {
        Some(token) => {
            let config = StreamableHttpClientTransportConfig::with_uri(Arc::<str>::from(url))
                .auth_header(token.to_string());
            StreamableHttpClientTransport::from_config(config)
        }
        None => StreamableHttpClientTransport::from_uri(Arc::<str>::from(url)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transport_config_http_carries_the_url_verbatim() {
        let cfg = TransportConfig::Http {
            url: "https://prowl.chat/mcp".to_string(),
        };
        match cfg {
            TransportConfig::Http { url } => assert_eq!(url, "https://prowl.chat/mcp"),
        }
    }

    // `build_http_transport` spawns rmcp's transport worker task (`tokio::spawn` inside
    // `StreamableHttpClientTransport::with_client`), so constructing it needs a live Tokio
    // runtime even though no request is ever sent — hence `#[tokio::test]`, not `#[test]`.
    #[tokio::test]
    async fn build_http_transport_accepts_no_bearer() {
        // Smoke-test only: constructing the transport must not panic or require network I/O
        // (rmcp's worker task doesn't dial out until the first request is sent).
        let _transport = build_http_transport("https://example.invalid/mcp", None);
    }

    #[tokio::test]
    async fn build_http_transport_accepts_a_bare_bearer() {
        let _transport = build_http_transport("https://example.invalid/mcp", Some("token-123"));
    }
}
