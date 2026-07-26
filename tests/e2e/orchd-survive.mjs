#!/usr/bin/env node
// E2E orchd survive-restart + export/import round-trip + cross-project-graph harness
// (S3 spec §12, S4 spec §8).
//
// Proves the S3+S4 roadmap DoD entirely at the daemon layer, over the real Hop-B `bpa-orchd` wire
// protocol (spec §4.2), without driving the WKWebView / Tauri GUI:
//
//   boot orchd on a temp socket -> preamble handshake [1,1] -> create a project + goal tree +
//   idea + task -> OrchdShutdown{drain:true} -> relaunch the SAME binary against the SAME state
//   dir -> data intact -> ExportAll -> shutdown -> delete orchd.db* -> relaunch (fresh schema v1)
//   -> ImportBundle(the earlier export) -> ExportAll again -> re-export equals the original
//   modulo exportedAt (+ the one documented boot-reseed exception below) -> create 2 projects ->
//   add a node to each -> add a CROSS-PROJECT edge between them -> OrchdShutdown{drain:true} ->
//   relaunch -> GraphListProject(P1) still shows the edge AND P2's node surfaces as an
//   `externalNodes` ghost (S4 spec §8 DoD: "a cross-project link survives BOTH projects' restart").
//
// Exit code 0 = full pass. Any failed assertion throws, is logged with a diagnostic message, and
// the process exits non-zero. No phase is skipped or weakened to pass vacuously.
//
// Run: `npm run e2e:orchd`.
//
// ---- why this file does NOT reuse `connect()`/`request()` from `lib/daemon-harness.mjs` ----
// `daemon-harness.mjs`'s `connect()` installs sessiond's `Frame`/`Request`/`Response`/`Push`
// frame codec (`decodeFrame`/`encodeFrame`, `crates/protocol/src/lib.rs`) — orchd speaks a
// COMPLETELY DIFFERENT frame contract (`bpa-orchd-proto::OrchdFrame`, `crates/orchd-proto/src/
// lib.rs`): different `OrchdRequest`/`OrchdResponse`/`OrchdPush` variant sets, a different
// wire-version space (`[1,1]`, not `[3,3]`). Reusing `connect()` unmodified would decode every
// orchd reply through the WRONG variant table; making `connect()` generic over an arbitrary frame
// codec was out of scope for this task's additive-only mandate on `daemon-harness.mjs` ("gains an
// optional `{clientMin, clientMax}` ... so `survive-restart.mjs` needs NO change" — spec §12).
// So this file reuses only the protocol-agnostic PARTS of the harness — `preambleHandshake`
// (now parameterized for orchd's `[1,1]` range), the hand-rolled standard-CBOR `cborEncode`/
// `cborDecode` primitives, and the process-lifecycle helpers (`spawnDaemon`, `pidAlive`,
// `killProcessGroup`) — and supplies its own tiny orchd frame encode/decode + connect/request
// pair below, mirroring `daemon-harness.mjs`'s own `connect()`/`request()` shape byte-for-byte.

import assert from "node:assert/strict";
import fs from "node:fs";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import { setTimeout as sleep } from "node:timers/promises";
import {
  preambleHandshake,
  cborEncode,
  cborDecode,
  cborFloat,
  spawnDaemon,
  pidAlive,
  killProcessGroup,
} from "./lib/daemon-harness.mjs";
import {
  startStubMcpServer,
  RESEARCH_TOOL_NAME,
  RESEARCH_FINDINGS_MARKER,
} from "./lib/stub-mcp-server.mjs";
import { startStubRestServer } from "./lib/stub-rest-server.mjs";

const REPO = path.resolve(import.meta.dirname, "..", "..");
const ORCHD_BIN = process.env.BPA_ORCHD ?? path.join(REPO, "target", "debug", "bpa-orchd");

// orchd's own independent version space (spec §4.2 D8): `ORCHD_CLIENT_MIN/MAX_VERSION = 1` in
// `crates/orchd-proto/src/lib.rs`, both pinned to `1` (first wire version).
const ORCHD_CLIENT_MIN_VERSION = 1;
const ORCHD_CLIENT_MAX_VERSION = 1;

function log(msg) {
  console.log(`[e2e-orchd] ${msg}`);
}

/** Best-effort binary sanity check (mirrors `survive-restart.mjs::assertRealBinary`): refuse to
 * run against a missing binary or the S0-era placeholder shell-script stub. */
function assertRealBinary(binPath) {
  assert.ok(
    fs.existsSync(binPath),
    `daemon binary missing at ${binPath} (build with: cargo build -p bpa-orchd)`,
  );
  const head = Buffer.alloc(64);
  const fd = fs.openSync(binPath, "r");
  const n = fs.readSync(fd, head, 0, 64, 0);
  fs.closeSync(fd);
  const text = head.subarray(0, n).toString("utf8");
  assert.ok(
    !text.startsWith("#!/bin/sh") && !text.startsWith("#!/usr/bin/env sh"),
    `daemon binary at ${binPath} is a shell-script placeholder, not a real build — run: ` +
      `cargo build -p bpa-orchd (see tests/e2e/README.md)`,
  );
}

// ============================================================================
// orchd wire: OrchdFrame/OrchdRequest/OrchdResponse/OrchdPush CBOR shapes
// (`crates/orchd-proto/src/lib.rs`, spec §4.2 — LOCKED, enum order FROZEN append-only).
//
// `OrchdFrame`/`OrchdRequest`/`OrchdResponse`/`OrchdPush` are Hop-B wire-only and carry NO
// `#[serde(rename_all)]` — externally tagged, plain snake_case field names on the wire (exactly
// mirrors `bpa-protocol`'s `Frame`/`Request`/`Response`/`Push` convention that
// `daemon-harness.mjs`'s `encodeRequest`/`decodeFrame` already document). The nested ENTITY types
// (`Project`, `Goal`, `Idea`, `DomainTask`, `RuleSetView`, their enums, `OrchdErrorCode`) DO
// carry `#[serde(rename_all = "camelCase")]` (spec §4.2) — once CBOR-decoded they are already
// plain camelCase JS objects, so no per-field translation is needed on the way OUT; only the
// hand-constructed REQUESTS below need the JS-camelCase -> wire-snake_case mapping.
// ============================================================================

/** Encode an `OrchdRequest` (this harness only ever SENDS `OrchdRequest`s) to its externally-
 * tagged CBOR shape. `req.t` selects the variant; unit variants -> bare string, struct variants
 * -> single-key map with snake_case fields. Only the verbs spec §12's phases actually use are
 * implemented — an unimplemented verb throws loudly rather than silently miscoding. */
function encodeOrchdRequest(req) {
  switch (req.t) {
    case "Ping":
      return "Ping";
    case "ListProjects":
      return "ListProjects";
    case "ExportAll":
      return "ExportAll";
    case "CreateProject":
      return {
        CreateProject: {
          name: req.name,
          description: req.description,
          workspace_ids: req.workspaceIds,
        },
      };
    case "ListGoals":
      return { ListGoals: { project_id: req.projectId } };
    case "CreateGoal":
      return {
        CreateGoal: {
          project_id: req.projectId,
          parent_id: req.parentId ?? null,
          kind: req.kind, // already the wire string ("strategic" | "additional")
          title: req.title,
          body: req.body,
        },
      };
    case "CreateIdea":
      return {
        CreateIdea: {
          project_id: req.projectId ?? null,
          title: req.title,
          body: req.body,
        },
      };
    case "SetIdeaLifecycle":
      return { SetIdeaLifecycle: { id: req.id, lifecycle: req.lifecycle } }; // already the wire string
    case "ListIdeas":
      return { ListIdeas: { project_id: req.projectId ?? null } };
    // Insight (spec §4.2) — only the verbs the S-IDEA e2e phases (phase8) drive.
    case "CreateInsight":
      return {
        CreateInsight: {
          project_id: req.projectId ?? null,
          source: req.source,
          title: req.title,
          body: req.body,
        },
      };
    case "SetInsightFitVerdict":
      return {
        SetInsightFitVerdict: {
          id: req.id,
          fit_verdict: req.fitVerdict ?? null, // already the wire string ("fit"|"noFit"|"unknown") or null
          fit_reasoning: req.fitReasoning,
        },
      };
    case "SetInsightStatus":
      return {
        SetInsightStatus: {
          id: req.id,
          status: req.status, // already the wire string ("new"|"accepted"|"archived")
          resolution_reasoning: req.resolutionReasoning ?? null,
        },
      };
    case "ListInsights":
      return { ListInsights: { project_id: req.projectId ?? null } };
    case "CreateTask":
      return {
        CreateTask: {
          project_id: req.projectId,
          parent_id: req.parentId ?? null,
          title: req.title,
          body: req.body,
          status: req.status ?? null,
          source: req.source, // already the wire string ("idea" | "insight" | "bug" | "plan")
          source_id: req.sourceId ?? null,
          tags: req.tags ?? [],
        },
      };
    case "ListTasks":
      return { ListTasks: { project_id: req.projectId ?? null } };
    case "OrchdShutdown":
      return { OrchdShutdown: { drain: !!req.drain } };
    case "ImportBundle":
      return { ImportBundle: { json: req.json } };
    // S4 knowledge graph (spec §3/§8) — only the 3 verbs phase5 actually drives.
    case "GraphAddNode":
      return {
        GraphAddNode: {
          project_id: req.projectId,
          kind: req.kind, // already the wire string (e.g. "concept" — see GraphNodeKind)
          label: req.label,
          body: req.body,
          // `pos_x`/`pos_y` are Rust `f64` — `cborFloat` forces a CBOR float encoding even when
          // the test's coordinate happens to be a whole number (ciborium's `f64` deserialize
          // rejects a CBOR integer outright rather than coercing it).
          pos_x: cborFloat(req.posX),
          pos_y: cborFloat(req.posY),
        },
      };
    case "GraphAddEdge":
      return {
        GraphAddEdge: {
          source_node_id: req.sourceNodeId,
          target_node_id: req.targetNodeId,
          kind: req.kind, // already the wire string (e.g. "relates" — see GraphEdgeKind)
          label: req.label,
        },
      };
    case "GraphListProject":
      return { GraphListProject: { project_id: req.projectId } };
    // S-EXT MCP (spec §5, appended — order FROZEN append-only) — only the 6 verbs phase6 drives.
    // `McpTransport`/`McpScope`/`McpAuthKind` are unit-variant entity enums with `#[serde(rename_
    // all = "camelCase")]` (spec §5 comment: "the new entity structs ... DO carry
    // #[serde(rename_all="camelCase")]"), so callers pass the already-lowercased wire string
    // directly (e.g. `"http"`/`"global"`/`"none"`) — verified against the committed
    // `src/ipc/orchd-types.ts` (`McpTransport = "http" | "stdio"`, `McpScope = "global" |
    // "project"`, `McpAuthKind = "none" | "bearer" | "oauth"`), same convention as `GraphAddNode`'s
    // `kind`/`GraphAddEdge`'s `kind` above.
    case "McpAddServer":
      return {
        McpAddServer: {
          name: req.name,
          transport: req.transport,
          url: req.url ?? null,
          command: req.command ?? null,
          args: req.args ?? null,
          env: req.env ?? null,
          scope: req.scope,
          project_id: req.projectId ?? null,
          auth_kind: req.authKind,
          timeout_ms: req.timeoutMs ?? null,
          max_retries: req.maxRetries ?? null,
        },
      };
    case "TrustGrantConsent":
      return { TrustGrantConsent: { server_id: req.serverId, kind: req.kind } };
    case "McpConnect":
      return { McpConnect: { id: req.id } };
    case "McpListTools":
      return { McpListTools: { server_id: req.serverId } };
    case "McpCallTool":
      return {
        McpCallTool: {
          server_id: req.serverId,
          tool_name: req.toolName,
          args_json: req.argsJson,
          project_id: req.projectId ?? null,
        },
      };
    case "McpListArtifacts":
      return {
        McpListArtifacts: {
          project_id: req.projectId ?? null,
          server_id: req.serverId ?? null,
          limit: req.limit ?? null,
        },
      };
    // S-EXT Connectors / accounts (spec §5/§7, appended — order FROZEN append-only) — only the 4
    // verbs phase7 drives. `ConnectorListAccounts` is a unit variant (no fields), same bare-string
    // convention as `ListProjects`/`ExportAll` above.
    case "ConnectorAddApiKey":
      return {
        ConnectorAddApiKey: {
          provider: req.provider,
          label: req.label,
          api_key: req.apiKey,
        },
      };
    case "ConnectorListAccounts":
      return "ConnectorListAccounts";
    case "ConnectorInvoke":
      return {
        ConnectorInvoke: {
          account_id: req.accountId,
          op: req.op,
          args_json: req.argsJson,
          project_id: req.projectId ?? null,
        },
      };
    case "ConnectorDeleteAccount":
      return { ConnectorDeleteAccount: { id: req.id } };
    // S-IDEA research (spec §5, task T3, appended — order FROZEN append-only) — only the 3 verbs
    // phase8/phase9 drive.
    case "ResearchStartRun":
      return {
        ResearchStartRun: {
          idea_id: req.ideaId,
          server_id: req.serverId,
          tool_name: req.toolName,
          args_json: req.argsJson,
        },
      };
    case "ResearchListRuns":
      return { ResearchListRuns: { idea_id: req.ideaId } };
    case "ResearchGetRun":
      return { ResearchGetRun: { id: req.id } };
    default:
      throw new Error(`encodeOrchdRequest: unsupported request type ${req.t}`);
  }
}

