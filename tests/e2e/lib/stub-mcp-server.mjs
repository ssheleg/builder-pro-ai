// Minimal Node MCP "Streamable HTTP" stub server (S-EXT spec §9's "e2e ... against a local stub
// MCP server") — the counterpart, on the JS side, to `crates/orchd/tests/dispatch_integration.rs`'s
// real loopback rmcp Streamable-HTTP stub (that one is driven by orchd's own Rust integration
// tests; THIS one is what `tests/e2e/orchd-survive.mjs`'s phase6 spawns so the harness can prove
// the DoD — connect -> list -> call -> durable-artifact-across-restart — against the REAL
// `bpa-orchd` binary, which in turn drives the REAL `rmcp` Streamable-HTTP CLIENT
// (`crates/mcp/src/transport.rs::build_http_transport` -> `StreamableHttpClientTransport<reqwest::
// Client>`, `crates/mcp/src/client.rs::connect`). Every response shape here was verified against
// the vendored `rmcp` 2.2.0 source (`~/.cargo/registry/src/.../rmcp-2.2.0/src/{model.rs,
// transport/streamable_http_client.rs, transport/common/reqwest/streamable_http_client.rs}`), NOT
// guessed from the MCP spec alone — see the per-branch comments below for exactly which client
// code path each response shape satisfies.
//
// ---- design: STATELESS (no `Mcp-Session-Id`) ----
//
// `StreamableHttpClientTransportConfig::default()` sets `allow_stateless: true` (rmcp
// `streamable_http_client.rs`), and the client's `run()` loop only:
//   - fatally errors on a missing session id when `allow_stateless` is `false` (not our case), and
//   - only spawns the long-poll `GET` SSE stream (`streams.spawn(... client.get_stream(...) ...)`)
//     `if let Some(session_id) = &session_id` — i.e. only when the `initialize` response carried a
//     `Mcp-Session-Id` response header.
// This stub deliberately NEVER sends that header, so the real client never opens a GET stream and
// never sends a session-scoped `DELETE` on close. `GET`/`DELETE` are still handled below (405 /
// 200) purely for protocol completeness / robustness against a hypothetical non-stateless client,
// per the task brief — they are not expected to be hit by this harness's own flow.
//
// ---- design: every response is a single synchronous JSON object, never SSE ----
//
// The client's `Accept` header on every POST is `text/event-stream, application/json`
// (`reqwest/streamable_http_client.rs::post_message`), so a plain `Content-Type: application/json`
// response with a single JSON-RPC message body is accepted directly via the `StreamableHttpPost
// Response::Json` branch (`expect_initialized`/`expect_accepted_or_json` both special-case it) —
// no SSE framing is needed. A JSON-RPC *notification* (`notifications/initialized`, carries no
// `id`) is replied to with a bare `202 Accepted` (`status.is_success() && ACCEPTED|NO_CONTENT` is
// the client's own "just proceed" branch), no body.
//
// ---- protocol surface ----
//
//   initialize              -> `{protocolVersion, capabilities:{tools:{}}, serverInfo}`
//                               (`InitializeResult`, `model.rs` — client stores whatever
//                               `protocolVersion` comes back; no client-side compat check exists
//                               in `service.rs`/`handler/client.rs`, verified by source read).
//   notifications/initialized -> 202, no body (notification, no reply payload expected).
//   tools/list               -> one `echo` tool, `inputSchema:{type:"object",
//                               properties:{msg:{type:"string"}}}` (`Tool`/`ListToolsResult`,
//                               `model.rs`/`model/tool.rs` — `nextCursor`/`_meta` are `Option`
//                               fields serde defaults to `None` when absent from the JSON, so
//                               omitting them is safe).
//   tools/call (echo)        -> `{content:[{type:"text", text:<msg>}], isError:false}`
//                               (`CallToolResult`/`ContentBlock::Text` — `crates/mcp/src/types.rs
//                               ::map_call_result` serializes `content` straight through, so this
//                               exact shape is what ends up in `McpCallResult.contentJson` /
//                               `mcp_artifact.content_json` on the orchd side — the harness's
//                               phase6 asserts the planted message text against THAT shape).
//   tools/call (unknown tool) -> `{content:[{type:"text", text:"unknown tool: <name>"}],
//                               isError:true}` — never hit by this harness's own flow (defensive).
//   any other request         -> JSON-RPC `-32601 method not found` (defensive; unused today).

import http from "node:http";

const PROTOCOL_VERSION = "2025-11-25";
const JSON_MIME = "application/json";
const MCP_PATH = "/mcp";

function sendJson(res, status, body) {
  const buf = Buffer.from(JSON.stringify(body), "utf8");
  res.writeHead(status, {
    "Content-Type": JSON_MIME,
    "Content-Length": buf.length,
  });
  res.end(buf);
}

function sendEmpty(res, status) {
  res.writeHead(status, { "Content-Length": 0 });
  res.end();
}

