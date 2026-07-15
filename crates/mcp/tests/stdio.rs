//! Real child-process stdio round-trip test for `TransportConfig::Stdio` (S-EXT §3, D6, T15).
//!
//! Unlike `tests/stub.rs` (an in-memory `tokio::io::duplex()` pair), this test spawns an ACTUAL
//! separate OS process — `stub_stdio_server` (`src/bin/stub_stdio_server.rs`, built only when
//! the `stub-stdio-server` feature is active) — and connects `bpa_mcp::connect` to it over real
//! stdin/stdout pipes (`rmcp::transport::TokioChildProcess`), proving the whole
//! `TransportConfig::Stdio` path end-to-end. Hermetic: the "server" is a local child process
//! built by this same `cargo test` invocation, no network involved.

use std::collections::BTreeMap;

use bpa_mcp::TransportConfig;
use serde_json::json;

fn stub_stdio_server_path() -> String {
    env!("CARGO_BIN_EXE_stub_stdio_server").to_string()
}

fn stdio_cfg() -> TransportConfig {
    TransportConfig::Stdio {
        command: stub_stdio_server_path(),
        args: vec![],
        env: BTreeMap::new(),
    }
}

#[tokio::test]
async fn stdio_transport_lists_the_echo_tool_with_its_schema() {
    let session = bpa_mcp::connect(stdio_cfg(), None)
        .await
        .expect("connect over a real stdio child process should succeed");

    let tools = session
        .list_tools()
        .await
        .expect("list_tools should succeed");
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&"echo"), "tools: {names:?}");

    let echo = tools.iter().find(|t| t.name == "echo").unwrap();
    let echo_props = echo.input_schema["properties"]
        .as_object()
        .expect("echo input_schema should have properties");
    assert!(
        echo_props.contains_key("msg"),
        "echo schema: {:?}",
        echo.input_schema
    );
}

#[tokio::test]
async fn stdio_transport_call_tool_echo_round_trips_over_a_real_child_process() {
    let mut session = bpa_mcp::connect(stdio_cfg(), None)
        .await
        .expect("connect over a real stdio child process should succeed");

    let result = session
        .call_tool("echo", json!({"msg": "hello-stdio"}))
        .await
        .expect("call_tool(echo) should succeed");

    assert!(
        !result.is_error,
        "echo should not be an error result: {:?}",
        result.content
    );
    assert!(
        result.content.to_string().contains("hello-stdio"),
        "echo content should contain 'hello-stdio': {:?}",
        result.content
    );

    session.close().await.expect("close should succeed");
}

#[tokio::test]
async fn stdio_transport_unknown_tool_returns_mcp_error() {
    let mut session = bpa_mcp::connect(stdio_cfg(), None)
        .await
        .expect("connect over a real stdio child process should succeed");

    let err = session
        .call_tool("nope", json!({}))
        .await
        .expect_err("calling an unknown tool should fail");

    assert!(
        matches!(err, bpa_mcp::McpError::ToolError(_)),
        "expected McpError::ToolError for an unknown tool, got: {err:?}"
    );

    session.close().await.expect("close should succeed");
}

#[tokio::test]
async fn stdio_transport_bearer_is_ignored_not_rejected() {
    // Spec/brief: stdio servers don't take an HTTP bearer. `connect` must not error just
    // because a caller passed one — it's simply unused for this transport.
    let session = bpa_mcp::connect(stdio_cfg(), Some("ignored-token".to_string()))
        .await
        .expect("a bearer passed alongside TransportConfig::Stdio must be ignored, not rejected");

    let tools = session
        .list_tools()
        .await
        .expect("list_tools should succeed");
    assert!(tools.iter().any(|t| t.name == "echo"));
}