/** Encode an `OrchdFrame::Request { id, req }` -> `{ "Request": { "id": <u64>, "req": <...> } }`,
 * `u32`-LE length-prefixed (mirrors `daemon-harness.mjs::encodeFrame`). */
function encodeOrchdFrame(frame) {
  if (frame.t !== "Request") throw new Error("orchd harness only sends Request frames");
  const cborValue = { Request: { id: frame.id, req: encodeOrchdRequest(frame.req) } };
  const body = cborEncode(cborValue);
  const out = Buffer.alloc(4 + body.length);
  out.writeUInt32LE(body.length, 0);
  body.copy(out, 4);
  return out;
}

/** Decode a top-level `OrchdFrame` (externally tagged): `{"Response": {id, res}}` or
 * `{"Push": <push>}`. The harness never decodes `Request` frames (client -> daemon only). */
function decodeOrchdFrame(buf) {
  const value = cborDecode(buf);
  const keys = Object.keys(value);
  if (keys.length !== 1) {
    throw new Error(`decodeOrchdFrame: expected single-key OrchdFrame map, got keys ${JSON.stringify(keys)}`);
  }
  const [variant] = keys;
  if (variant === "Response") {
    const { id, res } = value.Response;
    return { t: "Response", id, res: decodeOrchdResponse(res) };
  }
  if (variant === "Push") {
    return { t: "Push", push: decodeOrchdPush(value.Push) };
  }
  if (variant === "Request") {
    throw new Error("orchd harness does not decode Request frames");
  }
  throw new Error(`decodeOrchdFrame: unknown OrchdFrame variant ${variant}`);
}

/** Decode an `OrchdResponse`. `Ack`/`Pong` are unit variants -> bare strings; every entity-
 * returning variant (`Project`, `Projects`, `Goal`, ... `RuleSetView`, `ExportJson`) is a NEWTYPE
 * variant whose inner payload is already camelCase-decoded by the generic CBOR map decode (the
 * entity structs carry `#[serde(rename_all = "camelCase")]`) — handed back unchanged as
 * `.value`. `ImportReport`/`Error` are struct variants, decoded explicitly. */
function decodeOrchdResponse(value) {
  if (value === "Ack") return { t: "Ack" };
  if (value === "Pong") return { t: "Pong" };
  const keys = Object.keys(value);
  if (keys.length !== 1) {
    throw new Error(`decodeOrchdResponse: expected single-key map, got ${JSON.stringify(value)}`);
  }
  const [variant] = keys;
  const inner = value[variant];
  switch (variant) {
    case "Project":
    case "Projects":
    case "Goal":
    case "Goals":
    case "Idea":
    case "Ideas":
    case "Insight":
    case "Insights":
    case "Task":
    case "Tasks":
    case "RuleSetView":
    case "ExportJson":
    // S4 knowledge graph (spec §3/§8) — `GraphNode`/`GraphEdge` are newtype variants over
    // camelCase-serde entity structs (like `Project`/`Goal` above); `GraphView` is a newtype over
    // `{ nodes, edges, externalNodes }`, same camelCase convention — no extra per-field mapping
    // needed, `inner` already decodes with those exact JS keys.
    case "GraphNode":
    case "GraphEdge":
    case "GraphView":
    // S-EXT MCP (spec §5, appended — order FROZEN append-only): `McpServer`/`McpTool`/
    // `McpConnectReport`/`McpCallResult`/`McpArtifact` are newtype variants over camelCase-serde
    // entity structs (same convention as `GraphNode`/`GraphEdge`/`GraphView` above — verified
    // against `src/ipc/orchd-types.ts`'s generated field names, e.g. `McpConnectReport =
    // {protocolVersion, toolCount}`, `McpCallResult = {artifactId, invocationId, contentJson,
    // isError}`, `McpArtifact = {id, invocationId, serverId, toolName, projectId, contentJson,
    // contentText, isUntrusted, createdAt}`); `McpServers`/`McpTools`/`McpInvocations`/
    // `McpArtifacts` are the `Vec<...>` newtype siblings, decoded identically.
    case "McpServer":
    case "McpServers":
    case "McpTool":
    case "McpTools":
    case "McpConnectReport":
    case "McpCallResult":
    case "McpInvocations":
    case "McpArtifacts":
    case "McpArtifact":
    // S-EXT Connectors / accounts (spec §5/§7, appended — order FROZEN append-only): `Account`
    // is a newtype variant over a camelCase-serde entity struct (same convention as `McpServer`/
    // `McpArtifact` above — verified against `src/ipc/orchd-types.ts`'s generated field names,
    // `Account = {id, provider, label, authKind, scopes, expiresAt, createdAt, updatedAt}`);
    // `Accounts` is the `Vec<Account>` newtype sibling, decoded identically. `ConnectorInvoke`'s
    // own success payload is `McpCallResult` (reuses the MCP call/artifact/invocation path, spec
    // §6), already decoded above — no separate case needed for it.
    case "Account":
    case "Accounts":
    // S-IDEA research (spec §5, task T3, appended — order FROZEN append-only): `ResearchRun` is a
    // newtype variant over a camelCase-serde entity struct (same convention as `McpArtifact`/
    // `Account` above — verified against `crates/orchd-proto/src/lib.rs`'s `ResearchRun` field
    // set: `{id, ideaId, serverId, toolName, argsJson, status, invocationId, artifactId,
    // errorKind, createdAt, updatedAt}`); `ResearchRuns` is the `Vec<ResearchRun>` newtype
    // sibling, decoded identically.
    case "ResearchRun":
    case "ResearchRuns":
      return { t: variant, value: inner };
    case "ImportReport":
      return {
        t: "ImportReport",
        projects: inner.projects,
        goals: inner.goals,
        ideas: inner.ideas,
        insights: inner.insights,
        tasks: inner.tasks,
        rulesets: inner.rulesets,
      };
    case "Error":
      return { t: "Error", code: inner.code, message: inner.message };
    default:
      throw new Error(`decodeOrchdResponse: unknown OrchdResponse variant ${variant}`);
  }
}

/** Decode an `OrchdPush` (spec §4.2, D10 `orchd://` prefix on the frontend side — this harness
 * only needs the raw wire shape). `ProjectsChanged`/`IdeasChanged`/`InsightsChanged` are unit
 * variants -> bare strings; `GoalsChanged`/`TasksChanged`/`RuleSetChanged` are struct variants.
 * The full enum is decoded (not just the subset phase1's mutations trigger) so an unexpected-but-
 * valid push never throws mid-test — `sock.on("data", ...)` runs this decode synchronously on
 * every inbound push, whether or not the test ever awaits it.
 *
 * S-EXT Connectors (spec §5/§7, appended — order FROZEN append-only): `ConnectorsChanged` is
 * ALSO a unit variant (no `project_id` — the `account` table has no such column, spec §5 comment)
 * — it falls through the SAME bare-string branch immediately below as `ProjectsChanged`/
 * `IdeasChanged`/`InsightsChanged`, so no dedicated `case` is needed for it in the switch below;
 * `ConnectorInvoke`'s OTHER push, `McpArtifactsChanged`, is a struct variant already decoded
 * further down (shared with `McpCallTool`'s own success path). */
function decodeOrchdPush(value) {
  if (typeof value === "string") {
    return { t: value };
  }
  const keys = Object.keys(value);
  if (keys.length !== 1) {
    throw new Error(`decodeOrchdPush: expected single-key map, got ${JSON.stringify(value)}`);
  }
  const [variant] = keys;
  const inner = value[variant];
  switch (variant) {
    case "GoalsChanged":
      return { t: "GoalsChanged", projectId: inner.project_id };
    case "TasksChanged":
      return { t: "TasksChanged", projectId: inner.project_id };
    case "RuleSetChanged":
      return { t: "RuleSetChanged", scope: inner.scope, projectId: inner.project_id ?? null };
    // S4 knowledge graph (spec §3/§8): `GraphChanged { project_id }` — same struct-variant /
    // snake_case-on-the-wire convention as `GoalsChanged`/`TasksChanged` above.
    case "GraphChanged":
      return { t: "GraphChanged", projectId: inner.project_id };
    // S-EXT MCP (spec §5, appended — order FROZEN append-only): coarse-invalidation pushes, all
    // struct variants with snake_case wire fields (same convention as `GoalsChanged`/
    // `TasksChanged`/`GraphChanged` above). Decoded (not just tolerated) so an unexpected-but-valid
    // push emitted mid-phase6 (e.g. `McpArtifactsChanged` after `McpCallTool`) never throws —
    // mirrors this function's own doc comment ("the full enum is decoded ... so an
    // unexpected-but-valid push never throws mid-test").
    case "McpServersChanged":
      return { t: "McpServersChanged", projectId: inner.project_id ?? null };
    case "McpToolsChanged":
      return { t: "McpToolsChanged", serverId: inner.server_id };
    case "McpArtifactsChanged":
      return { t: "McpArtifactsChanged", projectId: inner.project_id ?? null };
    case "McpInvocationLogged":
      return { t: "McpInvocationLogged", serverId: inner.server_id };
    // S-IDEA research (spec §5, task T3, appended — order FROZEN append-only): fired after a
    // research run's status changes (start/running/done/failed). `idea_id: Option<String>` — same
    // optional-scope shape as `McpServersChanged`/`McpArtifactsChanged` above.
    case "ResearchRunsChanged":
      return { t: "ResearchRunsChanged", ideaId: inner.idea_id ?? null };
    default:
      throw new Error(`decodeOrchdPush: unknown OrchdPush variant ${variant}`);
  }
}

/**
 * Connect to `sockPath`, perform the orchd preamble handshake (`[1,1]`), then install the CBOR
 * frame-stream reader for the remainder of the connection's life. Mirrors
 * `daemon-harness.mjs::connect()` structurally, but instantiated over THIS file's orchd frame
 * codec instead of sessiond's.
 */
function orchdConnect(sockPath) {
  return new Promise((resolve, reject) => {
    const sock = net.connect(sockPath);
    sock.once("connect", async () => {
      try {
        const { chosen, daemonBuild, leftover } = await preambleHandshake(sock, {
          clientMin: ORCHD_CLIENT_MIN_VERSION,
          clientMax: ORCHD_CLIENT_MAX_VERSION,
        });
        const conn = {
          sock,
          buf: leftover,
          pending: [],
          pushes: [],
          waiters: [],
          chosenVersion: chosen,
          daemonBuild,
        };
        sock.on("data", (chunk) => {
          conn.buf = Buffer.concat([conn.buf, chunk]);
          for (;;) {
            if (conn.buf.length < 4) break;
            const len = conn.buf.readUInt32LE(0);
            if (conn.buf.length < 4 + len) break;
            const body = conn.buf.subarray(4, 4 + len);
            conn.buf = conn.buf.subarray(4 + len);
            const frame = decodeOrchdFrame(body);
            if (frame.t === "Response") {
              const w = conn.pending.find((p) => p.id === frame.id);
              if (w) {
                conn.pending = conn.pending.filter((p) => p !== w);
                w.resolve(frame.res);
              }
            } else if (frame.t === "Push") {
              conn.pushes.push(frame.push);
              const w = conn.waiters.find((x) => x.pred(frame.push));
              if (w) {
                conn.waiters = conn.waiters.filter((x) => x !== w);
                w.resolve(frame.push);
              }
            }
          }
        });
        resolve(conn);
      } catch (e) {
        sock.destroy();
        reject(e);
      }
    });
    sock.once("error", reject);
  });
}

