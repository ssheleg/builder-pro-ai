//! Project-shaped MCP types + the `rmcp` → `bpa-mcp` mappings (S-EXT §3, D2, D8). Orchd only
//! ever sees these types — never `rmcp::model::{Tool, CallToolResult}` directly.

use serde_json::Value;

use crate::error::McpError;

/// One tool advertised by a connected MCP server.
#[derive(Debug, Clone, PartialEq)]
pub struct McpTool {
    pub name: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub input_schema: Value,
}

/// The result of one `tools/call` invocation.
#[derive(Debug, Clone, PartialEq)]
pub struct McpToolResult {
    pub content: Value,
    pub structured: Option<Value>,
    pub is_error: bool,
    pub usage: Option<Usage>,
}

/// Token/cost accounting for one invocation (spec D8). MCP tool results rarely carry usage
/// data; every field stays `None` unless the server's response clearly reports it — never
/// fabricated.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Usage {
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub cost_usd: Option<f64>,
}

pub(crate) fn map_tool(tool: rmcp::model::Tool) -> McpTool {
    McpTool {
        name: tool.name.into_owned(),
        title: tool.title,
        description: tool.description.map(|d| d.into_owned()),
        input_schema: Value::Object((*tool.input_schema).clone()),
    }
}

pub(crate) fn map_call_result(
    result: rmcp::model::CallToolResult,
) -> Result<McpToolResult, McpError> {
    let content = serde_json::to_value(&result.content)
        .map_err(|e| McpError::Protocol(format!("failed to serialize tool result content: {e}")))?;
    Ok(McpToolResult {
        content,
        structured: result.structured_content,
        is_error: result.is_error.unwrap_or(false),
        usage: None,
    })
}

#[cfg(test)]
mod tests {
    use rmcp::model::{CallToolResult, ContentBlock, Tool};
    use serde_json::json;

    use super::*;

    #[test]
    fn map_tool_copies_name_title_description_and_schema() {
        let mut schema = serde_json::Map::new();
        schema.insert("type".to_string(), json!("object"));
        let tool = Tool::new(
            "echo",
            "Echoes the input",
            std::sync::Arc::new(schema.clone()),
        )
        .with_title("Echo");

        let mapped = map_tool(tool);

        assert_eq!(mapped.name, "echo");
        assert_eq!(mapped.title.as_deref(), Some("Echo"));
        assert_eq!(mapped.description.as_deref(), Some("Echoes the input"));
        assert_eq!(mapped.input_schema, Value::Object(schema));
    }

    #[test]
    fn map_call_result_success_maps_is_error_false_and_serializes_content() {
        let result = CallToolResult::success(vec![ContentBlock::text("hi")]);
        let mapped = map_call_result(result).expect("mapping should succeed");

        assert!(!mapped.is_error);
        assert_eq!(mapped.content, json!([{"type": "text", "text": "hi"}]));
        assert!(mapped.structured.is_none());
        assert!(mapped.usage.is_none());
    }

    #[test]
    fn map_call_result_error_maps_is_error_true() {
        let result = CallToolResult::error(vec![ContentBlock::text("boom")]);
        let mapped = map_call_result(result).expect("mapping should succeed");

        assert!(mapped.is_error);
        assert_eq!(mapped.content, json!([{"type": "text", "text": "boom"}]));
    }
}
