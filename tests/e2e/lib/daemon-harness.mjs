// Hop-B client harness for the E2E survive-restart test (spec §7, §14.1).
//
// Re-implements the exact bincode 1.3.3 (fixint, little-endian) encoding for the subset
// of `Frame`/`Request`/`Response`/`Push` this harness sends and receives, against the
// real daemon socket protocol — no Rust/WASM bridge, no test-runner dependency.
//
// Wire format (locked, spec §7 + `crates/protocol/src/framing.rs`):
//   u32-LE length prefix + bincode(Frame) body.
// bincode 1.3.3 primitives used here:
//   - enum variant index -> u32-LE
//   - String / Vec<u8>   -> u64-LE length + raw bytes (no NUL, no padding)
//   - Vec<T>             -> u64-LE length + items
//   - Option<T>          -> 1 byte tag (0 = None, 1 = Some) + inner (only if Some)
//   - struct / tuple     -> fields in declaration order, no header
//
// Variant orders below are transcribed directly from `crates/protocol/src/lib.rs`
// (`Frame`, `Request`, `Response`, `Push`, `SessionLifecycle`) — see the doc comment on
// each decoder function for the exact source ordering. A mismatch here fails the very
// first `Welcome` roundtrip (self-checking, per the task brief's locked rationale).

import net from "node:net";
import { execFileSync, spawn } from "node:child_process";
import os from "node:os";
import path from "node:path";
import fs from "node:fs";

export const MAGIC = 0x42504131; // "BPA1" big-endian ASCII, spec §7 / crates/protocol
export const PROTO_VERSION = 1;

// ---- bincode 1.3.3 (fixint, little-endian) minimal encoder ----

function u32le(n) {
  const b = Buffer.alloc(4);
  b.writeUInt32LE(n >>> 0, 0);
  return b;
}
function u64le(n) {
  const b = Buffer.alloc(8);
  b.writeBigUInt64LE(BigInt(n), 0);
  return b;
}
function u16le(n) {
  const b = Buffer.alloc(2);
  b.writeUInt16LE(n & 0xffff, 0);
  return b;
}
function encStr(s) {
  const body = Buffer.from(s, "utf8");
  return Buffer.concat([u64le(body.length), body]);
}
function encBytes(v) {
  const body = Buffer.from(v);
  return Buffer.concat([u64le(body.length), body]);
}
function encEnvOverrides(pairs) {
  const parts = [u64le(pairs.length)];
  for (const [k, val] of pairs) parts.push(encStr(k), encStr(val));
  return Buffer.concat(parts);
}
function encOptStr(s) {
  return s == null ? Buffer.from([0]) : Buffer.concat([Buffer.from([1]), encStr(s)]);
}

/**
 * `Request` enum variant order — MUST match `crates/protocol/src/lib.rs` `pub enum Request`:
 *   0 Hello, 1 ListWorkspaces, 2 CreateWorkspace, 3 ListSessions, 4 CreateSession,
 *   5 AttachSession, 6 DetachSession, 7 WriteStdin, 8 Resize, 9 KillSession,
 *   10 GetSessionState, 11 DaemonShutdown
 * Only the variants this harness actually sends are encoded; others throw loudly rather
 * than silently miscoding (fail-fast if a future phase needs one not yet implemented).
 */
