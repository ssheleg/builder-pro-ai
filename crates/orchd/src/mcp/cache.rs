//! Tool cache translation (S-EXT spec §3: "tool_cache read/write; staleness; list_changed
//! invalidation"). Phase-1 scope (task-5 brief) only needs the read/write half: translating a
//! freshly `list_tools()`-fetched set into the `mcp_tool` cache-write shape ([`NewMcpTool`], via
//! `Db::upsert_mcp_tools` — T2), which [`super::lifecycle::connect`] calls on every successful
//! connect. Staleness tracking and `list_changed`-triggered invalidation are explicitly Phase 3
//! (spec D14) and are NOT implemented here yet.

use super::NewMcpTool;

/// Translate a server's freshly `list_tools()`-fetched set into `mcp_tool` cache-write rows.
/// `bpa_mcp::McpTool.input_schema` is a `serde_json::Value`; `mcp_tool.input_schema_json` is
/// TEXT (spec §4) — falls back to `"{}"` on the never-expected-in-practice case that a schema
/// fails to re-serialize, rather than panicking on malformed server output.
pub fn to_new_tools(tools: &[bpa_mcp::McpTool]) -> Vec<NewMcpTool> {
    tools
        .iter()
        .map(|t| NewMcpTool {
            name: t.name.clone(),
            title: t.title.clone(),
            description: t.description.clone(),
            input_schema_json: serde_json::to_string(&t.input_schema)
                .unwrap_or_else(|_| "{}".to_string()),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn to_new_tools_maps_every_field_and_serializes_the_schema() {
        let tools = vec![bpa_mcp::McpTool {
            name: "search".to_string(),
            title: Some("Search".to_string()),
            description: Some("desc".to_string()),
            input_schema: json!({"type": "object"}),
        }];

        let new_tools = to_new_tools(&tools);

        assert_eq!(new_tools.len(), 1);
        assert_eq!(new_tools[0].name, "search");
        assert_eq!(new_tools[0].title.as_deref(), Some("Search"));
        assert_eq!(new_tools[0].description.as_deref(), Some("desc"));
        assert_eq!(new_tools[0].input_schema_json, "{\"type\":\"object\"}");
    }

    #[test]
    fn to_new_tools_handles_no_title_or_description() {
        let tools = vec![bpa_mcp::McpTool {
            name: "fetch".to_string(),
            title: None,
            description: None,
            input_schema: json!({}),
        }];

        let new_tools = to_new_tools(&tools);

        assert_eq!(new_tools[0].title, None);
        assert_eq!(new_tools[0].description, None);
        assert_eq!(new_tools[0].input_schema_json, "{}");
    }
}
