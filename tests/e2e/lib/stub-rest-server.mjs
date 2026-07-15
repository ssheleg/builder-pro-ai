// Minimal Node HTTP stub for the `generic-rest` connector adapter (S-EXT spec §7's reference
// `ConnectorAdapter`, `crates/orchd/src/connectors/adapter.rs::GenericRestAdapter`) — the
// counterpart, on the JS side, to `adapter.rs`'s own in-crate `spawn_rest_stub` axum/TcpListener
// test helper (same shape, deliberately dependency-free Node `http` here since this file is what
// `tests/e2e/orchd-survive.mjs`'s phase7 spawns to drive the REAL `bpa-orchd` binary end to end,
// mirroring `stub-mcp-server.mjs`'s own "the Node counterpart to the Rust loopback stub" role for
// phase6).
//
// `GenericRestAdapter::invoke` (verified against `adapter.rs`) drives exactly two ops against
// this stub:
//   - `get`  -> `self.client.get(url)` (no body, `Authorization: Bearer <token>` header only)
//   - `post` -> `self.client.post(url).json(&args["body"])` (`Authorization: Bearer <token>` +
//               a JSON request body)
// Either way it expects a `2xx` JSON response body back (`response.json::<serde_json::Value>()`)
// — a non-2xx status maps to `ConnectorError::UpstreamStatus` instead. This stub always answers
// `200 application/json` with `{ok: true, echo: <parsed request body, or null for a bodyless
// GET>}` — the caller-supplied JSON payload round-trips into `echo` so a phase can assert its own
// planted marker survived the adapter's HTTP round trip end to end (into `McpCallResult.
// contentJson` / the persisted `mcp_artifact.content_json`, exactly like `stub-mcp-server.mjs`'s
// echo tool proves the same thing for the MCP path in phase6).

import http from "node:http";

function sendJson(res, status, body) {
  const buf = Buffer.from(JSON.stringify(body), "utf8");
  res.writeHead(status, {
    "Content-Type": "application/json",
    "Content-Length": buf.length,
  });
  res.end(buf);
}

/**
 * Start the stub generic-rest target on `127.0.0.1:0` (OS-assigned loopback port). Resolves once
 * bound, with `{ port, url, lastAuthHeader(), close() }`:
 *   - `url` is `http://127.0.0.1:<port>/` — exactly the shape `ConnectorInvoke`'s
 *     `args_json.url` expects (`GenericRestAdapter::invoke` reads `args["url"]` verbatim, no path
 *     convention assumed — this stub answers every path the SAME way, so `/` is as good as any).
 *   - `lastAuthHeader()` returns the `Authorization` header value captured off the MOST RECENT
 *     request (or `null` if none yet) — lets a phase assert the account's bearer genuinely
 *     reached the wire (spec §7: "using the account's bearer as `Authorization: Bearer <token>`"),
 *     mirroring `adapter.rs`'s own `CapturedRequest.auth_header` test helper.
 */
export function startStubRestServer() {
  return new Promise((resolve, reject) => {
    let lastAuthHeader = null;

    const server = http.createServer((req, res) => {
      const chunks = [];
      req.on("data", (chunk) => chunks.push(chunk));
      req.on("end", () => {
        lastAuthHeader = req.headers.authorization ?? null;

        let parsedBody = null;
        if (chunks.length > 0) {
          try {
            parsedBody = JSON.parse(Buffer.concat(chunks).toString("utf8"));
          } catch {
            sendJson(res, 400, { ok: false, error: "invalid json body" });
            return;
          }
        }
        sendJson(res, 200, { ok: true, echo: parsedBody });
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
        reject(new Error("startStubRestServer: could not resolve the bound port"));
        return;
      }
      resolve({
        port,
        url: `http://127.0.0.1:${port}/`,
        lastAuthHeader: () => lastAuthHeader,
        close: () => new Promise((res2) => server.close(() => res2())),
      });
    });
  });
}
