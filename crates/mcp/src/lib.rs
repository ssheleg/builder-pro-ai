//! `bpa-mcp` — a thin, project-shaped wrapper over the official `rmcp` SDK (S-EXT §3, D2).
//!
//! This crate is the ONLY place in Builder Pro AI that imports `rmcp` types. `bpa-orchd` (T5+)
//! talks to MCP servers exclusively through [`TransportConfig`], [`connect`], [`McpSession`],
//! [`McpTool`], [`McpToolResult`] and [`McpError`] — never `rmcp::*` directly. Phase 1 ships the
//! Streamable HTTP transport only (spec D6); a `Stdio` variant lands in a later phase (S-EXT
//! T15) behind an execution-consent gate, which is why [`TransportConfig`] is `#[non_exhaustive]`
//! today.

mod client;
mod error;
mod transport;
mod types;

pub use client::{connect, McpSession};
pub use error::McpError;
pub use transport::TransportConfig;
pub use types::{McpTool, McpToolResult, Usage};

// Test-only entry point (an in-memory `tokio::io::duplex()` transport) used exclusively by this
// crate's own stub-server tests (tests/stub.rs) — never part of the production surface.
#[cfg(feature = "test-support")]
pub use client::connect_duplex;