function encRequest(req) {
  switch (req.t) {
    case "Hello":
      return Buffer.concat([u32le(0), u32le(req.magic), u16le(req.protoVersion), encStr(req.clientBuild)]);
    case "ListWorkspaces":
      return u32le(1);
    case "CreateWorkspace":
      return Buffer.concat([u32le(2), encStr(req.name), encStr(req.rootPath)]);
    case "ListSessions":
      return u32le(3);
    case "CreateSession":
      return Buffer.concat([
        u32le(4),
        encStr(req.workspaceId),
        encOptStr(req.shell),
        encOptStr(req.cwd),
        encEnvOverrides(req.envOverrides ?? []),
        u16le(req.cols),
        u16le(req.rows),
      ]);
    case "AttachSession":
      return Buffer.concat([u32le(5), encStr(req.sessionId)]);
    case "DetachSession":
      return Buffer.concat([u32le(6), encStr(req.sessionId)]);
    case "WriteStdin":
      return Buffer.concat([u32le(7), encStr(req.sessionId), encBytes(req.bytes)]);
    case "Resize":
      return Buffer.concat([u32le(8), encStr(req.sessionId), u16le(req.cols), u16le(req.rows)]);
    case "KillSession":
      return Buffer.concat([u32le(9), encStr(req.sessionId)]);
    case "GetSessionState":
      return Buffer.concat([u32le(10), encStr(req.sessionId)]);
    case "DaemonShutdown":
      return Buffer.concat([u32le(11), Buffer.from([req.drain ? 1 : 0])]);
    default:
      throw new Error(`unsupported request type ${req.t}`);
  }
}

/**
 * `Frame` enum variant order — MUST match `crates/protocol/src/lib.rs` `pub enum Frame`:
 *   0 Request { id, req }, 1 Response { id, res }, 2 Push(push)
 * The harness only ever *sends* `Request` frames (Response/Push are daemon -> client only).
 */
export function encodeFrame(frame) {
  let body;
  if (frame.t === "Request") body = Buffer.concat([u32le(0), u64le(frame.id), encRequest(frame.req)]);
  else throw new Error("harness only sends Request frames");
  return Buffer.concat([u32le(body.length), body]);
}

// ---- decoder (a cursor over a Buffer) ----

class Cur {
  constructor(buf) {
    this.b = buf;
    this.o = 0;
  }
  u32() {
    const v = this.b.readUInt32LE(this.o);
    this.o += 4;
    return v;
  }
  u16() {
    const v = this.b.readUInt16LE(this.o);
    this.o += 2;
    return v;
  }
  u64() {
    const v = Number(this.b.readBigUInt64LE(this.o));
    this.o += 8;
    return v;
  }
  u8() {
    const v = this.b.readUInt8(this.o);
    this.o += 1;
    return v;
  }
  str() {
    const n = this.u64();
    const s = this.b.toString("utf8", this.o, this.o + n);
    this.o += n;
    return s;
  }
  bytes() {
    const n = this.u64();
    const s = this.b.subarray(this.o, this.o + n);
    this.o += n;
    return s;
  }
  optU8() {
    return this.u8() === 0 ? null : this.u8();
  }
  optStr() {
    return this.u8() === 0 ? null : this.str();
  }
}

/**
 * `SessionLifecycle` wire shape (spec §7 dual-codec note, `crates/protocol/src/lib.rs`):
 * over Hop-B (bincode / non-human-readable) it is serialized as a plain bincode `String`
 * holding the JSON-tagged shape (`{"kind":"atPrompt"}` etc.), NOT as a raw bincode enum
 * discriminant. Decode it as a length-prefixed UTF-8 string, then JSON.parse.
 */
function decLifecycle(c) {
  const json = c.str();
  const shape = JSON.parse(json);
  return shape; // { kind: "atPrompt" | "typing" | "running" | "exited", code?, signal? }
}
function decSessionMeta(c) {
  return {
    id: c.str(),
    workspaceId: c.str(),
    title: c.str(),
    shell: c.str(),
    cwd: c.str(),
    cols: c.u16(),
    rows: c.u16(),
    lifecycle: decLifecycle(c),
    waitingForInput: c.u8() === 1,
    isActive: c.u8() === 1,
    createdAt: c.u64(),
  };
}
function decWorkspace(c) {
  return { id: c.str(), name: c.str(), rootPath: c.str() };
}