/**
 * Handle one parsed JSON-RPC 2.0 message (a request — has `id` — or a notification — no `id`).
 * Returns `{status, body}`; `body: null` means "send `status` with an empty body" (the
 * Accepted/notification path).
 */
function handleRpcMessage(message) {
  const hasId = Object.prototype.hasOwnProperty.call(message, "id");
  const { id, method, params } = message;

  switch (method) {
    case "initialize": {
      // Echo the client's requested protocolVersion when present (so a future stricter client
      // that DOES validate the echo still negotiates cleanly); fall back to this stub's own
      // supported version otherwise. Either way the real `bpa-mcp` client accepts whatever comes
      // back verbatim (verified: no compat check on the client's `initialize` handling).
      const clientVersion =
        params && typeof params.protocolVersion === "string" && params.protocolVersion.length > 0
          ? params.protocolVersion
          : PROTOCOL_VERSION;
      return {
        status: 200,
        body: {
          jsonrpc: "2.0",
          id,
          result: {
            protocolVersion: clientVersion,
            capabilities: { tools: {} },
            serverInfo: { name: "stub", version: "0.0.1" },
          },
        },
      };
    }
    case "notifications/initialized":
      return { status: 202, body: null };
    case "tools/list":
      return {
        status: 200,
        body: {
          jsonrpc: "2.0",
          id,
          result: {
            tools: [
              {
                name: "echo",
                description: "Echo the given message back",
                inputSchema: {
                  type: "object",
                  properties: { msg: { type: "string" } },
                },
              },
            ],
          },
        },
      };
    case "tools/call": {
      const toolName = params && params.name;
      const args = (params && params.arguments) || {};
      if (toolName !== "echo") {
        return {
          status: 200,
          body: {
            jsonrpc: "2.0",
            id,
            result: {
              content: [{ type: "text", text: `unknown tool: ${toolName}` }],
              isError: true,
            },
          },
        };
      }
      const msg = typeof args.msg === "string" ? args.msg : "";
      return {
        status: 200,
        body: {
          jsonrpc: "2.0",
          id,
          result: {
            content: [{ type: "text", text: msg }],
            isError: false,
          },
        },
      };
    }
    case "ping":
      return { status: 200, body: { jsonrpc: "2.0", id, result: {} } };
    default:
      if (!hasId) {
        // An unrecognized NOTIFICATION (no reply expected either way) -> Accepted, never an error.
        return { status: 202, body: null };
      }
      return {
        status: 200,
        body: {
          jsonrpc: "2.0",
          id,
          error: { code: -32601, message: `method not found: ${method}` },
        },
      };
  }
}

/**
 * Start the stub MCP server on `127.0.0.1:0` (OS-assigned loopback port). Resolves once bound,
 * with `{ port, url, close() }` — `url` is the server's single MCP endpoint
 * (`http://127.0.0.1:<port>/mcp`), exactly the shape `McpAddServer.url` expects (spec §5).
 */
export function startStubMcpServer() {
  return new Promise((resolve, reject) => {
    const server = http.createServer((req, res) => {
      const urlPath = req.url ? req.url.split("?")[0] : "/";
      if (urlPath !== MCP_PATH) {
        sendEmpty(res, 404);
        return;
      }

      if (req.method === "GET") {
        // See module doc: this stub is stateless (no `Mcp-Session-Id`), so the real rmcp client
        // never actually issues this request in this harness's flow — handled for completeness.
        // 405 is exactly what the client's `get_stream` maps to `StreamableHttpError::
        // ServerDoesNotSupportSse` (non-fatal, gracefully skipped, per `reqwest/
        // streamable_http_client.rs::get_stream`).
        sendEmpty(res, 405);
        return;
      }
      if (req.method === "DELETE") {
        // Session teardown -- no-op (no session was ever allocated); 200 either way.
        sendEmpty(res, 200);
        return;
      }
      if (req.method !== "POST") {
        sendEmpty(res, 405);
        return;
      }

      const chunks = [];
      req.on("data", (chunk) => chunks.push(chunk));
      req.on("end", () => {
        let message;
        try {
          message = JSON.parse(Buffer.concat(chunks).toString("utf8"));
        } catch (e) {
          sendJson(res, 400, {
            jsonrpc: "2.0",
            id: null,
            error: { code: -32700, message: `parse error: ${e.message}` },
          });
          return;
        }
        const { status, body } = handleRpcMessage(message);
        if (body === null) {
          sendEmpty(res, status);
          return;
        }
        sendJson(res, status, body);
      });
      req.on("error", () => {
        // Best-effort: connection aborted mid-body -- nothing left to respond to.
      });
    });

    server.on("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      const port = typeof address === "object" && address ? address.port : null;
      if (port == null) {
        reject(new Error("startStubMcpServer: could not resolve the bound port"));
        return;
      }
      resolve({
        port,
        url: `http://127.0.0.1:${port}${MCP_PATH}`,
        close: () => new Promise((res2) => server.close(() => res2())),
      });
    });
  });
}
