//! In-process rmcp stub-server test proving `bpa-mcp`'s client path without any real HTTP I/O
//! (S-EXT §3/§9, task brief T4 step 1). Server and client run on opposite halves of one
//! `tokio::io::duplex()` pair — the in-memory analogue of rmcp's TCP-stream example (rmcp
//! transport is generic over any `AsyncRead + AsyncWrite`).
//!
//! `bpa-mcp` never re-exports `rmcp`, so this file (a dev-only stub *server*) is the one place
//! outside `bpa-mcp`'s own `src/` that legitimately imports `rmcp` — it plays the role of a
//! third-party MCP server, not of orchd (which only ever sees `bpa_mcp::{connect, McpSession,
//! McpTool, McpToolResult, McpError}`).

use bpa_mcp::McpError;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock};
use rmcp::{schemars, tool, tool_router, ServiceExt};
use serde::Deserialize;
use serde_json::json;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct EchoRequest {
    /// The message to echo back.
    msg: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AddRequest {
    a: i64,
    b: i64,
}

/// The stub MCP server: `echo`, `add`, and a `fail` tool that always returns a tool-level error
/// result (used to prove `is_error` propagates through the client mapping untouched — distinct
/// from an unknown-tool call, which is a *protocol*-level error, not a tool-level one).
#[derive(Debug, Clone, Copy, Default)]
struct StubServer;

#[tool_router(server_handler)]
impl StubServer {
    #[tool(description = "Echo the given message back")]
    fn echo(&self, Parameters(EchoRequest { msg }): Parameters<EchoRequest>) -> String {
        msg
    }

    #[tool(description = "Add two integers")]
    fn add(&self, Parameters(AddRequest { a, b }): Parameters<AddRequest>) -> String {
        (a + b).to_string()
    }

    #[tool(description = "Always fails (tool-level error, not a protocol error)")]
    fn fail(&self) -> CallToolResult {
        CallToolResult::error(vec![ContentBlock::text("stub tool failed intentionally")])
    }
}

/// Spawn the stub server on one half of an in-memory duplex pair and connect a `bpa-mcp`
/// client to the other half.
async fn connected_stub() -> (bpa_mcp::McpSession, tokio::task::JoinHandle<()>) {
    let (server_half, client_half) = tokio::io::duplex(8192);

    let server_handle = tokio::spawn(async move {
        StubServer
            .serve(server_half)
            .await
            .expect("stub server failed to initialize")
            .waiting()
            .await
            .expect("stub server task panicked");
    });

    let session = bpa_mcp::connect_duplex(client_half)
        .await
        .expect("client failed to connect to the stub server");

    (session, server_handle)
}

#[tokio::test]
async fn list_tools_returns_echo_add_and_fail_with_schemas() {
    let (session, server_handle) = connected_stub().await;

    let tools = session
        .list_tools()
        .await
        .expect("list_tools should succeed");
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&"echo"), "tools: {names:?}");
    assert!(names.contains(&"add"), "tools: {names:?}");
    assert!(names.contains(&"fail"), "tools: {names:?}");

    let echo = tools.iter().find(|t| t.name == "echo").unwrap();
    let echo_props = echo.input_schema["properties"]
        .as_object()
        .expect("echo input_schema should have properties");
    assert!(
        echo_props.contains_key("msg"),
        "echo schema: {:?}",
        echo.input_schema
    );

    let add = tools.iter().find(|t| t.name == "add").unwrap();
    let add_props = add.input_schema["properties"]
        .as_object()
        .expect("add input_schema should have properties");
    assert!(
        add_props.contains_key("a") && add_props.contains_key("b"),
        "add schema: {:?}",
        add.input_schema
    );

    drop(session);
    server_handle
        .await
        .expect("server task should exit cleanly");
}

#[tokio::test]
async fn call_tool_echo_returns_the_message_and_is_not_an_error() {
    let (mut session, server_handle) = connected_stub().await;

    let result = session
        .call_tool("echo", json!({"msg": "hi"}))
        .await
        .expect("call_tool(echo) should succeed");

    assert!(
        !result.is_error,
        "echo should not be an error result: {:?}",
        result.content
    );
    let rendered = result.content.to_string();
    assert!(
        rendered.contains("hi"),
        "echo content should contain 'hi': {rendered}"
    );

    session.close().await.expect("close should succeed");
    server_handle
        .await
        .expect("server task should exit cleanly");
}

#[tokio::test]
async fn call_tool_add_returns_the_sum() {
    let (mut session, server_handle) = connected_stub().await;

    let result = session
        .call_tool("add", json!({"a": 2, "b": 3}))
        .await
        .expect("call_tool(add) should succeed");

    assert!(!result.is_error, "{:?}", result.content);
    assert!(
        result.content.to_string().contains('5'),
        "add content should contain the sum '5': {:?}",
        result.content
    );

    session.close().await.expect("close should succeed");
    server_handle
        .await
        .expect("server task should exit cleanly");
}

#[tokio::test]
async fn call_tool_fail_reports_is_error_true_without_a_transport_error() {
    let (mut session, server_handle) = connected_stub().await;

    let result = session
        .call_tool("fail", json!(null))
        .await
        .expect("call_tool(fail) is a successful RPC that returns a tool-level error result");

    assert!(
        result.is_error,
        "fail tool should report is_error=true: {:?}",
        result.content
    );
    assert!(
        result
            .content
            .to_string()
            .contains("stub tool failed intentionally"),
        "{:?}",
        result.content
    );

    session.close().await.expect("close should succeed");
    server_handle
        .await
        .expect("server task should exit cleanly");
}

#[tokio::test]
async fn call_tool_unknown_tool_returns_mcp_error() {
    let (mut session, server_handle) = connected_stub().await;

    let err = session
        .call_tool("nope", json!({}))
        .await
        .expect_err("calling an unknown tool should fail");

    assert!(
        matches!(err, McpError::ToolError(_)),
        "expected McpError::ToolError for an unknown tool, got: {err:?}"
    );

    session.close().await.expect("close should succeed");
    server_handle
        .await
        .expect("server task should exit cleanly");
}

#[tokio::test]
async fn protocol_version_reports_a_negotiated_version() {
    let (mut session, server_handle) = connected_stub().await;

    let version = session.protocol_version();
    assert!(!version.is_empty(), "protocol_version should not be empty");

    session.close().await.expect("close should succeed");
    server_handle
        .await
        .expect("server task should exit cleanly");
}
