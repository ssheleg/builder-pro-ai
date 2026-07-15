//! Tool cache translation (S-EXT spec §3: "tool_cache read/write; staleness; list_changed
//! invalidation"). Translates a freshly `list_tools()`-fetched set into the `mcp_tool` cache-write
//! shape ([`NewMcpTool`], via `Db::upsert_mcp_tools` — T2, a full REPLACE, not a merge), which
//! [`super::lifecycle::connect`] calls on every successful connect.
//!
//! **`tools/list_changed` — honest handling (task T18, spec §3):** `bpa_mcp::client::connect`
//! opens a session with `().serve(transport)` (the unit `()` `ClientHandler` — see that
//! function's own doc comment), which has no `on_tool_list_changed` override; and Phase-1's
//! architecture (task-5 brief: "connect-per-call is fine") never holds a session open between
//! calls in the first place — `mcp::invoke::call_tool` opens a FRESH session per `tools/call` and
//! `McpConnect` opens a fresh one per explicit connect, each torn down at the end of that one
//! request. There is therefore no LIVE, long-lived connection over which a server could ever push
//! an async `notifications/tools/list_changed` for this daemon to observe in the first place —
//! implementing `ClientHandler::on_tool_list_changed` here would be dead code with nothing to
//! ever call it, an inert handler that FALSELY implies live behavior.
//!
//! The honest alternative this task takes instead: **the cache is refreshed on every
//! `McpConnect`** — `super::lifecycle::connect` always does a fresh `list_tools()` immediately
//! after the handshake and REPLACES the cached set wholesale (`connect_replaces_a_previously_
//! cached_tool_set`, that module's own test), and `socket_server::dispatch`'s `McpConnect` arm
//! pushes `McpToolsChanged{server_id}` on every success so the frontend's tool list re-fetches
//! too. In the connect-per-call model this is not a workaround: a server's list_changed
//! notification only ever matters to a client that's ABOUT to read the tool list again, and every
//! `McpConnect` already does exactly that read, unconditionally. The residual gap — a change that
//! happens on the SERVER between two `McpConnect`/`tools/call` attempts is invisible until the
//! next explicit `McpConnect` — is real but narrow (no persistent session exists to have observed
//! it live either way) and is tracked as **BL-70** (a persistent-session architecture + a live
//! `tools/list_changed` subscription is a follow-up for whenever Phase-1's connect-per-call model
//! is revisited, e.g. alongside S6b's agent org).

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