let nextOrchdId = 1;

// Default per-request timeout (10s) catches a genuinely hung daemon fast. A few requests are
// legitimately slower than a normal round trip — chiefly `OrchdShutdown{drain:true}`, which flushes
// scrollback rings + domain state to SQLite before acking; under CI's `-O0` + coverage-instrumented
// (`cargo-llvm-cov`) build that drain can exceed 10s on a loaded macOS runner. Those callers pass a
// larger `timeoutMs` explicitly rather than globally loosening the hang-detection budget.
function orchdRequest(conn, req, id = nextOrchdId++, timeoutMs = 10000) {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      conn.pending = conn.pending.filter((p) => p.id !== id);
      reject(new Error(`orchd request ${req.t} timed out`));
    }, timeoutMs);
    conn.pending.push({
      id,
      resolve: (v) => {
        clearTimeout(timer);
        resolve(v);
      },
      reject,
    });
    conn.sock.write(encodeOrchdFrame({ t: "Request", id, req }));
  });
}

// ---- state isolation (never touch the real user socket/DB at /tmp/bpa-<uid>,
// $XDG_RUNTIME_DIR/bpa, or ~/Library/Application Support/ai.builderpro.desktop) ----
//
// `bpa-orchd` resolves BOTH its single-instance lockfile (`crates/daemon-core/src/singleton.rs`
// `resolve_lockfile("orchd.lock")`) and its default socket dir off `XDG_RUNTIME_DIR` — even
// though this harness always passes an explicit `--socket <path>` (see `spawnDaemon`),
// `main.rs::main()` unconditionally calls `ensure_socket_dir()` + `acquire_single_instance_lock`
// against the DEFAULT resolved path first, so `XDG_RUNTIME_DIR` must be isolated to avoid
// colliding with (or SIGTERM-ing during cleanup) a real running orchd on this machine. Its
// DURABLE state (the SQLite DB + the `rules/` markdown tree) is a SEPARATE resolution path keyed
// off `HOME` (`crates/daemon-core/src/dirs.rs::app_support_dir` ->
// `~/Library/Application Support/ai.builderpro.desktop`), isolated the same way
// `survive-restart.mjs` isolates it — a fresh `mkdtemp` HOME reused verbatim across every
// spawn/relaunch in this file, so the REAL app-support tree (specifically its `rules/` dir, the
// one thing this task's DoD explicitly calls out) never grows by a single file.
const isolatedTmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "bpa-e2e-orchd-"));
const isolatedHomeDir = fs.mkdtempSync(path.join(os.tmpdir(), "bpa-e2e-orchd-home-"));
const daemonEnvOverrides = { XDG_RUNTIME_DIR: isolatedTmpDir, HOME: isolatedHomeDir };
const SOCK = path.join(isolatedTmpDir, "bpa", "orchd.sock");
const APP_SUPPORT_DIR = path.join(
  isolatedHomeDir,
  "Library",
  "Application Support",
  "ai.builderpro.desktop",
);
const DB_PATH = path.join(APP_SUPPORT_DIR, "orchd.db");

// ---- Keychain access under the isolated HOME (S-EXT phase7: `ConnectorAddApiKey` writes a real
// macOS Keychain entry via `bpa_secrets`/`security_framework::passwords::*`, which resolves the
// default (login) Keychain off a `$HOME`-derived path). A bare synthetic `$HOME` pointing at a
// fresh, otherwise-empty tempdir (like `isolatedHomeDir` above) has no `Library/Keychains`
// subtree, so Security.framework finds no keychain there and attempts to provision a brand-new
// one — which requires an interactive "choose a password" prompt that hangs forever in this
// harness's non-interactive run. Symlinking `Library/Keychains` from the REAL `$HOME` into the
// isolated one fixes this (confirmed empirically against this exact daemon binary): the OS's
// default-keychain path resolution then finds the SAME keychain this test process could already
// read/write directly — no new access is granted, only where to look. This mirrors
// `crates/orchd/tests/dispatch_integration.rs`'s `HomeGuard::set` byte-for-byte (S-EXT task T13a's
// own discovery of the identical problem for orchd's in-process Rust integration tests). Applied
// unconditionally, before phase0's very first boot: harmless for every earlier phase (none of
// them touch Keychain), and required starting phase7.
{
  const realHomeDir = os.homedir();
  const keychainsLink = path.join(isolatedHomeDir, "Library", "Keychains");
  fs.mkdirSync(path.dirname(keychainsLink), { recursive: true });
  fs.symlinkSync(path.join(realHomeDir, "Library", "Keychains"), keychainsLink);
}

/**
 * Cleanup state tracked across phases so the top-level `finally` in `main()` can tear everything
 * down on ANY exit path — success, a failed assertion mid-phase, or a request timeout.
 */
const cleanup = {
  daemonPid: null,
  conns: [],
  stubMcpServer: null,
  stubRestServer: null,
  // Set while a phase7 connector account exists but hasn't been explicitly `ConnectorDeleteAccount`-
  // ed yet, so `cleanupAll()` can best-effort clean up the REAL Keychain entry it created even if a
  // mid-phase assertion throws before the happy-path deletion runs (see phase7 below).
  connectorAccountId: null,
  // Same, for the throwaway keychain-availability probe account phase7 creates FIRST (also a REAL
  // Keychain entry) — normally deleted within a couple of lines, but tracked for the tiny window
  // between its create and its delete so a delete failure never orphans it either.
  connectorProbeAccountId: null,
};

// BL-106: tracks whether the connector/keychain survival phase actually RAN its full happy path
// (vs. SKIPped on an unavailable keychain). CI provisions an unlocked search-list keychain, so a
// skip THERE is a keychain-provisioning regression that must fail the gate, not pass vacuously —
// see the assertion in `main()` gated on BPA_REQUIRE_KEYCHAIN=1.
let phase7Ran = false;

/** Poll `pidAlive` until `pid` exits or `deadlineMs` elapses; throws if it never exits. */
async function waitForExit(pid, deadlineMs, what) {
  const deadline = Date.now() + deadlineMs;
  while (pidAlive(pid) && Date.now() < deadline) {
    await sleep(100);
  }
  assert.ok(!pidAlive(pid), `${what} (pid ${pid}) did not exit within ${deadlineMs}ms`);
}

/** Spawn (or relaunch) the daemon at `SOCK` against the isolated state dir, and connect+handshake
 * a fresh client to it, retrying the connect for up to 5s (daemon startup is async). */
async function bootAndConnect() {
  const daemonProc = spawnDaemon(ORCHD_BIN, SOCK, daemonEnvOverrides);
  cleanup.daemonPid = daemonProc.pid;

  let conn;
  for (let i = 0; i < 50; i++) {
    try {
      conn = await orchdConnect(SOCK);
      break;
    } catch {
      await sleep(100);
    }
  }
  assert.ok(conn, `could not connect to orchd socket at ${SOCK} within 5s`);
  cleanup.conns.push(conn);
  return conn;
}

/** `OrchdShutdown{drain:true}` over `conn`, then wait for the OS process (`cleanup.daemonPid`) to
 * actually exit. Drops every tracked connection afterward (they're all stale once the daemon
 * process is gone). */
async function shutdownAndWaitExit(conn) {
  // 60s: the drain-and-persist can run well past a normal request under CI instrumentation (see
  // `orchdRequest`'s doc comment). Local/dev acks in well under a second; the generous ceiling only
  // absorbs the slow-runner tail, it does not mask a real hang (a truly wedged drain still fails).
  const shutdownAck = await orchdRequest(conn, { t: "OrchdShutdown", drain: true }, undefined, 60000);
  assert.equal(shutdownAck.t, "Ack", `OrchdShutdown{drain:true} -> ${JSON.stringify(shutdownAck)}`);
  const daemonPid = cleanup.daemonPid;
  await waitForExit(daemonPid, 30000, "orchd");
  for (const c of cleanup.conns) {
    try {
      c.sock.destroy();
    } catch {
      /* already closed */
    }
  }
  cleanup.conns = [];
  return daemonPid;
}

/**
 * Strip the fields a byte-for-byte round-trip comparison must NOT expect to match, with a
 * documented reason for each:
 *
 *  - `exportedAt` (top-level, spec §8/§12: "re-export equals the original modulo exportedAt") —
 *    a fresh wall-clock stamp on every `ExportAll` call, never meaningful data.
 *  - `globalRuleset.rule.id` / `.createdAt` — the GLOBAL ruleset row is a boot-seeded singleton
 *    (`crates/orchd/src/boot.rs::ensure_global_ruleset`, spec §5.2 "ensured at every orchd boot").
 *    Phase 4 relaunches orchd against a FRESH (post-deletion) `orchd.db`, so boot re-seeds a NEW
 *    global ruleset row with a fresh random uuid/timestamp BEFORE `ImportBundle` runs; import then
 *    RECONCILES that already-seeded singleton in place (`ON CONFLICT(scope) WHERE scope='global'
 *    DO UPDATE` — `crates/orchd/src/persistence.rs::insert_ruleset_raw`, proved by the Rust unit
 *    test `import_into_a_boot_seeded_store_reconciles_the_global_ruleset` in
 *    `crates/orchd/src/export.rs`) rather than inserting the bundle's own row verbatim, so a
 *    SECOND global-scope row is never created and the whole-store import never collides on the
 *    `ruleset_single_global` partial unique index. That reconcile explicitly keeps the freshly-
 *    seeded row's `id`/`created_at` ("both boot impl details, not meaningful data" per that
 *    function's own doc comment) while overwriting `mdPath`/`mdHash`/`policy`/`updatedAt` with the
 *    bundle's — every one of those OTHER fields (plus every project/goal/idea/task/project-scoped
 *    ruleset, which are NOT boot-seeded singletons and so import verbatim) is asserted equal
 *    below; only this one documented, intentionally-non-preserved pair is excluded.
 */
function normalizeExportForRoundTrip(bundleJson) {
  const clone = JSON.parse(bundleJson);
  delete clone.exportedAt;
  if (clone.globalRuleset && clone.globalRuleset.rule) {
    delete clone.globalRuleset.rule.id;
    delete clone.globalRuleset.rule.createdAt;
  }
  return clone;
}

