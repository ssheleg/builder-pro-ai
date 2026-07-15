//! `bpa-mcp` — a thin, project-shaped wrapper over the official `rmcp` SDK (S-EXT §3, D2).
//!
//! This crate is the ONLY place in Builder Pro AI that imports `rmcp` types. `bpa-orchd` (T5+)
//! talks to MCP servers exclusively through [`TransportConfig`], [`connect`], [`McpSession`],
//! [`McpTool`], [`McpToolResult`] and [`McpError`] — never `rmcp::*` directly. [`TransportConfig`]
//! supports both the Streamable HTTP transport (Phase 1, spec D6 — the DoD path) and, as of
//! S-EXT T15, `Stdio` (a local child-process MCP server). It stays `#[non_exhaustive]` because
//! this crate does not itself gate `Stdio` behind execution consent or filter its env — that's
//! orchd's job, one layer up (BL-22 consent gate, T16 `DYLD_*`/`LD_*` denylist).

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