/**
 * `Response` enum variant order — MUST match `crates/protocol/src/lib.rs` `pub enum Response`:
 *   0 Welcome, 1 Incompatible, 2 Workspaces, 3 Workspace, 4 Sessions, 5 Session, 6 Ack, 7 Error
 */
function decResponse(c) {
  const v = c.u32();
  switch (v) {
    case 0:
      return { t: "Welcome", protoVersion: c.u16(), daemonBuild: c.str() };
    case 1:
      return { t: "Incompatible", min: c.u16(), max: c.u16() };
    case 2: {
      const n = c.u64();
      const a = [];
      for (let i = 0; i < n; i++) a.push(decWorkspace(c));
      return { t: "Workspaces", value: a };
    }
    case 3:
      return { t: "Workspace", value: decWorkspace(c) };
    case 4: {
      const n = c.u64();
      const a = [];
      for (let i = 0; i < n; i++) a.push(decSessionMeta(c));
      return { t: "Sessions", value: a };
    }
    case 5:
      return { t: "Session", value: decSessionMeta(c) };
    case 6:
      return { t: "Ack" };
    case 7:
      return { t: "Error", code: c.str(), message: c.str() };
    default:
      throw new Error(`unknown Response variant ${v}`);
  }
}

/**
 * `Push` enum variant order — MUST match `crates/protocol/src/lib.rs` `pub enum Push`:
 *   0 Replay, 1 Output, 2 StateChanged, 3 ChildExited, 4 SessionCreated,
 *   5 WorkspaceCreated, 6 Error
 */
function decPush(c) {
  const v = c.u32();
  switch (v) {
    case 0:
      return { t: "Replay", sessionId: c.str(), cols: c.u16(), rows: c.u16(), content: c.bytes() };
    case 1:
      return { t: "Output", sessionId: c.str(), bytes: c.bytes() };
    case 2:
      return {
        t: "StateChanged",
        sessionId: c.str(),
        lifecycle: decLifecycle(c),
        waitingForInput: c.u8() === 1,
        cwd: c.str(),
      };
    case 3:
      return { t: "ChildExited", sessionId: c.str(), code: c.optU8(), signal: c.optStr() };
    case 4:
      return { t: "SessionCreated", meta: decSessionMeta(c) };
    case 5:
      return { t: "WorkspaceCreated", workspace: decWorkspace(c) };
    case 6:
      return { t: "Error", sessionId: c.optStr(), code: c.str(), message: c.str() };
    default:
      throw new Error(`unknown Push variant ${v}`);
  }
}

/** `Frame` enum: 0 Request (never decoded by this client), 1 Response, 2 Push. */
export function decodeFrame(buf) {
  const c = new Cur(buf);
  const fv = c.u32();
  if (fv === 1) return { t: "Response", id: c.u64(), res: decResponse(c) };
  if (fv === 2) return { t: "Push", push: decPush(c) };
  if (fv === 0) throw new Error("harness does not decode Request frames");
  throw new Error(`unknown Frame variant ${fv}`);
}

// ---- socket path resolution (spec §8.1) ----

export function resolveSocketPath() {
  const runtime = process.env.XDG_RUNTIME_DIR;
  const dir =
    runtime && runtime.length > 0 ? path.join(runtime, "bpa") : path.join("/tmp", `bpa-${os.userInfo().uid}`);
  return path.join(dir, "d.sock");
}

// ---- socket connection with length-prefixed framing ----

export function connect(sockPath) {
  return new Promise((resolve, reject) => {
    const sock = net.connect(sockPath);
    const conn = { sock, buf: Buffer.alloc(0), pending: [], pushes: [], waiters: [] };
    sock.once("connect", () => resolve(conn));
    sock.once("error", reject);
    sock.on("data", (chunk) => {
      conn.buf = Buffer.concat([conn.buf, chunk]);
      for (;;) {
        if (conn.buf.length < 4) break;
        const len = conn.buf.readUInt32LE(0);
        if (conn.buf.length < 4 + len) break;
        const body = conn.buf.subarray(4, 4 + len);
        conn.buf = conn.buf.subarray(4 + len);
        const frame = decodeFrame(body);
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
  });
}

let nextId = 1;

export function request(conn, req) {
  const id = nextId++;
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      conn.pending = conn.pending.filter((p) => p.id !== id);
      reject(new Error(`request ${req.t} timed out`));
    }, 10000);
    conn.pending.push({
      id,
      resolve: (v) => {
        clearTimeout(timer);
        resolve(v);
      },
      reject,
    });
    conn.sock.write(encodeFrame({ t: "Request", id, req }));
  });
}