async function main() {
  assertRealBinary(ORCHD_BIN);

  // ---- phase 0: boot orchd on a temp socket, handshake [1,1] ----
  log(`phase0: spawn orchd ${ORCHD_BIN} (isolated XDG_RUNTIME_DIR=${isolatedTmpDir}, HOME=${isolatedHomeDir})`);
  let conn = await bootAndConnect();
  assert.equal(conn.chosenVersion, 1, `preamble negotiated unexpected version: ${JSON.stringify(conn)}`);
  log(`phase0 OK: preamble handshake (chosen=${conn.chosenVersion}, daemonBuild=${JSON.stringify(conn.daemonBuild)})`);

  // ---- phase 1: create a project + 2 additional goals under its auto-created strategic root +
  // an idea + a task; assert every entity comes back ----
  log("phase1: CreateProject + 2×CreateGoal + CreateIdea + CreateTask");

  const projectResp = await orchdRequest(conn, {
    t: "CreateProject",
    name: "E2E Project",
    description: "created by orchd-survive.mjs",
    workspaceIds: ["ws-e2e-orchd-1"],
  });
  assert.equal(projectResp.t, "Project", `CreateProject -> ${JSON.stringify(projectResp)}`);
  const projectId = projectResp.value.id;
  assert.equal(projectResp.value.workspaceIds.length, 1, "project must carry the one workspace_id");

  const goalsAfterCreate = await orchdRequest(conn, { t: "ListGoals", projectId });
  assert.equal(goalsAfterCreate.t, "Goals", `ListGoals -> ${JSON.stringify(goalsAfterCreate)}`);
  const strategicGoals = goalsAfterCreate.value.filter((g) => g.kind === "strategic");
  assert.equal(
    strategicGoals.length,
    1,
    `expected exactly 1 auto-created strategic goal, got ${JSON.stringify(goalsAfterCreate.value)}`,
  );
  const strategicGoalId = strategicGoals[0].id;

  const goalAResp = await orchdRequest(conn, {
    t: "CreateGoal",
    projectId,
    parentId: strategicGoalId,
    kind: "additional",
    title: "Goal A (e2e)",
    body: "",
  });
  assert.equal(goalAResp.t, "Goal", `CreateGoal(A) -> ${JSON.stringify(goalAResp)}`);
  const goalAId = goalAResp.value.id;

  const goalBResp = await orchdRequest(conn, {
    t: "CreateGoal",
    projectId,
    parentId: strategicGoalId,
    kind: "additional",
    title: "Goal B (e2e)",
    body: "",
  });
  assert.equal(goalBResp.t, "Goal", `CreateGoal(B) -> ${JSON.stringify(goalBResp)}`);
  const goalBId = goalBResp.value.id;

  const ideaResp = await orchdRequest(conn, {
    t: "CreateIdea",
    projectId,
    title: "Idea 1 (e2e)",
    body: "an idea captured by the e2e harness",
  });
  assert.equal(ideaResp.t, "Idea", `CreateIdea -> ${JSON.stringify(ideaResp)}`);

  const taskResp = await orchdRequest(conn, {
    t: "CreateTask",
    projectId,
    parentId: null,
    title: "Task 1 (e2e)",
    body: "a task captured by the e2e harness",
    status: null,
    source: "bug",
    sourceId: null,
    tags: ["e2e"],
  });
  assert.equal(taskResp.t, "Task", `CreateTask -> ${JSON.stringify(taskResp)}`);

  log(
    `phase1 OK: project ${projectId}, strategic goal ${strategicGoalId}, ` +
      `goals [${goalAId}, ${goalBId}], idea ${ideaResp.value.id}, task ${taskResp.value.id}`,
  );

  // ---- phase 2: OrchdShutdown{drain:true} -> relaunch the SAME binary against the SAME state
  // dir -> ListProjects + ListGoals return the persisted data intact ----
  log("phase2: OrchdShutdown{drain:true} -> relaunch -> ListProjects/ListGoals intact");
  await shutdownAndWaitExit(conn);
  log(`phase2 OK: orchd (pid ${cleanup.daemonPid}) process exited`);

  conn = await bootAndConnect();
  assert.equal(
    conn.chosenVersion,
    1,
    `post-relaunch preamble handshake negotiated unexpected version: ${JSON.stringify(conn)}`,
  );

  const projectsAfterRestart = await orchdRequest(conn, { t: "ListProjects" });
  assert.equal(projectsAfterRestart.t, "Projects", `ListProjects -> ${JSON.stringify(projectsAfterRestart)}`);
  const rehydratedProject = projectsAfterRestart.value.find((p) => p.id === projectId);
  assert.ok(
    rehydratedProject,
    `project ${projectId} lost across orchd restart (ListProjects returned: ` +
      `${JSON.stringify(projectsAfterRestart.value.map((p) => p.id))})`,
  );
  assert.equal(rehydratedProject.name, "E2E Project", "rehydrated project lost its name");
  assert.deepEqual(rehydratedProject.workspaceIds, ["ws-e2e-orchd-1"], "rehydrated project lost its workspaceIds");

  const goalsAfterRestart = await orchdRequest(conn, { t: "ListGoals", projectId });
  assert.equal(goalsAfterRestart.t, "Goals", `ListGoals -> ${JSON.stringify(goalsAfterRestart)}`);
  const rehydratedGoalIds = goalsAfterRestart.value.map((g) => g.id).sort();
  const expectedGoalIds = [strategicGoalId, goalAId, goalBId].sort();
  assert.deepEqual(
    rehydratedGoalIds,
    expectedGoalIds,
    `goal set lost across orchd restart: expected ${JSON.stringify(expectedGoalIds)}, got ` +
      `${JSON.stringify(rehydratedGoalIds)}`,
  );
  log(`phase2 OK: project ${projectId} + its 3 goals rehydrated intact after orchd restart`);

  // ---- phase 3: ExportAll, capture the JSON ----
  log("phase3: ExportAll");
  const exportResp1 = await orchdRequest(conn, { t: "ExportAll" });
  assert.equal(exportResp1.t, "ExportJson", `ExportAll -> ${JSON.stringify(exportResp1)}`);
  const exportJson1 = exportResp1.value;
  const parsedExport1 = JSON.parse(exportJson1); // throws loudly if the export isn't valid JSON
  assert.equal(parsedExport1.bundleFormat, 1, `unexpected bundleFormat: ${JSON.stringify(parsedExport1.bundleFormat)}`);
  assert.ok(
    parsedExport1.projects.some((p) => p.project.id === projectId),
    `ExportAll bundle is missing project ${projectId}`,
  );
  log(`phase3 OK: captured ExportAll bundle (${exportJson1.length} bytes)`);

  // ---- phase 4: shutdown -> delete orchd.db* -> relaunch (fresh schema v1) -> ImportBundle ->
  // ExportAll again -> assert deep-equal to phase3's export modulo exportedAt (+ the documented
  // global-ruleset boot-reseed exception, see normalizeExportForRoundTrip) ----
  log("phase4: shutdown -> delete orchd.db* -> relaunch (fresh v1) -> ImportBundle -> re-ExportAll");
  await shutdownAndWaitExit(conn);
  log(`phase4 OK: orchd (pid ${cleanup.daemonPid}) process exited`);

  for (const suffix of ["", "-wal", "-shm"]) {
    fs.rmSync(`${DB_PATH}${suffix}`, { force: true });
  }
  assert.ok(!fs.existsSync(DB_PATH), `${DB_PATH} still exists after deletion`);
  log(`phase4 OK: deleted ${DB_PATH}{,-wal,-shm}`);

  conn = await bootAndConnect();
  assert.equal(
    conn.chosenVersion,
    1,
    `post-fresh-boot preamble handshake negotiated unexpected version: ${JSON.stringify(conn)}`,
  );

  // Sanity: the fresh boot must NOT already know about the pre-deletion project (proves the
  // schema really is fresh v1, not a leftover WAL replay of the deleted DB).
  const projectsBeforeImport = await orchdRequest(conn, { t: "ListProjects" });
  assert.equal(projectsBeforeImport.t, "Projects", `ListProjects -> ${JSON.stringify(projectsBeforeImport)}`);
  assert.equal(
    projectsBeforeImport.value.length,
    0,
    `fresh v1 schema unexpectedly already has projects: ${JSON.stringify(projectsBeforeImport.value)}`,
  );

  const importResp = await orchdRequest(conn, { t: "ImportBundle", json: exportJson1 });
  assert.equal(importResp.t, "ImportReport", `ImportBundle -> ${JSON.stringify(importResp)}`);
  assert.equal(importResp.projects, 1, `ImportReport.projects -> ${JSON.stringify(importResp)}`);
  assert.equal(importResp.goals, 3, `ImportReport.goals -> ${JSON.stringify(importResp)}`); // strategic + A + B
  assert.equal(importResp.ideas, 1, `ImportReport.ideas -> ${JSON.stringify(importResp)}`);
  assert.equal(importResp.tasks, 1, `ImportReport.tasks -> ${JSON.stringify(importResp)}`);
  log(`phase4 OK: ImportBundle -> ${JSON.stringify(importResp)}`);

  const exportResp2 = await orchdRequest(conn, { t: "ExportAll" });
  assert.equal(exportResp2.t, "ExportJson", `ExportAll (post-import) -> ${JSON.stringify(exportResp2)}`);
  const exportJson2 = exportResp2.value;

  const normalized1 = normalizeExportForRoundTrip(exportJson1);
  const normalized2 = normalizeExportForRoundTrip(exportJson2);
  assert.deepStrictEqual(
    normalized2,
    normalized1,
    "re-exported bundle after import-into-empty does not match the original export modulo " +
      "exportedAt (+ the documented global-ruleset boot-reseed id/createdAt exception)",
  );
  log("phase4 OK: re-ExportAll deep-equals the original export (modulo exportedAt)");

  // ---- phase 5: cross-project graph edge survives restart (S4 spec §8 DoD) ----
  // Create two projects P1/P2, add one node to each, add a CROSS-PROJECT edge N1(P1)->N2(P2),
  // OrchdShutdown{drain:true} -> relaunch (mirrors phase1/phase2's create-then-restart shape, over
  // the graph verbs instead) -> GraphListProject(P1) must still return the edge, with its
  // endpoints intact, AND N2 must appear in `externalNodes` — proving the cross-project link
  // survives BOTH projects' restart, not merely same-project persistence (already proven by
  // phase2).
  log(
    "phase5: 2×CreateProject + 2×GraphAddNode + cross-project GraphAddEdge -> restart -> " +
      "GraphListProject",
  );

  const p1Resp = await orchdRequest(conn, {
    t: "CreateProject",
    name: "E2E Graph Project 1",
    description: "created by orchd-survive.mjs (phase5, cross-project graph)",
    workspaceIds: ["ws-e2e-orchd-graph-1"],
  });
  assert.equal(p1Resp.t, "Project", `CreateProject(P1) -> ${JSON.stringify(p1Resp)}`);
  const p1Id = p1Resp.value.id;

  const p2Resp = await orchdRequest(conn, {
    t: "CreateProject",
    name: "E2E Graph Project 2",
    description: "created by orchd-survive.mjs (phase5, cross-project graph)",
    workspaceIds: ["ws-e2e-orchd-graph-2"],
  });
  assert.equal(p2Resp.t, "Project", `CreateProject(P2) -> ${JSON.stringify(p2Resp)}`);
  const p2Id = p2Resp.value.id;

  const n1Resp = await orchdRequest(conn, {
    t: "GraphAddNode",
    projectId: p1Id,
    kind: "concept",
    label: "N1 (P1)",
    body: "node in project 1, e2e phase5",
    posX: 0,
    posY: 0,
  });
  assert.equal(n1Resp.t, "GraphNode", `GraphAddNode(P1) -> ${JSON.stringify(n1Resp)}`);
  const n1Id = n1Resp.value.id;
  assert.equal(n1Resp.value.projectId, p1Id, "N1 must belong to P1");

  const n2Resp = await orchdRequest(conn, {
    t: "GraphAddNode",
    projectId: p2Id,
    kind: "concept",
    label: "N2 (P2)",
    body: "node in project 2, e2e phase5",
    posX: 100,
    posY: 100,
  });
  assert.equal(n2Resp.t, "GraphNode", `GraphAddNode(P2) -> ${JSON.stringify(n2Resp)}`);
  const n2Id = n2Resp.value.id;
  assert.equal(n2Resp.value.projectId, p2Id, "N2 must belong to P2");

  const edgeResp = await orchdRequest(conn, {
    t: "GraphAddEdge",
    sourceNodeId: n1Id,
    targetNodeId: n2Id,
    kind: "relates",
    label: "cross-project link (e2e phase5)",
  });
  assert.equal(edgeResp.t, "GraphEdge", `GraphAddEdge(N1->N2) -> ${JSON.stringify(edgeResp)}`);
  const edgeId = edgeResp.value.id;
  assert.equal(edgeResp.value.sourceNodeId, n1Id, "edge sourceNodeId must be N1");
  assert.equal(edgeResp.value.targetNodeId, n2Id, "edge targetNodeId must be N2");

  log(
    `phase5: created P1 ${p1Id}, P2 ${p2Id}, N1 ${n1Id}, N2 ${n2Id}, cross-project edge ${edgeId}`,
  );

  await shutdownAndWaitExit(conn);
  log(`phase5 OK: orchd (pid ${cleanup.daemonPid}) process exited (pre-graph-restart)`);

  conn = await bootAndConnect();
  assert.equal(
    conn.chosenVersion,
    1,
    `post-graph-restart preamble handshake negotiated unexpected version: ${JSON.stringify(conn)}`,
  );

  const viewResp = await orchdRequest(conn, { t: "GraphListProject", projectId: p1Id });
  assert.equal(viewResp.t, "GraphView", `GraphListProject(P1) -> ${JSON.stringify(viewResp)}`);
  const view = viewResp.value;

  const rehydratedN1 = view.nodes.find((n) => n.id === n1Id);
  assert.ok(
    rehydratedN1,
    `N1 ${n1Id} lost from P1's own node list across orchd restart (nodes: ` +
      `${JSON.stringify(view.nodes.map((n) => n.id))})`,
  );

  const rehydratedEdge = view.edges.find((e) => e.id === edgeId);
  assert.ok(
    rehydratedEdge,
    `cross-project edge ${edgeId} lost from P1's GraphView across orchd restart (edges: ` +
      `${JSON.stringify(view.edges.map((e) => e.id))})`,
  );
  assert.equal(rehydratedEdge.sourceNodeId, n1Id, "rehydrated edge lost its sourceNodeId (N1)");
  assert.equal(rehydratedEdge.targetNodeId, n2Id, "rehydrated edge lost its targetNodeId (N2)");

  const rehydratedExternalN2 = view.externalNodes.find((n) => n.id === n2Id);
  assert.ok(
    rehydratedExternalN2,
    `N2 ${n2Id} (P2, the cross-project edge's foreign endpoint) did not surface in P1's ` +
      `externalNodes across orchd restart (externalNodes: ` +
      `${JSON.stringify(view.externalNodes.map((n) => n.id))})`,
  );
  assert.equal(
    rehydratedExternalN2.projectId,
    p2Id,
    "external ghost node must still report its OWN project id (P2)",
  );

  log("phase5 OK: cross-project graph edge survived restart");

  // ---- phase 6: MCP tool artifact survives restart (S-EXT Phase-1 DoD) ----
  // Spawn a local stub MCP server (Streamable HTTP) -> register it -> grant connect consent ->
  // McpConnect (tools cached) -> McpListTools (echo present) -> McpCallTool("echo") -> assert a
  // persisted artifact -> close the stub (proves the artifact does NOT depend on the MCP server
  // still being reachable) -> OrchdShutdown{drain:true} -> relaunch -> McpListArtifacts still
  // returns the artifact with its content intact (durable across restart, spec §9's DoD).
  log(
    "phase6: spawn stub MCP server + McpAddServer + TrustGrantConsent + McpConnect + " +
      "McpListTools + McpCallTool(echo)",
  );

  const stubMcpServer = await startStubMcpServer();
  cleanup.stubMcpServer = stubMcpServer;
  log(`phase6: stub MCP server listening at ${stubMcpServer.url}`);

  const addServerResp = await orchdRequest(conn, {
    t: "McpAddServer",
    name: "E2E Stub MCP",
    transport: "http",
    url: stubMcpServer.url,
    command: null,
    args: null,
    env: null,
    scope: "global",
    projectId: null,
    authKind: "none",
    timeoutMs: null,
    maxRetries: null,
  });
  assert.equal(addServerResp.t, "McpServer", `McpAddServer -> ${JSON.stringify(addServerResp)}`);
  const mcpServerId = addServerResp.value.id;
  assert.equal(
    addServerResp.value.url,
    stubMcpServer.url,
    "registered server must carry the stub's url verbatim",
  );
  assert.equal(addServerResp.value.transport, "http");
  assert.equal(addServerResp.value.scope, "global");
  assert.ok(addServerResp.value.enabled, "a freshly added server defaults to enabled");

  const consentResp = await orchdRequest(conn, {
    t: "TrustGrantConsent",
    serverId: mcpServerId,
    kind: "connect",
  });
  assert.equal(consentResp.t, "Ack", `TrustGrantConsent -> ${JSON.stringify(consentResp)}`);

  const connectResp = await orchdRequest(conn, { t: "McpConnect", id: mcpServerId });
  assert.equal(
    connectResp.t,
    "McpConnectReport",
    `McpConnect -> ${JSON.stringify(connectResp)}`,
  );
  assert.ok(
    connectResp.value.toolCount >= 1,
    `expected >=1 tool advertised by the stub, got ${JSON.stringify(connectResp.value)}`,
  );
  assert.ok(
    connectResp.value.protocolVersion && connectResp.value.protocolVersion.length > 0,
    "McpConnectReport must carry a negotiated protocol version",
  );

  const listToolsResp = await orchdRequest(conn, { t: "McpListTools", serverId: mcpServerId });
  assert.equal(listToolsResp.t, "McpTools", `McpListTools -> ${JSON.stringify(listToolsResp)}`);
  const echoTool = listToolsResp.value.find((tool) => tool.name === "echo");
  assert.ok(echoTool, `expected an "echo" tool in ${JSON.stringify(listToolsResp.value)}`);
  assert.ok(echoTool.enabled, "a freshly cached tool defaults to enabled");

  const callResp = await orchdRequest(conn, {
    t: "McpCallTool",
    serverId: mcpServerId,
    toolName: "echo",
    argsJson: JSON.stringify({ msg: "restart-survivor" }),
    projectId: null,
  });
  assert.equal(callResp.t, "McpCallResult", `McpCallTool -> ${JSON.stringify(callResp)}`);
  assert.equal(
    callResp.value.isError,
    false,
    `echo call must not be a tool-level error: ${JSON.stringify(callResp.value)}`,
  );
  assert.ok(callResp.value.artifactId, "expected a persisted artifact id");
  assert.ok(
    callResp.value.contentJson.includes("restart-survivor"),
    `expected the echoed message in the call result content: ${callResp.value.contentJson}`,
  );
  const mcpArtifactId = callResp.value.artifactId;

  log(
    `phase6: registered server ${mcpServerId}, connected (protocol ` +
      `${connectResp.value.protocolVersion}), called echo -> artifact ${mcpArtifactId}`,
  );

  // Close the stub NOW, before the restart — `McpListArtifacts` below reads straight from
  // `orchd.db`, never re-touching the MCP server, so the artifact's durability must not depend on
  // the stub still being reachable after this point.
  await stubMcpServer.close();
  cleanup.stubMcpServer = null;
  log("phase6: stub MCP server closed (artifact durability must not depend on it)");

  await shutdownAndWaitExit(conn);
  log(`phase6 OK: orchd (pid ${cleanup.daemonPid}) process exited (pre-mcp-restart)`);

  conn = await bootAndConnect();
  assert.equal(
    conn.chosenVersion,
    1,
    `post-mcp-restart preamble handshake negotiated unexpected version: ${JSON.stringify(conn)}`,
  );

  const artifactsResp = await orchdRequest(conn, {
    t: "McpListArtifacts",
    projectId: null,
    serverId: mcpServerId,
    limit: null,
  });
  assert.equal(
    artifactsResp.t,
    "McpArtifacts",
    `McpListArtifacts -> ${JSON.stringify(artifactsResp)}`,
  );
  const rehydratedArtifact = artifactsResp.value.find((a) => a.id === mcpArtifactId);
  assert.ok(
    rehydratedArtifact,
    `artifact ${mcpArtifactId} lost across orchd restart (artifacts: ` +
      `${JSON.stringify(artifactsResp.value.map((a) => a.id))})`,
  );
  assert.ok(
    rehydratedArtifact.contentJson.includes("restart-survivor"),
    `rehydrated artifact lost its content across orchd restart: ${rehydratedArtifact.contentJson}`,
  );
  assert.equal(
    rehydratedArtifact.serverId,
    mcpServerId,
    "rehydrated artifact must still reference its source server",
  );
  assert.ok(rehydratedArtifact.isUntrusted, "spec D9: every mcp_artifact is is_untrusted=1");

  log("phase6 OK: mcp tool artifact survived restart");

  // ---- phase 7: CONNECTOR invoke artifact survives restart (S-EXT Phase-2 DoD, spec §9) —
  // extracted into `connectorInvokePhase` (see its doc comment) so its keychain-availability probe
  // can RETURN early with a LOUD graceful skip on a headless CI runner whose login keychain is
  // locked, WITHOUT failing the run — while still running the full survival assertions whenever the
  // keychain is writable (every local dev run, and any CI runner with an unlocked keychain). ----
  conn = await connectorInvokePhase(conn);

  // ---- phase 8: idea -> research -> insight -> task survives restart (S-IDEA DoD, spec §8) ----
  conn = await researchInsightTaskSurvivalPhase(conn);

  // ---- phase 9: interrupted research run boot-reconcile (S-IDEA spec D11) ----
  conn = await researchBootReconcilePhase(conn);

  // BL-106: phase7 (connector/keychain survival) can SKIP gracefully on a runner whose login
  // keychain is unavailable. CI provisions an unlocked search-list keychain (ci.yml "unlock
  // keychain" step, same job), so phase7 MUST run there — a SKIP in CI means keychain provisioning
  // regressed and must fail the gate instead of passing vacuously. Locally (no
  // BPA_REQUIRE_KEYCHAIN) a SKIP stays an allowed warning.
  if (!phase7Ran) {
    if (process.env.BPA_REQUIRE_KEYCHAIN === "1") {
      throw new Error(
        "phase7 (connector/keychain survival) SKIPPED but BPA_REQUIRE_KEYCHAIN=1 is set — a " +
          "keychain-provisioning regression would otherwise pass vacuously (BL-106)",
      );
    }
    log(
      "WARNING: phase7 (connector/keychain survival) SKIPPED — login keychain unavailable in " +
        "this environment. Allowed locally; CI sets BPA_REQUIRE_KEYCHAIN=1 to fail on a real " +
        "provisioning regression.",
    );
  }

  log("ALL PHASES PASSED");
}

