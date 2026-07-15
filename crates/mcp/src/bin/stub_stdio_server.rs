//! A REAL child-process MCP server speaking MCP over its own stdin/stdout, used only by
//! `tests/stdio.rs` to prove `bpa_mcp::TransportConfig::Stdio` end-to-end against an actual
//! separate OS process (S-EXT §3, D6, T15) — unlike `tests/stub.rs`'s server, which runs
//! in-process over an in-memory `tokio::io::duplex()`.
//!
//! Built only when the `stub-stdio-server` feature is active (see `crates/mcp/Cargo.toml`'s
//! `[[bin]]` `required-features`) — never part of a normal `cargo build`. `tests/stdio.rs`
//! locates this binary via `env!("CARGO_BIN_EXE_stub_stdio_server")`, which Cargo sets because
//! this crate declares itself as a `test-support`+`stub-stdio-server` dev-dependency of its own
//! integration tests (the same self-dependency trick `test-support` already uses).

use rmcp::handler::server::wrapper::Parameters;
use rmcp::{schemars, tool, tool_router, ServiceExt};
use serde::Deserialize;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct EchoRequest {
    /// The message to echo back.
    msg: String,
}

/// The stub MCP server: one `echo` tool, enough to prove `list_tools`/`call_tool` round-trip
/// over a real child-process transport.
#[derive(Debug, Clone, Copy, Default)]
struct StubStdioServer;

#[tool_router(server_handler)]
impl StubStdioServer {
    #[tool(description = "Echo the given message back")]
    fn echo(&self, Parameters(EchoRequest { msg }): Parameters<EchoRequest>) -> String {
        msg
    }
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let transport = rmcp::transport::stdio();
    let server = StubStdioServer
        .serve(transport)
        .await
        .map_err(std::io::Error::other)?;
    server.waiting().await.map_err(std::io::Error::other)?;
    Ok(())
}
