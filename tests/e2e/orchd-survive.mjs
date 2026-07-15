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
import { startStubMcpServer } from "./lib/stub-mcp-server.mjs";

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
 * every inbound push, whether or not the test ever awaits it. */
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

function orchdRequest(conn, req, id = nextOrchdId++) {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      conn.pending = conn.pending.filter((p) => p.id !== id);
      reject(new Error(`orchd request ${req.t} timed out`));
    }, 10000);
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

/**
 * Cleanup state tracked across phases so the top-level `finally` in `main()` can tear everything
 * down on ANY exit path — success, a failed assertion mid-phase, or a request timeout.
 */
const cleanup = {
  daemonPid: null,
  conns: [],
  stubMcpServer: null,
};

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
  const shutdownAck = await orchdRequest(conn, { t: "OrchdShutdown", drain: true });
  assert.equal(shutdownAck.t, "Ack", `OrchdShutdown{drain:true} -> ${JSON.stringify(shutdownAck)}`);
  const daemonPid = cleanup.daemonPid;
  await waitForExit(daemonPid, 10000, "orchd");
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

  log("ALL PHASES PASSED");
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