/**
 * Poll `ResearchGetRun{id}` until `predicate(run)` is true, or throw after `tries` attempts
 * (`delayMs` apart) — bounded, no wall-clock-fragile assumption about exactly how fast the async
 * run driver transitions (S4 lesson, spec §8: "no env-fragile timing asserts — the run test uses a
 * fake seam, not wall-clock"; this e2e drives the REAL async driver over a real loopback HTTP stub,
 * so a bounded poll — not a fixed sleep — is this harness's own equivalent). `what` is a short
 * human label used only in the failure message.
 */
async function pollResearchRunUntil(conn, runId, predicate, what, tries = 100, delayMs = 100) {
  let lastRun = null;
  for (let i = 0; i < tries; i++) {
    const resp = await orchdRequest(conn, { t: "ResearchGetRun", id: runId });
    assert.equal(resp.t, "ResearchRun", `ResearchGetRun -> ${JSON.stringify(resp)}`);
    lastRun = resp.value;
    if (predicate(lastRun)) return lastRun;
    await sleep(delayMs);
  }
  assert.fail(
    `research run ${runId} never reached the expected state (${what}) within ${tries * delayMs}ms; ` +
      `last observed: ${JSON.stringify(lastRun)}`,
  );
}

/**
 * Phase 7: a CONNECTOR (`generic-rest`) invocation result persists as a durable artifact that
 * survives an orchd restart (S-EXT Phase-2 DoD, spec §9 — the connector-path analogue of phase6's
 * MCP path: spec §6 "ConnectorInvoke passes through trust::authorize IDENTICALLY to McpCallTool ...
 * every mcp_artifact from McpCallTool AND ConnectorInvoke is is_untrusted=1").
 *
 * Extracted from `main` (rather than inlined like phases 0–6) for ONE reason: it is the only phase
 * that drives `ConnectorAddApiKey`, which writes a REAL macOS login-Keychain entry through the
 * daemon (`bpa_secrets` -> `security_framework::passwords::*`). On a headless CI runner (this phase
 * runs in CI via `final-suite.sh` stage 9) the login keychain can be locked/unavailable, in which
 * case the daemon returns a keychain-shaped `Error` (NOT an `Account`) rather than hanging (the
 * `Library/Keychains` symlink set up above prevents the hang, not the error). So this phase begins
 * with a keychain-availability PROBE that is a FULL round-trip: `ConnectorAddApiKey` (a Keychain
 * WRITE) -> `ConnectorInvoke` (a Keychain READ via `bpa_secrets::get`) -> `ConnectorDeleteAccount`
 * (a Keychain DELETE). A set-only probe would NOT catch a keychain that is writable-but-not-on-
 * the-search-list (WRITE goes to the default keychain, READ/DELETE resolve the search list — they
 * are independent in Keychain Services), so the read leg is essential. If ANY leg fails, this
 * phase LOUDLY skips (a visible `SKIP` log line, never a silent vacuous pass) and RETURNs early,
 * mirroring the Rust integration suite's own hardened `connector_keychain_available()`
 * probe-and-skip (`crates/orchd/tests/dispatch_integration.rs`). When the keychain is fully usable
 * the FULL phase runs and its survival assertions are NOT weakened.
 *
 * Steps (keychain available): spawn a local stub generic-rest target -> `ConnectorAddApiKey`
 * (real Keychain entry) -> capture the account id -> `ConnectorInvoke(op="post")` against the stub
 * with a planted marker in the body (args shape verified against `crates/orchd/src/connectors/
 * adapter.rs::GenericRestAdapter::invoke`: `post` reads `args["url"]` + JSON-encodes `args["body"]`)
 * -> assert `McpCallResult{isError:false}` + a persisted artifact whose content carries the stub's
 * OWN echoed marker (proving the adapter's real HTTP round trip, not a canned local value) -> close
 * the stub (mirrors phase6: artifact durability must not depend on the connector target still being
 * reachable) -> `OrchdShutdown{drain:true}` -> relaunch -> `McpListArtifacts` still returns the
 * artifact with its content/account/untrusted flags intact -> `ConnectorDeleteAccount` (cleans up
 * the DB row AND the real Keychain entry, `connectors::accounts::Db::delete_account`).
 *
 * Returns the (possibly relaunched) connection; `main` does not use it afterward, but returning it
 * keeps the reassignment contract honest for a future caller.
 */
