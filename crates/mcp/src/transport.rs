//! Transport construction (S-EXT §3, D2, D6). Phase 1 shipped only the Streamable HTTP
//! transport (`prowl.chat` is remote — spec D6); Phase 3 (T15) adds
//! [`TransportConfig::Stdio`] for local process MCP servers, spawned via `rmcp`'s
//! `TokioChildProcess`. [`TransportConfig`] stays `#[non_exhaustive]` — orchd's own
//! execution-consent gate (spawning a local process is code-exec; BL-22) and the
//! `DYLD_*`/`LD_*` env denylist (T16) live one layer up, in orchd's spawn call site, not here.

use std::collections::BTreeMap;
use std::sync::Arc;

use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::{StreamableHttpClientTransport, TokioChildProcess};
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
    /// [`tokio::process::Command::new`] accepts); `args` are its CLI arguments; `env` is the
    /// child's **COMPLETE** environment — the child inherits **nothing** from orchd's ambient
    /// env (the spawn does `env_clear()` before applying `env`, see [`build_stdio_transport`]).
    ///
    /// `bpa-mcp` does **no env policy**: the caller supplies the entire environment the child
    /// should see and is trusted to have already (a) included a safe base — `PATH`/`HOME`/… the
    /// child needs to run — and (b) applied any filtering (`DYLD_*`/`LD_*` denylist, allowlist,
    /// secret injection). In particular, orchd's stdio spawn call site (T16) builds this map as
    /// its own ambient env MERGED with the DB-configured `server.env`, then strips `DYLD_*`/`LD_*`
    /// from the whole thing — that denylist lives in `bpa_daemon_core::env_filter` (shared with
    /// sessiond's `env_overrides`, spec §6) and must not be added here.
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
/// with `args` and `env`, then hand the spawned process's stdin/stdout to `rmcp` as an MCP
/// transport.
///
/// The child's environment is **exactly** `env` and nothing else: `cmd.env_clear()` runs BEFORE
/// `cmd.envs(env)`, so the child does NOT inherit orchd's (this process's) ambient environment.
/// `tokio::process::Command` inherits the parent env by default and `.envs()` only merges on top
/// of that — without the clear, orchd's own `DYLD_*`/`LD_*` (dev tooling, a compromised launch
/// profile, an attacker who can influence orchd's launch env) would flow into every stdio MCP
/// child, completely bypassing the `env` denylist the caller applied. Clearing first makes the
/// child env fully caller-specified: the caller (orchd, see
/// `bpa_orchd::mcp::build_transport_config`) owns the COMPLETE env — it is responsible for
/// including a safe base (PATH/HOME/…) as well as filtering — and this crate does no env policy
/// itself (see [`TransportConfig::Stdio`]'s own doc comment). Mirrors `bpa-sessiond`'s PTY spawn,
/// which likewise `env_clear()`s then sets a strict allowlist (`pty_supervisor.rs`).
///
/// Returns the `std::io::Error` from the underlying spawn (e.g. `command` not found or not
/// executable) unmapped; [`crate::client::connect`] maps it via
/// [`crate::error::map_spawn_err`].
pub(crate) fn build_stdio_transport(
    command: &str,
    args: &[String],
    env: &BTreeMap<String, String>,
) -> std::io::Result<TokioChildProcess> {
    TokioChildProcess::new(configure_stdio_command(command, args, env))
}

/// Build the exact [`tokio::process::Command`] [`build_stdio_transport`] hands to
/// `rmcp`'s `TokioChildProcess` — `env_clear()` (drop ALL inherited/ambient env) THEN `envs(env)`
/// (set exactly the caller's map). Extracted so a test can spawn it with `.output()` and inspect
/// the child's ACTUAL environment (the transport itself consumes stdin/stdout for MCP, so the
/// child's env can't be observed through `build_stdio_transport`'s return value).
fn configure_stdio_command(
    command: &str,
    args: &[String],
    env: &BTreeMap<String, String>,
) -> Command {
    let mut cmd = Command::new(command);
    cmd.args(args);
    // Clear BEFORE merging: the child gets EXACTLY `env`, never orchd's ambient env.
    cmd.env_clear();
    cmd.envs(env);
    cmd
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

    // ---- T16 review (Critical): the child inherits NOTHING from this (orchd's) ambient env —
    // `configure_stdio_command` `env_clear()`s before applying the caller's map, so a
    // `DYLD_INSERT_LIBRARIES`/`LD_PRELOAD` sitting in orchd's OWN process env can never leak into
    // a stdio MCP child. Proven against a REAL spawned process (`/usr/bin/env` prints its actual
    // environment), mirroring how `bpa-sessiond`'s `pty_supervisor` tests prove its `env_clear`.
    #[tokio::test]
    async fn stdio_child_env_is_exactly_the_passed_map_no_ambient_inheritance() {
        // Plant dangerous + benign markers in THIS process's ambient env (edition 2021 — safe).
        std::env::set_var("DYLD_INSERT_LIBRARIES", "/evil.dylib");
        std::env::set_var("LD_PRELOAD", "/evil.so");
        std::env::set_var("BPA_AMBIENT_MARKER", "should-not-leak-into-child");

        let mut env = BTreeMap::new();
        env.insert("FOO".to_string(), "bar".to_string());
        env.insert("PATH".to_string(), "/usr/bin:/bin".to_string());

        let output = configure_stdio_command("/usr/bin/env", &[], &env)
            .output()
            .await
            .expect("/usr/bin/env should spawn and exit");
        let text = String::from_utf8_lossy(&output.stdout);

        assert!(
            text.contains("FOO=bar"),
            "the caller's env must reach the child: {text:?}"
        );
        assert!(
            text.contains("PATH=/usr/bin:/bin"),
            "the caller's PATH must reach the child: {text:?}"
        );
        assert!(
            !text.contains("DYLD_INSERT_LIBRARIES"),
            "orchd's ambient DYLD_INSERT_LIBRARIES must NOT leak into the child: {text:?}"
        );
        assert!(
            !text.contains("LD_PRELOAD"),
            "orchd's ambient LD_PRELOAD must NOT leak into the child: {text:?}"
        );
        assert!(
            !text.contains("BPA_AMBIENT_MARKER"),
            "no ambient var may leak — the child env is exactly the passed map: {text:?}"
        );

        std::env::remove_var("DYLD_INSERT_LIBRARIES");
        std::env::remove_var("LD_PRELOAD");
        std::env::remove_var("BPA_AMBIENT_MARKER");
    }
}
