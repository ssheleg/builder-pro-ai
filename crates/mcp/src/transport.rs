//! Transport construction (S-EXT §3, D2, D6). Phase 1 shipped only the Streamable HTTP
//! transport (`prowl.chat` is remote — spec D6); Phase 3 (T15) adds
//! [`TransportConfig::Stdio`] for local process MCP servers, spawned via `rmcp`'s
//! `TokioChildProcess`. [`TransportConfig`] stays `#[non_exhaustive]` — orchd's own
//! execution-consent gate (spawning a local process is code-exec; BL-22) and the
//! `DYLD_*`/`LD_*` env denylist (T16) live one layer up, in orchd's spawn call site, not here.

use std::collections::BTreeMap;
use std::sync::Arc;

use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::{ConfigureCommandExt, StreamableHttpClientTransport, TokioChildProcess};
use tokio::process::Command;

/// How to reach an MCP server: a remote **Streamable HTTP** endpoint or a local **stdio**
/// child process (spec D6).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportConfig {
    /// Streamable HTTP transport (spec D6 — the Phase-1 DoD path). `url` is the server's MCP
    /// endpoint (e.g. `https://prowl.chat/mcp`).
    Http { url: String },
    /// Spawn a local MCP server as a child process and speak MCP over its stdin/stdout (spec
    /// D6, Phase 3 / T15).
    ///
    /// `command` is the executable to spawn (absolute path, or resolved via `PATH` — whatever
    /// [`tokio::process::Command::new`] accepts); `args` are its CLI arguments; `env` are extra
    /// environment variables applied to the child **as given**.
    ///
    /// `bpa-mcp` does **no env filtering**: the caller is trusted to have already applied any
    /// policy (denylist, allowlist, secret injection) before constructing this variant. In
    /// particular, orchd's stdio spawn call site (T16) owns stripping `DYLD_*`/`LD_*` from
    /// `env` — that denylist does not exist in this crate and must not be added here (it's a
    /// shared helper used by both orchd's stdio spawn and sessiond's `env_overrides`, spec §6).
    Stdio {
        command: String,
        args: Vec<String>,
        env: BTreeMap<String, String>,
    },
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

/// Build the child-process transport for a [`TransportConfig::Stdio`] server: spawn `command`
/// with `args` and `env` (applied as-given — see the variant's doc comment), then hand the
/// spawned process's stdin/stdout to `rmcp` as an MCP transport.
///
/// Returns the `std::io::Error` from the underlying spawn (e.g. `command` not found or not
/// executable) unmapped; [`crate::client::connect`] maps it via
/// [`crate::error::map_spawn_err`].
pub(crate) fn build_stdio_transport(
    command: &str,
    args: &[String],
    env: &BTreeMap<String, String>,
) -> std::io::Result<TokioChildProcess> {
    let cmd = Command::new(command).configure(|cmd| {
        cmd.args(args);
        cmd.envs(env);
    });
    TokioChildProcess::new(cmd)
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
            TransportConfig::Stdio { .. } => panic!("expected Http"),
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

    #[test]
    fn transport_config_stdio_carries_command_args_and_env_verbatim() {
        let mut env = BTreeMap::new();
        env.insert("FOO".to_string(), "bar".to_string());
        let cfg = TransportConfig::Stdio {
            command: "/usr/bin/true".to_string(),
            args: vec!["--flag".to_string()],
            env: env.clone(),
        };
        match cfg {
            TransportConfig::Stdio {
                command,
                args,
                env: got_env,
            } => {
                assert_eq!(command, "/usr/bin/true");
                assert_eq!(args, vec!["--flag".to_string()]);
                assert_eq!(got_env, env);
            }
            TransportConfig::Http { .. } => panic!("expected Stdio"),
        }
    }

    // `TokioChildProcess::new` actually spawns the process (it's not deferred), so this needs a
    // live Tokio runtime (its Drop impl schedules an async kill via `tokio::spawn`) — same
    // reasoning as the `build_http_transport_*` tests above.
    #[tokio::test]
    async fn build_stdio_transport_spawns_a_trivial_command() {
        let env = BTreeMap::new();
        let transport = build_stdio_transport("true", &[], &env);
        assert!(transport.is_ok(), "{:?}", transport.err());
    }

    #[tokio::test]
    async fn build_stdio_transport_reports_the_spawn_error_for_a_missing_command() {
        let env = BTreeMap::new();
        let result = build_stdio_transport(
            "/this/command/definitely/does/not/exist/bpa-mcp-test",
            &[],
            &env,
        );
        assert!(
            result.is_err(),
            "expected a spawn error for a missing command"
        );
    }
}