async function connectorInvokePhase(conn) {
  log(
    "phase7: keychain probe + spawn stub generic-rest server + ConnectorAddApiKey(generic-rest) " +
      "+ ConnectorInvoke(post)",
  );

  // The stub REST server starts BEFORE the probe so the probe can exercise a FULL keychain
  // round-trip (see below), not just a write.
  const stubRestServer = await startStubRestServer();
  cleanup.stubRestServer = stubRestServer;
  log(`phase7: stub generic-rest server listening at ${stubRestServer.url}`);

  // ---- keychain-availability probe (headless-CI guard, mirrors the Rust suite's hardened
  // `connector_keychain_available()`): a FULL round-trip — `ConnectorAddApiKey` (a Keychain
  // WRITE) THEN `ConnectorInvoke` (a Keychain READ via `accounts::token_for` ->
  // `bpa_secrets::get`) THEN `ConnectorDeleteAccount` (a Keychain DELETE). A set-only probe is
  // NOT enough: Keychain Services treats the "default keychain" and the "search list" as
  // independent, so a CI keychain that was created + set-default + unlocked but NOT added to the
  // search list makes the WRITE succeed while the READ/DELETE fail "not found". A set-only probe
  // would report "available" and the real phase's own `ConnectorInvoke` read-back would then FAIL
  // the gate. `ConnectorListAccounts` does NOT help — it reads DB rows, never the Keychain — so
  // the read MUST go through `ConnectorInvoke` (the only wire verb that resolves the stored
  // secret). Any failure at any step (write, read, or delete) => LOUD graceful skip + early
  // return, so the gate stays green WITHOUT ever masking a real failure when the keychain IS
  // fully usable. ----
  const probeApiKey = `e2e-keychain-probe-${Date.now()}-${Math.random().toString(36).slice(2)}`;
  let probeResp;
  try {
    probeResp = await orchdRequest(conn, {
      t: "ConnectorAddApiKey",
      provider: "generic-rest",
      label: "E2E Keychain Probe",
      apiKey: probeApiKey,
    });
  } catch (e) {
    log(
      "SKIP phase7: login keychain unavailable in this environment (headless CI) — graceful " +
        `skip, not a pass (probe write request errored: ${e.message})`,
    );
    await stubRestServer.close();
    cleanup.stubRestServer = null;
    return conn;
  }
  if (probeResp.t !== "Account") {
    log(
      "SKIP phase7: login keychain unavailable in this environment (headless CI) — graceful " +
        `skip, not a pass (probe write -> ${JSON.stringify(probeResp)})`,
    );
    await stubRestServer.close();
    cleanup.stubRestServer = null;
    return conn;
  }
  const probeAccountId = probeResp.value.id;
  cleanup.connectorProbeAccountId = probeAccountId;
  // Read-back leg: `ConnectorInvoke` resolves the just-written secret via `bpa_secrets::get`. On a
  // broken search list the WRITE above succeeded but this READ fails (the daemon returns an
  // `Error`, not a `McpCallResult`) — exactly the case a set-only probe would miss.
  let probeInvokeResp;
  try {
    probeInvokeResp = await orchdRequest(conn, {
      t: "ConnectorInvoke",
      accountId: probeAccountId,
      op: "get",
      argsJson: JSON.stringify({ url: stubRestServer.url }),
      projectId: null,
    });
  } catch (e) {
    log(
      "SKIP phase7: keychain WRITE succeeded but READ-BACK errored (keychain likely not on the " +
        `search list) — graceful skip, not a pass (probe read request errored: ${e.message})`,
    );
    await orchdRequest(conn, { t: "ConnectorDeleteAccount", id: probeAccountId }).catch(() => {});
    cleanup.connectorProbeAccountId = null;
    await stubRestServer.close();
    cleanup.stubRestServer = null;
    return conn;
  }
  if (probeInvokeResp.t !== "McpCallResult") {
    log(
      "SKIP phase7: keychain WRITE succeeded but READ-BACK failed (keychain likely not on the " +
        `search list) — graceful skip, not a pass (probe read -> ${JSON.stringify(probeInvokeResp)})`,
    );
    await orchdRequest(conn, { t: "ConnectorDeleteAccount", id: probeAccountId }).catch(() => {});
    cleanup.connectorProbeAccountId = null;
    await stubRestServer.close();
    cleanup.stubRestServer = null;
    return conn;
  }
  // Full round-trip proven writable + readable. Delete the throwaway probe account (a REAL Keychain
  // DELETE — the third leg) before the real phase begins.
  const probeDeleteResp = await orchdRequest(conn, {
    t: "ConnectorDeleteAccount",
    id: probeAccountId,
  });
  assert.equal(
    probeDeleteResp.t,
    "Ack",
    `probe ConnectorDeleteAccount -> ${JSON.stringify(probeDeleteResp)}`,
  );
  cleanup.connectorProbeAccountId = null;
  log(
    "phase7: keychain probe OK (login keychain write + read-back + delete all succeeded) — " +
      "running the full connector phase",
  );

  const apiKeyValue = `e2e-key-${Date.now()}-${Math.random().toString(36).slice(2)}`;
  const addAccountResp = await orchdRequest(conn, {
    t: "ConnectorAddApiKey",
    provider: "generic-rest",
    label: "E2E Connector",
    apiKey: apiKeyValue,
  });
  assert.equal(
    addAccountResp.t,
    "Account",
    `ConnectorAddApiKey -> ${JSON.stringify(addAccountResp)}`,
  );
  const accountId = addAccountResp.value.id;
  // Tracked so `cleanupAll()` can best-effort delete the REAL Keychain entry this created even if
  // a later assertion in this phase throws before the happy-path `ConnectorDeleteAccount` below
  // runs — cleared once that explicit deletion succeeds.
  cleanup.connectorAccountId = accountId;
  assert.equal(addAccountResp.value.provider, "generic-rest");
  assert.equal(addAccountResp.value.authKind, "apikey");
  assert.equal(addAccountResp.value.label, "E2E Connector");

  const invokeMarker = "connector-restart-survivor";
  const invokeResp = await orchdRequest(conn, {
    t: "ConnectorInvoke",
    accountId,
    op: "post",
    argsJson: JSON.stringify({ url: stubRestServer.url, body: { marker: invokeMarker } }),
    projectId: null,
  });
  assert.equal(invokeResp.t, "McpCallResult", `ConnectorInvoke -> ${JSON.stringify(invokeResp)}`);
  assert.equal(
    invokeResp.value.isError,
    false,
    `connector post call must not be a call-level error: ${JSON.stringify(invokeResp.value)}`,
  );
  assert.ok(invokeResp.value.artifactId, "expected a persisted artifact id");
  assert.ok(
    invokeResp.value.contentJson.includes(invokeMarker),
    `expected the stub's echoed marker in the call result content: ${invokeResp.value.contentJson}`,
  );
  const connectorArtifactId = invokeResp.value.artifactId;

  // `assert.ok` on a PRE-COMPUTED boolean (deliberately NOT `assert.equal(actual, expected)`): a
  // failing `assert.equal` renders BOTH operands into the AssertionError message, which would
  // print the live api key. Comparing here and passing only a key-free message keeps the secret
  // out of any failure output (the bearer is still genuinely asserted to have reached the wire).
  assert.ok(
    stubRestServer.lastAuthHeader() === `Bearer ${apiKeyValue}`,
    "GenericRestAdapter must send the account's api key as Authorization: Bearer <key> (value withheld)",
  );

  log(`phase7: added account ${accountId}, invoked post -> artifact ${connectorArtifactId}`);

  // Close the stub NOW, before the restart — same rationale as phase6: the artifact's durability
  // must not depend on the connector target still being reachable after this point.
  await stubRestServer.close();
  cleanup.stubRestServer = null;
  log("phase7: stub generic-rest server closed (artifact durability must not depend on it)");

  await shutdownAndWaitExit(conn);
  log(`phase7 OK: orchd (pid ${cleanup.daemonPid}) process exited (pre-connector-restart)`);

  conn = await bootAndConnect();
  assert.equal(
    conn.chosenVersion,
    1,
    `post-connector-restart preamble handshake negotiated unexpected version: ${JSON.stringify(conn)}`,
  );

  const artifactsAfterConnectorRestart = await orchdRequest(conn, {
    t: "McpListArtifacts",
    // No `serverId` filter: a `ConnectorInvoke` artifact has `server_id: null` (spec §4
    // account_id/server_id XOR) — filtering by the phase6 MCP server id would never match it.
    projectId: null,
    serverId: null,
    limit: null,
  });
  assert.equal(
    artifactsAfterConnectorRestart.t,
    "McpArtifacts",
    `McpListArtifacts -> ${JSON.stringify(artifactsAfterConnectorRestart)}`,
  );
  const rehydratedConnectorArtifact = artifactsAfterConnectorRestart.value.find(
    (a) => a.id === connectorArtifactId,
  );
  assert.ok(
    rehydratedConnectorArtifact,
    `connector artifact ${connectorArtifactId} lost across orchd restart (artifacts: ` +
      `${JSON.stringify(artifactsAfterConnectorRestart.value.map((a) => a.id))})`,
  );
  assert.ok(
    rehydratedConnectorArtifact.contentJson.includes(invokeMarker),
    `rehydrated connector artifact lost its content across orchd restart: ` +
      `${rehydratedConnectorArtifact.contentJson}`,
  );
  assert.equal(
    rehydratedConnectorArtifact.accountId,
    accountId,
    "rehydrated connector artifact must still reference its source account",
  );
  assert.equal(
    rehydratedConnectorArtifact.serverId,
    null,
    "a ConnectorInvoke artifact has NO server_id (account_id/server_id XOR, spec §4)",
  );
  assert.ok(
    rehydratedConnectorArtifact.isUntrusted,
    "spec §6/D9: every mcp_artifact from ConnectorInvoke is is_untrusted=1",
  );

  log("phase7: connector invoke artifact survived restart — cleaning up the connector account");

  const deleteAccountResp = await orchdRequest(conn, {
    t: "ConnectorDeleteAccount",
    id: accountId,
  });
  assert.equal(
    deleteAccountResp.t,
    "Ack",
    `ConnectorDeleteAccount -> ${JSON.stringify(deleteAccountResp)}`,
  );
  cleanup.connectorAccountId = null;

  const accountsAfterDelete = await orchdRequest(conn, { t: "ConnectorListAccounts" });
  assert.equal(
    accountsAfterDelete.t,
    "Accounts",
    `ConnectorListAccounts -> ${JSON.stringify(accountsAfterDelete)}`,
  );
  assert.ok(
    !accountsAfterDelete.value.some((a) => a.id === accountId),
    `ConnectorDeleteAccount must remove the account (still present: ` +
      `${JSON.stringify(accountsAfterDelete.value)})`,
  );

  log("phase7 OK: connector invoke artifact survived restart");
  phase7Ran = true;
  return conn;
}