export function hello(conn) {
  return request(conn, { t: "Hello", magic: MAGIC, protoVersion: PROTO_VERSION, clientBuild: "e2e-harness" });
}

/**
 * Resolve with the next push matching `pred`. Checks the already-buffered `conn.pushes`
 * queue first (so pushes that arrived before this call was made are not missed), then
 * waits for a future push. Rejects on timeout — callers must not treat that as "OK, no
 * push arrived"; a missing push is a real assertion failure (spec: "no assertion weakened
 * to pass vacuously").
 */
export function nextPush(conn, pred, timeoutMs = 15000) {
  const existing = conn.pushes.find(pred);
  if (existing) {
    conn.pushes = conn.pushes.filter((p) => p !== existing);
    return Promise.resolve(existing);
  }
  return new Promise((resolve, reject) => {
    const w = {
      pred,
      resolve: (v) => {
        clearTimeout(timer);
        resolve(v);
      },
    };
    const timer = setTimeout(() => {
      conn.waiters = conn.waiters.filter((x) => x !== w);
      reject(new Error("push wait timed out"));
    }, timeoutMs);
    conn.waiters.push(w);
  });
}

// ---- process probes (survive-restart core) ----

/** All PIDs of a running `bpa-sessiond`, or `[]` if none found. Never throws. */
export function pgrepDaemon() {
  try {
    return execFileSync("pgrep", ["-x", "bpa-sessiond"], { encoding: "utf8" }).trim().split("\n").filter(Boolean);
  } catch {
    return [];
  }
}

/**
 * Direct child PIDs of `parentPid` (used to find the shell process the daemon spawned
 * under it), or `[]`. Named `pgrepShell` per the harness's locked export contract
 * (task brief "Produces"); despite the name it is a generic `pgrep -P` child lookup —
 * the daemon's only direct child in this harness's scenarios is the session's shell.
 */
export function pgrepShell(parentPid) {
  try {
    return execFileSync("pgrep", ["-P", String(parentPid)], { encoding: "utf8" }).trim().split("\n").filter(Boolean);
  } catch {
    return [];
  }
}

/** `kill -0` liveness probe — true iff a process with `pid` exists and is signalable. */
export function pidAlive(pid) {
  try {
    process.kill(Number(pid), 0);
    return true;
  } catch {
    return false;
  }
}

/** Spawn `bpa-sessiond` detached (out-of-band, as launchd would) bound to `sockPath`. */
export function spawnDaemon(binPath, sockPath) {
  fs.mkdirSync(path.dirname(sockPath), { recursive: true, mode: 0o700 });
  const child = spawn(binPath, ["--socket", sockPath], { stdio: "ignore", detached: true });
  child.unref();
  return child;
}

/** `launchctl kickstart gui/$UID/<label>` — used by the launchd-managed harness variant. */
export function launchctlKickstart(label) {
  const uid = os.userInfo().uid;
  execFileSync("launchctl", ["kickstart", "-k", `gui/${uid}/${label}`], { stdio: "inherit" });
}

/** `launchctl kill TERM gui/$UID/<label>` — simulates the GUI quitting via its own path. */
export function killGui(label) {
  const uid = os.userInfo().uid;
  execFileSync("launchctl", ["kill", "TERM", `gui/${uid}/${label}`], { stdio: "inherit" });
}