/**
 * Phase 8: idea -> research -> insight -> task survives restart — the S-IDEA slice's own DoD
 * (spec §8's "the DoD" e2e bullet). Mirrors the REAL owner-driven flow byte-for-byte, verified
 * against `src/components/idea/FormInsightDialog.tsx`'s `handleCreate`/`handleAccept`/
 * `handleBacklog` (S-IDEA task T6): `CreateInsight` with `source: "research-run:<runId>"` (THAT
 * exact convention — `FormInsightDialog.handleCreate`), then `SetInsightFitVerdict`, then (once
 * accepted) `CreateTask{source:"insight", sourceId:<insightId>}` followed by the dialog's own
 * explicit `SetIdeaLifecycle{specced}` call (`handleBacklog`) — lifecycle `specced` is NOT an
 * automatic side effect of any other verb here, it is its own request.
 *
 * Sequence: CreateProject + CreateIdea -> register+connect a stub MCP server exposing a
 * `research` tool (canned findings, see `stub-mcp-server.mjs`) -> `ResearchStartRun` -> poll
 * `ResearchGetRun` until `done` -> assert the run's `artifactId` (the durable `mcp_artifact`, spec
 * D2's provenance link) -> close the stub (mirrors phase6/phase7: artifact durability must not
 * depend on the MCP server still being reachable) -> `CreateInsight` + `SetInsightFitVerdict{fit}`
 * -> `SetInsightStatus{accepted}` -> `CreateTask{source:insight}` -> `SetIdeaLifecycle{specced}`
 * -> `OrchdShutdown{drain:true}` -> relaunch -> re-fetch EVERY entity by id and assert it
 * survived: the idea's `specced` lifecycle, the run's `done` status + its artifact (content
 * intact, re-verified via `McpListArtifacts`), the insight's fit-verdict + `accepted` status, and
 * the task's `source`/`sourceId` link back to the insight.
 *
 * Returns the (possibly relaunched) connection.
 */
async function researchInsightTaskSurvivalPhase(conn) {
  log("phase8: CreateProject + CreateIdea + register/connect stub MCP research server");

  const projectResp = await orchdRequest(conn, {
    t: "CreateProject",
    name: "E2E Research Project",
    description: "created by orchd-survive.mjs (phase8, idea->research->insight->task survival)",
    workspaceIds: ["ws-e2e-orchd-research-1"],
  });
  assert.equal(projectResp.t, "Project", `CreateProject -> ${JSON.stringify(projectResp)}`);
  const researchProjectId = projectResp.value.id;

  const ideaResp = await orchdRequest(conn, {
    t: "CreateIdea",
    projectId: researchProjectId,
    title: "Idea 8 (e2e research survival)",
    body: "an idea driven through the full research->insight->task pipeline",
  });
  assert.equal(ideaResp.t, "Idea", `CreateIdea -> ${JSON.stringify(ideaResp)}`);
  const ideaId = ideaResp.value.id;
  assert.equal(ideaResp.value.lifecycle, "captured", "a freshly created idea must start captured");

  const stubMcpServer = await startStubMcpServer();
  cleanup.stubMcpServer = stubMcpServer;
  log(`phase8: stub MCP research server listening at ${stubMcpServer.url}`);

  const addServerResp = await orchdRequest(conn, {
    t: "McpAddServer",
    name: "E2E Research Stub MCP",
    transport: "http",
    url: stubMcpServer.url,
    command: null,
    args: null,
    env: null,
    scope: "global",
    projectId: null,
    authKind: "none",
    timeoutMs: null,
    maxRetries: null,
  });
  assert.equal(addServerResp.t, "McpServer", `McpAddServer -> ${JSON.stringify(addServerResp)}`);
  const researchServerId = addServerResp.value.id;

  const consentResp = await orchdRequest(conn, {
    t: "TrustGrantConsent",
    serverId: researchServerId,
    kind: "connect",
  });
  assert.equal(consentResp.t, "Ack", `TrustGrantConsent -> ${JSON.stringify(consentResp)}`);

  const connectResp = await orchdRequest(conn, { t: "McpConnect", id: researchServerId });
  assert.equal(connectResp.t, "McpConnectReport", `McpConnect -> ${JSON.stringify(connectResp)}`);
  assert.ok(
    connectResp.value.toolCount >= 1,
    `expected >=1 tool advertised by the stub, got ${JSON.stringify(connectResp.value)}`,
  );

  const listToolsResp = await orchdRequest(conn, { t: "McpListTools", serverId: researchServerId });
  assert.equal(listToolsResp.t, "McpTools", `McpListTools -> ${JSON.stringify(listToolsResp)}`);
  const researchTool = listToolsResp.value.find((tool) => tool.name === RESEARCH_TOOL_NAME);
  assert.ok(
    researchTool,
    `expected a "${RESEARCH_TOOL_NAME}" tool in ${JSON.stringify(listToolsResp.value)}`,
  );

  log("phase8: ResearchStartRun -> poll ResearchGetRun until done");

  const startRunResp = await orchdRequest(conn, {
    t: "ResearchStartRun",
    ideaId,
    serverId: researchServerId,
    toolName: RESEARCH_TOOL_NAME,
    argsJson: JSON.stringify({ query: "e2e research query" }),
  });
  assert.equal(startRunResp.t, "ResearchRun", `ResearchStartRun -> ${JSON.stringify(startRunResp)}`);
  const runId = startRunResp.value.id;
  assert.equal(startRunResp.value.ideaId, ideaId, "run must reference the idea it was started for");
  assert.equal(
    startRunResp.value.status,
    "pending",
    `a freshly started run must be pending, got ${JSON.stringify(startRunResp.value)}`,
  );

  const doneRun = await pollResearchRunUntil(conn, runId, (run) => run.status === "done", "status=done");
  assert.ok(doneRun.artifactId, `a done run must carry an artifactId: ${JSON.stringify(doneRun)}`);
  assert.ok(doneRun.invocationId, `a done run must carry an invocationId: ${JSON.stringify(doneRun)}`);
  const runArtifactId = doneRun.artifactId;

  const artifactsResp = await orchdRequest(conn, {
    t: "McpListArtifacts",
    projectId: null,
    serverId: researchServerId,
    limit: null,
  });
  assert.equal(artifactsResp.t, "McpArtifacts", `McpListArtifacts -> ${JSON.stringify(artifactsResp)}`);
  const runArtifact = artifactsResp.value.find((a) => a.id === runArtifactId);
  assert.ok(
    runArtifact,
    `expected the run's artifact ${runArtifactId} in ${JSON.stringify(artifactsResp.value.map((a) => a.id))}`,
  );
  assert.ok(
    runArtifact.contentJson.includes(RESEARCH_FINDINGS_MARKER),
    `expected the canned findings marker in the artifact content: ${runArtifact.contentJson}`,
  );

  log(`phase8: research run ${runId} done, artifact ${runArtifactId}`);

  // Close the stub NOW, before insight/task creation and the restart — mirrors phase6/phase7:
  // artifact durability must not depend on the MCP server still being reachable.
  await stubMcpServer.close();
  cleanup.stubMcpServer = null;
  log("phase8: stub MCP research server closed (artifact durability must not depend on it)");

  log("phase8: CreateInsight + SetInsightFitVerdict{fit} + SetInsightStatus{accepted}");

  const createInsightResp = await orchdRequest(conn, {
    t: "CreateInsight",
    projectId: researchProjectId,
    source: `research-run:${runId}`,
    title: "Insight 8 (e2e)",
    body: "an insight formed from the research findings",
  });
  assert.equal(createInsightResp.t, "Insight", `CreateInsight -> ${JSON.stringify(createInsightResp)}`);
  const insightId = createInsightResp.value.id;
  assert.equal(createInsightResp.value.source, `research-run:${runId}`, "insight must carry the research-run source");
  assert.equal(createInsightResp.value.status, "new", "a freshly created insight must start new");

  const fitVerdictResp = await orchdRequest(conn, {
    t: "SetInsightFitVerdict",
    id: insightId,
    fitVerdict: "fit",
    fitReasoning: "aligns with the project's strategic goal (e2e)",
  });
  assert.equal(fitVerdictResp.t, "Insight", `SetInsightFitVerdict -> ${JSON.stringify(fitVerdictResp)}`);
  assert.equal(fitVerdictResp.value.fitVerdict, "fit", "SetInsightFitVerdict must persist the fit verdict");

  const acceptResp = await orchdRequest(conn, {
    t: "SetInsightStatus",
    id: insightId,
    status: "accepted",
    resolutionReasoning: null,
  });
  assert.equal(acceptResp.t, "Insight", `SetInsightStatus -> ${JSON.stringify(acceptResp)}`);
  assert.equal(acceptResp.value.status, "accepted", "SetInsightStatus must persist accepted");

  log("phase8: CreateTask{source:insight} + SetIdeaLifecycle{specced}");

  const createTaskResp = await orchdRequest(conn, {
    t: "CreateTask",
    projectId: researchProjectId,
    parentId: null,
    title: acceptResp.value.title,
    body: acceptResp.value.body,
    status: null,
    source: "insight",
    sourceId: insightId,
    tags: [],
  });
  assert.equal(createTaskResp.t, "Task", `CreateTask -> ${JSON.stringify(createTaskResp)}`);
  const taskId = createTaskResp.value.id;
  assert.equal(createTaskResp.value.source, "insight", "task must carry source=insight");
  assert.equal(createTaskResp.value.sourceId, insightId, "task must link sourceId back to the insight");

  const lifecycleResp = await orchdRequest(conn, {
    t: "SetIdeaLifecycle",
    id: ideaId,
    lifecycle: "specced",
  });
  assert.equal(lifecycleResp.t, "Idea", `SetIdeaLifecycle -> ${JSON.stringify(lifecycleResp)}`);
  assert.equal(lifecycleResp.value.lifecycle, "specced", "SetIdeaLifecycle must persist specced");

  log(
    `phase8: idea ${ideaId} -> run ${runId} -> insight ${insightId} -> task ${taskId} all ` +
      "created — restarting to prove survival",
  );

  await shutdownAndWaitExit(conn);
  log(`phase8 OK: orchd (pid ${cleanup.daemonPid}) process exited (pre-research-restart)`);

  conn = await bootAndConnect();
  assert.equal(
    conn.chosenVersion,
    1,
    `post-research-restart preamble handshake negotiated unexpected version: ${JSON.stringify(conn)}`,
  );

  const ideasAfterRestart = await orchdRequest(conn, { t: "ListIdeas", projectId: researchProjectId });
  assert.equal(ideasAfterRestart.t, "Ideas", `ListIdeas -> ${JSON.stringify(ideasAfterRestart)}`);
  const rehydratedIdea = ideasAfterRestart.value.find((i) => i.id === ideaId);
  assert.ok(
    rehydratedIdea,
    `idea ${ideaId} lost across orchd restart (ideas: ${JSON.stringify(ideasAfterRestart.value.map((i) => i.id))})`,
  );
  assert.equal(rehydratedIdea.lifecycle, "specced", "rehydrated idea lost its specced lifecycle");

  const runAfterRestart = await orchdRequest(conn, { t: "ResearchGetRun", id: runId });
  assert.equal(
    runAfterRestart.t,
    "ResearchRun",
    `ResearchGetRun (post-restart) -> ${JSON.stringify(runAfterRestart)}`,
  );
  assert.equal(runAfterRestart.value.status, "done", "rehydrated research run lost its done status");
  assert.equal(
    runAfterRestart.value.artifactId,
    runArtifactId,
    "rehydrated research run lost its artifactId",
  );

  const artifactsAfterRestart = await orchdRequest(conn, {
    t: "McpListArtifacts",
    projectId: null,
    serverId: researchServerId,
    limit: null,
  });
  assert.equal(
    artifactsAfterRestart.t,
    "McpArtifacts",
    `McpListArtifacts (post-restart) -> ${JSON.stringify(artifactsAfterRestart)}`,
  );
  const rehydratedRunArtifact = artifactsAfterRestart.value.find((a) => a.id === runArtifactId);
  assert.ok(rehydratedRunArtifact, `run artifact ${runArtifactId} lost across orchd restart`);
  assert.ok(
    rehydratedRunArtifact.contentJson.includes(RESEARCH_FINDINGS_MARKER),
    `rehydrated run artifact lost its content across orchd restart: ${rehydratedRunArtifact.contentJson}`,
  );

  const insightsAfterRestart = await orchdRequest(conn, {
    t: "ListInsights",
    projectId: researchProjectId,
  });
  assert.equal(insightsAfterRestart.t, "Insights", `ListInsights -> ${JSON.stringify(insightsAfterRestart)}`);
  const rehydratedInsight = insightsAfterRestart.value.find((i) => i.id === insightId);
  assert.ok(rehydratedInsight, `insight ${insightId} lost across orchd restart`);
  assert.equal(rehydratedInsight.fitVerdict, "fit", "rehydrated insight lost its fit verdict");
  assert.equal(rehydratedInsight.status, "accepted", "rehydrated insight lost its accepted status");

  const tasksAfterRestart = await orchdRequest(conn, { t: "ListTasks", projectId: researchProjectId });
  assert.equal(tasksAfterRestart.t, "Tasks", `ListTasks -> ${JSON.stringify(tasksAfterRestart)}`);
  const rehydratedTask = tasksAfterRestart.value.find((t) => t.id === taskId);
  assert.ok(rehydratedTask, `task ${taskId} lost across orchd restart`);
  assert.equal(rehydratedTask.source, "insight", "rehydrated task lost its insight source");
  assert.equal(
    rehydratedTask.sourceId,
    insightId,
    "rehydrated task lost its sourceId link to the insight",
  );

  log("phase8 OK: idea→research→insight→task survives restart");
  return conn;
}

/**
 * Phase 9: an interrupted research run is reconciled to `failed{interrupted}` on the NEXT boot
 * (S-IDEA spec D11 — the boot-reconcile safety net for the async run driver's detached,
 * drain-untracked `tokio::spawn` task, `research::run_research`). This phase deliberately
 * exercises the in-flight-at-restart race phase8 avoids (phase8's run always reaches `done`
 * BEFORE its restart): register+connect a stub MCP server whose `research` tool's `tools/call`
 * NEVER responds (`startStubMcpServer({ blockResearchTool: true })`, see that module's own doc
 * comment) -> `ResearchStartRun` -> poll `ResearchGetRun` until `running` (NOT `done` — the tool
 * call never completes, so the run can never reach `done` on its own) ->
 * `OrchdShutdown{drain:true}` (spec D11: the drain does NOT track this orphaned task — the ack
 * comes back promptly regardless of the still-blocked network call) -> relaunch -> boot's
 * `reconcile_interrupted_research_runs` (called unconditionally, before any client connects —
 * mirrors `ensure_global_ruleset`'s "ensured at every boot" placement) must have flipped the
 * still-`running` row to `failed{interrupted}` — asserted below, not a lingering `running`.
 *
 * Returns the (possibly relaunched) connection.
 */
async function researchBootReconcilePhase(conn) {
  log("phase9: CreateProject + CreateIdea + register/connect BLOCKING stub MCP research server");

  const projectResp = await orchdRequest(conn, {
    t: "CreateProject",
    name: "E2E Boot-Reconcile Project",
    description: "created by orchd-survive.mjs (phase9, S-IDEA D11 boot-reconcile)",
    workspaceIds: ["ws-e2e-orchd-reconcile-1"],
  });
  assert.equal(projectResp.t, "Project", `CreateProject -> ${JSON.stringify(projectResp)}`);
  const reconcileProjectId = projectResp.value.id;

  const ideaResp = await orchdRequest(conn, {
    t: "CreateIdea",
    projectId: reconcileProjectId,
    title: "Idea 9 (e2e boot-reconcile)",
    body: "an idea whose research run gets interrupted mid-flight",
  });
  assert.equal(ideaResp.t, "Idea", `CreateIdea -> ${JSON.stringify(ideaResp)}`);
  const ideaId = ideaResp.value.id;

  const blockingStub = await startStubMcpServer({ blockResearchTool: true });
  cleanup.stubMcpServer = blockingStub;
  log(`phase9: BLOCKING stub MCP research server listening at ${blockingStub.url}`);

  const addServerResp = await orchdRequest(conn, {
    t: "McpAddServer",
    name: "E2E Boot-Reconcile Stub MCP",
    transport: "http",
    url: blockingStub.url,
    command: null,
    args: null,
    env: null,
    scope: "global",
    projectId: null,
    authKind: "none",
    timeoutMs: null,
    maxRetries: null,
  });
  assert.equal(addServerResp.t, "McpServer", `McpAddServer -> ${JSON.stringify(addServerResp)}`);
  const blockingServerId = addServerResp.value.id;

  const consentResp = await orchdRequest(conn, {
    t: "TrustGrantConsent",
    serverId: blockingServerId,
    kind: "connect",
  });
  assert.equal(consentResp.t, "Ack", `TrustGrantConsent -> ${JSON.stringify(consentResp)}`);

  const connectResp = await orchdRequest(conn, { t: "McpConnect", id: blockingServerId });
  assert.equal(connectResp.t, "McpConnectReport", `McpConnect -> ${JSON.stringify(connectResp)}`);

  log("phase9: ResearchStartRun -> poll ResearchGetRun until running (the tool call blocks)");

  const startRunResp = await orchdRequest(conn, {
    t: "ResearchStartRun",
    ideaId,
    serverId: blockingServerId,
    toolName: RESEARCH_TOOL_NAME,
    argsJson: JSON.stringify({ query: "e2e boot-reconcile query" }),
  });
  assert.equal(startRunResp.t, "ResearchRun", `ResearchStartRun -> ${JSON.stringify(startRunResp)}`);
  const runId = startRunResp.value.id;

  const runningRun = await pollResearchRunUntil(
    conn,
    runId,
    (run) => run.status === "running",
    "status=running",
  );
  assert.equal(
    runningRun.status,
    "running",
    `expected the run to be blocked in running, got ${JSON.stringify(runningRun)}`,
  );
  assert.equal(runningRun.artifactId, null, "a running run must not yet carry an artifactId");

  log(`phase9: research run ${runId} is running (blocked on the stub) — shutting down mid-flight`);

  await shutdownAndWaitExit(conn);
  log(`phase9 OK: orchd (pid ${cleanup.daemonPid}) process exited (pre-reconcile-restart)`);

  conn = await bootAndConnect();
  assert.equal(
    conn.chosenVersion,
    1,
    `post-reconcile-restart preamble handshake negotiated unexpected version: ${JSON.stringify(conn)}`,
  );

  const runAfterRestart = await orchdRequest(conn, { t: "ResearchGetRun", id: runId });
  assert.equal(
    runAfterRestart.t,
    "ResearchRun",
    `ResearchGetRun (post-restart) -> ${JSON.stringify(runAfterRestart)}`,
  );
  assert.equal(
    runAfterRestart.value.status,
    "failed",
    `boot-reconcile must flip an interrupted run to failed, got ${JSON.stringify(runAfterRestart.value)}`,
  );
  assert.equal(
    runAfterRestart.value.errorKind,
    "interrupted",
    `boot-reconcile must set errorKind=interrupted, got ${JSON.stringify(runAfterRestart.value)}`,
  );
  assert.equal(runAfterRestart.value.artifactId, null, "a failed run must not carry an artifactId");

  await blockingStub.close();
  cleanup.stubMcpServer = null;
  log("phase9: BLOCKING stub MCP research server closed");

  log("phase9 OK: interrupted research run reconciled on restart");
  return conn;
}

/**
 * Tear down everything this run created, regardless of how `main()` exited. Every step is
 * independently best-effort — this is cleanup code, not test assertions.
 */
async function cleanupAll() {
  if (cleanup.stubMcpServer != null) {
    try {
      await cleanup.stubMcpServer.close();
    } catch {
      /* best-effort cleanup */
    }
    cleanup.stubMcpServer = null;
  }

  if (cleanup.stubRestServer != null) {
    try {
      await cleanup.stubRestServer.close();
    } catch {
      /* best-effort cleanup */
    }
    cleanup.stubRestServer = null;
  }

  // Best-effort safety net: if phase7 created a connector account (the real invoke account OR the
  // throwaway keychain-availability probe — BOTH are REAL Keychain entries, not just isolated-
  // tempdir state) but a LATER assertion in that same phase threw before its own explicit
  // `ConnectorDeleteAccount` ran, clean it up here too — BEFORE the daemon is torn down below (a
  // dead daemon can't service this request). This is cleanup for an already-failing run, not a
  // test assertion, so nothing here throws or changes the run's verdict.
  //
  // Everything EXCEPT these entries lives under `isolatedTmpDir`/`isolatedHomeDir` and is deleted
  // wholesale at the end of this function, so a Keychain entry is the ONE thing this harness can
  // leave behind on the real machine — which is exactly why a failure here is REPORTED (loudly,
  // with the id and a copy-pasteable remedy) instead of swallowed. A silently skipped delete is
  // indistinguishable from a clean run, and that is how leaks survive unnoticed.
  const orphanedConnectorAccountIds = [
    cleanup.connectorAccountId,
    cleanup.connectorProbeAccountId,
  ].filter((id) => id != null);
  if (orphanedConnectorAccountIds.length > 0) {
    // The most recent tracked connection may already be dead — `shutdownAndWaitExit()` empties
    // `cleanup.conns` at every phase boundary, and a run that died between a shutdown and the next
    // `bootAndConnect()` leaves none at all. Pick the last LIVE one; failing that, open a fresh one
    // if the daemon process is still up.
    let conn = null;
    for (let i = cleanup.conns.length - 1; i >= 0; i--) {
      const c = cleanup.conns[i];
      if (c?.sock && !c.sock.destroyed) {
        conn = c;
        break;
      }
    }
    if (conn == null && cleanup.daemonPid != null && pidAlive(cleanup.daemonPid)) {
      try {
        conn = await orchdConnect(SOCK);
        cleanup.conns.push(conn);
      } catch {
        conn = null;
      }
    }
    for (const id of orphanedConnectorAccountIds) {
      let failure = "no live orchd connection (daemon already gone?)";
      if (conn != null) {
        try {
          const res = await orchdRequest(conn, { t: "ConnectorDeleteAccount", id });
          failure = res.t === "Ack" ? null : `orchd answered ${JSON.stringify(res)}`;
        } catch (e) {
          failure = e.message;
        }
      }
      if (failure == null) {
        log(`cleanup: deleted orphaned connector account ${id}`);
        continue;
      }
      console.error(
        `[e2e-orchd] !! LEAKED KEYCHAIN ENTRY: connector account ${id} could not be deleted ` +
          `(${failure}). Unlike everything else this harness creates, this lives in your REAL ` +
          `login keychain (the isolated HOME only symlinks Library/Keychains). Remove it with: ` +
          `security delete-generic-password -s ai.builderpro.desktop.account -a "${id}:apikey"`,
      );
    }
    cleanup.connectorAccountId = null;
    cleanup.connectorProbeAccountId = null;
  }

  for (const c of cleanup.conns) {
    try {
      c.sock.destroy();
    } catch {
      /* already closed */
    }
  }

  if (cleanup.daemonPid != null) {
    killProcessGroup(cleanup.daemonPid, "SIGTERM");
    for (let i = 0; i < 50 && pidAlive(cleanup.daemonPid); i++) await sleep(100);
    if (pidAlive(cleanup.daemonPid)) {
      log(`daemon (pid ${cleanup.daemonPid}) still alive after SIGTERM grace period; sending SIGKILL`);
      killProcessGroup(cleanup.daemonPid, "SIGKILL");
      for (let i = 0; i < 20 && pidAlive(cleanup.daemonPid); i++) await sleep(100);
    }
    if (pidAlive(cleanup.daemonPid)) {
      log(`WARNING: daemon (pid ${cleanup.daemonPid}) survived SIGKILL — manual cleanup may be needed`);
    }
  }

  for (const dir of [isolatedTmpDir, isolatedHomeDir]) {
    try {
      fs.rmSync(dir, { recursive: true, force: true });
    } catch {
      /* best-effort cleanup */
    }
  }
}

main()
  .catch((e) => {
    console.error("[e2e-orchd] FAIL:", e);
    process.exitCode = 1;
  })
  .finally(async () => {
    await cleanupAll();
  });
