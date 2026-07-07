// Hop-B client harness for the E2E survive-restart test (Pv2 §4.2, §7, §9.8).
//
// Speaks the v2 wire directly against the real daemon socket protocol — no Rust/WASM
// bridge, no test-runner dependency:
//   1. A codec-agnostic PREAMBLE handshake (raw little-endian primitives, no CBOR/bincode)
//      — see `crates/protocol/src/preamble.rs` (`encode_client_preamble`/`decode_daemon_reply`,
//      reproduced here byte-for-byte).
//   2. Once negotiated, a CBOR(RFC 8949) frame stream: `u32-LE length prefix | CBOR(Frame) body`
//      — see `crates/protocol/src/framing.rs`. `ciborium` decodes the body on the daemon side, so
//      the bytes emitted here must be standard CBOR, not any non-standard extension.
//
// This is a hand-rolled MINIMAL CBOR encoder/decoder (not a dependency) covering exactly the
// shapes `Frame`/`Request`/`Response`/`Push` (+ nested `SessionMeta`/`Workspace`/`SessionLifecycle`)
// need — matching this harness's existing hand-rolled-codec philosophy (previously bincode) and
// sidestepping interop traps a general-purpose CBOR library can introduce (e.g. `cbor-x`'s default
// non-standard record/tag-105 extension, which `ciborium` rejects).
//
// **CBOR shape rules (Pv2 §4.3/§7, task-12 brief) — get these exactly right:**
//   - `Frame`/`Request`/`Response`/`Push` are EXTERNALLY TAGGED, NO `rename_all`:
//       unit variant  -> bare CBOR text string, e.g. `Request::ListSessions` -> `"ListSessions"`.
//       struct variant -> single-entry map `{ "VariantName": { snake_case fields... } }`.
//       newtype variant -> `{ "VariantName": <inner> }`.
//   - Nested domain structs (`SessionMeta`, `Workspace`) ARE `camelCase` (`rename_all = "camelCase"`
//     on the Rust side) — `SessionMeta.isActive`, `.workspaceId`, `.waitingForInput`, etc.
//   - `SessionLifecycle` is INTERNALLY TAGGED (`tag = "kind"`, camelCase): `{ "kind": "atPrompt" }`,
//     `{ "kind": "exited", "code": 0|null, "signal": "..."|null }`.
//   - `Vec<u8>` fields (`WriteStdin.bytes`, `Push::Output.bytes`, `Push::Replay.content`) are a CBOR
//     ARRAY OF UNSIGNED INTEGERS (serde's `Vec<u8>` uses `serialize_seq`, NOT a CBOR byte string;
//     `ciborium`'s `Deserialize` for `Vec<u8>` REJECTS major-type-2 byte strings). Encode as plain
//     JS `number[]`; decode yields a `number[]` (callers `Buffer.from(arr)` to get text/bytes).
//
// Variant field names below are transcribed directly from `crates/protocol/src/lib.rs`. A shape
// mismatch here fails the very first real request in phase 0 loudly (self-checking, per the task
// brief's locked rationale) — CBOR is self-describing, so a wrong key/shape surfaces as either a
// `ciborium` decode error on the daemon side (connection closes) or a decode exception here.

import net from "node:net";
import { execFileSync, spawn } from "node:child_process";
import os from "node:os";
import path from "node:path";
import fs from "node:fs";

// "BPAA" ASCII read big-endian; encoded little-endian on the wire (raw bytes b"AAPB").
// Locked in `crates/protocol/src/preamble.rs::PREAMBLE_MAGIC` (Pv2 §4.2).
export const MAGIC = 0x42504141;
export const CLIENT_MIN_VERSION = 2;
export const CLIENT_MAX_VERSION = 2;
export const CLIENT_BUILD = "e2e-harness";

// ============================================================================
// Preamble handshake codec (raw LE primitives — NOT CBOR/bincode; see preamble.rs doc comment).
// ============================================================================

/**
 * Encode the client preamble: `magic:u32-LE | min:u16-LE | max:u16-LE | build_len:u16-LE | build[..]`.
 * Mirrors `crates/protocol/src/preamble.rs::encode_client_preamble` exactly.
 */
export function encodeClientPreamble(min, max, build) {
  const buildBytes = Buffer.from(build, "utf8");
  const out = Buffer.alloc(4 + 2 + 2 + 2 + buildBytes.length);
  let o = 0;
  out.writeUInt32LE(MAGIC, o); o += 4;
  out.writeUInt16LE(min, o); o += 2;
  out.writeUInt16LE(max, o); o += 2;
  out.writeUInt16LE(buildBytes.length, o); o += 2;
  buildBytes.copy(out, o);
  return out;
}

/**
 * Read exactly `n` bytes from `sock`, buffering across multiple `data` events (the daemon reply
 * "may arrive in >=1 chunks" per the brief). Returns a Promise<Buffer>.
 *
 * `state` is a small mutable cursor `{ buf: Buffer }` SHARED across every `readExactly` call made
 * during the handshake (both the 9-byte header read and the trailing `build` bytes read) — any
 * bytes beyond what THIS call needs stay in `state.buf` for the NEXT call to consume first, rather
 * than being pushed back onto the socket via `sock.unshift()`. `unshift()` is unsafe to rely on
 * here: once a `data` listener has been attached (switching the stream to flowing mode), unshifted
 * bytes re-emit as a fresh `data` event on a later tick, racing whichever listener happens to be
 * attached at that moment — if the NEXT `readExactly` call hasn't attached its own listener yet
 * (e.g. because of the `await` between calls), the re-emitted event fires with no listener
 * attached and the bytes are silently lost, hanging the caller forever. A single shared buffer
 * `state` avoids the race entirely: only one `data` listener is ever live at a time, spanning both
 * reads, and leftover bytes are handed off in-memory rather than round-tripped through the stream.
 */
function readExactly(sock, n, timeoutMs, state) {
  if (state.buf.length >= n) {
    const out = state.buf.subarray(0, n);
    state.buf = state.buf.subarray(n);
    return Promise.resolve(out);
  }
  return new Promise((resolve, reject) => {
    const onData = (chunk) => {
      state.buf = Buffer.concat([state.buf, chunk]);
      if (state.buf.length >= n) {
        cleanup();
        const out = state.buf.subarray(0, n);
        state.buf = state.buf.subarray(n);
        resolve(out);
      }
    };
    const onError = (e) => {
      cleanup();
      reject(e);
    };
    const onClose = () => {
      cleanup();
      reject(new Error(`socket closed after ${state.buf.length}/${n} preamble-reply bytes`));
    };
    const timer = setTimeout(() => {
      cleanup();
      reject(new Error(`timed out waiting for ${n} preamble-reply bytes (got ${state.buf.length})`));
    }, timeoutMs);
    function cleanup() {
      clearTimeout(timer);
      sock.off("data", onData);
      sock.off("error", onError);
      sock.off("close", onClose);
    }
    sock.on("data", onData);
    sock.on("error", onError);
    sock.on("close", onClose);
  });
}

/**
 * Perform the v2 preamble handshake on a freshly-connected, not-yet-framed socket: write the
 * client preamble, then read+decode the daemon's reply per
 * `crates/protocol/src/preamble.rs::decode_daemon_reply`:
 *   9-byte header `magic:u32-LE | result:u8 | a:u16-LE | b:u16-LE`, then:
 *     - result==1 (Accepted): a=chosen version, b=build_len, followed by `build_len` more bytes.
 *     - result==0 (Incompatible): a=daemon_min, b=daemon_max, no trailing bytes.
 * Throws loudly (wrong magic / result / chosen version / short read) — self-checking, matching
 * the old `Welcome` roundtrip's fail-fast contract.
 *
 * Returns `{ chosen, daemonBuild, leftover }`: `leftover` is any bytes read past the preamble reply
 * in the same chunk(s) (the daemon is free to pipeline the start of the CBOR frame stream right
 * after its preamble reply, and a single `data` event can legitimately contain both) — the caller
 * MUST prepend `leftover` to its own frame-stream buffer rather than discarding it.
 */
export async function preambleHandshake(sock, { timeoutMs = 5000 } = {}) {
  sock.write(encodeClientPreamble(CLIENT_MIN_VERSION, CLIENT_MAX_VERSION, CLIENT_BUILD));

  // Shared read cursor spanning BOTH reads below (header, then trailing build bytes) — see the
  // `readExactly` doc comment for why a single shared buffer replaces a `sock.unshift()`-based
  // approach (unshifting after a `data` listener is already attached races the next listener).
  const state = { buf: Buffer.alloc(0) };

  const header = await readExactly(sock, 9, timeoutMs, state);
  const magic = header.readUInt32LE(0);
  if (magic !== MAGIC) {
    throw new Error(
      `preamble reply bad magic: expected 0x${MAGIC.toString(16)}, got 0x${magic.toString(16)}`,
    );
  }
  const result = header.readUInt8(4);
  const a = header.readUInt16LE(5);
  const b = header.readUInt16LE(7);

  if (result === 0) {
    throw new Error(`daemon reported Incompatible: daemon supports [${a}, ${b}], client wanted [${CLIENT_MIN_VERSION}, ${CLIENT_MAX_VERSION}]`);
  }
  if (result !== 1) {
    throw new Error(`preamble reply unknown result byte: ${result}`);
  }

  const chosen = a;
  const buildLen = b;
  if (chosen !== 2) {
    throw new Error(`preamble negotiated unexpected version: chosen=${chosen} (expected 2)`);
  }
  const buildBytes = buildLen > 0 ? await readExactly(sock, buildLen, timeoutMs, state) : Buffer.alloc(0);
  const daemonBuild = buildBytes.toString("utf8");
  return { chosen, daemonBuild, leftover: state.buf };
}

// ============================================================================
// Minimal standard CBOR (RFC 8949) encoder/decoder — hand-rolled, no dependency.
// Covers exactly: unsigned/negative ints, text strings, bool, null, arrays (definite-length),
// maps (definite-length, text-string keys). That is everything `Frame`/`Request`/`Response`/
// `Push` + nested `SessionMeta`/`Workspace`/`SessionLifecycle` need.
// ============================================================================

const MT_UINT = 0, MT_NEGINT = 1, MT_TEXT = 3, MT_ARRAY = 4, MT_MAP = 5, MT_SIMPLE = 7;

function cborWriteHead(out, majorType, argument) {
  const mt = majorType << 5;
  if (argument < 24) {
    out.push(mt | argument);
  } else if (argument <= 0xff) {
    out.push(mt | 24, argument);
  } else if (argument <= 0xffff) {
    out.push(mt | 25, (argument >> 8) & 0xff, argument & 0xff);
  } else if (argument <= 0xffffffff) {
    out.push(
      mt | 26,
      (argument >>> 24) & 0xff,
      (argument >>> 16) & 0xff,
      (argument >>> 8) & 0xff,
      argument & 0xff,
    );
  } else {
    // 64-bit length/argument (e.g. a very large u64 id) — used rarely (session/frame ids fit in
    // 53-bit safe-integer range for this harness), but handled correctly rather than truncated.
    const big = BigInt(argument);
    out.push(mt | 27);
    for (let shift = 56n; shift >= 0n; shift -= 8n) {
      out.push(Number((big >> shift) & 0xffn));
    }
  }
}

/** Encode a JS value to CBOR bytes. Supports: number (as uint/negint; use {u64: n|bigint} or
 * {bytesArray: number[]} wrapper types below for disambiguation where needed), string, boolean,
 * null/undefined (-> CBOR null), plain array (-> CBOR array), plain object (-> CBOR map, insertion
 * order, string keys only). */
export function cborEncode(value) {
  const out = [];
  encodeValue(out, value);
  return Buffer.from(out);
}

function encodeValue(out, value) {
  if (value === null || value === undefined) {
    out.push((MT_SIMPLE << 5) | 22); // null
    return;
  }
  if (typeof value === "boolean") {
    out.push((MT_SIMPLE << 5) | (value ? 21 : 20));
    return;
  }
  if (typeof value === "bigint") {
    if (value >= 0n) {
      cborWriteHead(out, MT_UINT, value);
    } else {
      cborWriteHead(out, MT_NEGINT, -value - 1n);
    }
    return;
  }
  if (typeof value === "number") {
    if (!Number.isInteger(value)) {
      throw new Error(`cborEncode: non-integer numbers not supported (got ${value})`);
    }
    if (value >= 0) {
      cborWriteHead(out, MT_UINT, value);
    } else {
      cborWriteHead(out, MT_NEGINT, -value - 1);
    }
    return;
  }
  if (typeof value === "string") {
    const bytes = Buffer.from(value, "utf8");
    cborWriteHead(out, MT_TEXT, bytes.length);
    for (const b of bytes) out.push(b);
    return;
  }
  if (Array.isArray(value)) {
    cborWriteHead(out, MT_ARRAY, value.length);
    for (const item of value) encodeValue(out, item);
    return;
  }
  if (typeof value === "object") {
    const keys = Object.keys(value);
    cborWriteHead(out, MT_MAP, keys.length);
    for (const k of keys) {
      encodeValue(out, k);
      encodeValue(out, value[k]);
    }
    return;
  }
  throw new Error(`cborEncode: unsupported value type ${typeof value}`);
}

/** Decode a single CBOR value from `buf` starting at `offset`. Returns `[value, nextOffset]`.
 * Definite-length arrays/maps only (this harness never emits or expects indefinite-length items,
 * and `ciborium`'s writer never produces them for these shapes either). Map keys are decoded as
 * CBOR text strings (major type 3) — this is exactly what every map key in `Frame`/`Request`/
 * `Response`/`Push`/`SessionMeta`/`Workspace`/`SessionLifecycle` is. */
function decodeValue(buf, offset) {
  const first = buf[offset];
  const majorType = first >> 5;
  const infoBits = first & 0x1f;
  let o = offset + 1;

  function readArg() {
    if (infoBits < 24) return infoBits;
    if (infoBits === 24) {
      const v = buf.readUInt8(o); o += 1; return v;
    }
    if (infoBits === 25) {
      const v = buf.readUInt16BE(o); o += 2; return v;
    }
    if (infoBits === 26) {
      const v = buf.readUInt32BE(o); o += 4; return v;
    }
    if (infoBits === 27) {
      const v = buf.readBigUInt64BE(o); o += 8;
      return v <= BigInt(Number.MAX_SAFE_INTEGER) ? Number(v) : v;
    }
    throw new Error(`CBOR decode: unsupported additional info ${infoBits} at offset ${offset}`);
  }

  switch (majorType) {
    case MT_UINT: {
      const n = readArg();
      return [n, o];
    }
    case MT_NEGINT: {
      const n = readArg();
      const val = typeof n === "bigint" ? -(n) - 1n : -(n) - 1;
      return [val, o];
    }
    case 2: { // byte string (major type 2) — not expected in these shapes, but decodable defensively
      const n = readArg();
      const bytes = buf.subarray(o, o + Number(n));
      return [Array.from(bytes), o + Number(n)];
    }
    case MT_TEXT: {
      const n = Number(readArg());
      const s = buf.toString("utf8", o, o + n);
      return [s, o + n];
    }
    case MT_ARRAY: {
      const n = Number(readArg());
      const arr = [];
      for (let i = 0; i < n; i++) {
        const [v, no] = decodeValue(buf, o);
        arr.push(v);
        o = no;
      }
      return [arr, o];
    }
    case MT_MAP: {
      const n = Number(readArg());
      const obj = {};
      for (let i = 0; i < n; i++) {
        const [k, ko] = decodeValue(buf, o);
        o = ko;
        const [v, vo] = decodeValue(buf, o);
        o = vo;
        obj[k] = v;
      }
      return [obj, o];
    }
    case MT_SIMPLE: {
      if (infoBits === 20) return [false, o];
      if (infoBits === 21) return [true, o];
      if (infoBits === 22) return [null, o];
      if (infoBits === 23) return [undefined, o];
      throw new Error(`CBOR decode: unsupported simple value ${infoBits} at offset ${offset}`);
    }
    default:
      throw new Error(`CBOR decode: unsupported major type ${majorType} at offset ${offset}`);
  }
}

/** Decode a single top-level CBOR value from `buf` (must consume the WHOLE buffer — a decoded
 * frame body is exactly one CBOR value per `ciborium::into_writer`/`from_reader` framing.rs
 * contract). Throws if trailing bytes remain (a shape mismatch — extra bytes mean our decoder
 * mis-parsed a nested field length). */
export function cborDecode(buf) {
  const [value, offset] = decodeValue(buf, 0);
  if (offset !== buf.length) {
    throw new Error(`CBOR decode: ${buf.length - offset} trailing byte(s) after top-level value`);
  }
  return value;
}

// ============================================================================
// Frame <-> CBOR shape mapping (Pv2 §4.3/§7; crates/protocol/src/lib.rs).
// ============================================================================

/** `Vec<u8>` wire rule: CBOR array of unsigned integers, not a byte string (see module doc
 * comment). `data` may be a `Buffer`/`Uint8Array`/`number[]`; always emits a plain `number[]`. */
function toByteArray(data) {
  return Array.from(data, (b) => Number(b));
}

function encEnvOverrides(pairs) {
  return (pairs ?? []).map(([k, v]) => [k, v]);
}

/**
 * Encode a `Request` (harness only ever SENDS `Request` frames) to its externally-tagged CBOR
 * shape. `req.t` selects the variant; unit variants -> bare string, struct variants -> single-key
 * map with snake_case fields (per `crates/protocol/src/lib.rs::Request`, no `rename_all`).
 */
function encodeRequest(req) {
  switch (req.t) {
    case "ListWorkspaces":
      return "ListWorkspaces";
    case "CreateWorkspace":
      return { CreateWorkspace: { name: req.name, root_path: req.rootPath } };
    case "ListSessions":
      return "ListSessions";
    case "CreateSession":
      return {
        CreateSession: {
          workspace_id: req.workspaceId,
          shell: req.shell ?? null,
          cwd: req.cwd ?? null,
          env_overrides: encEnvOverrides(req.envOverrides),
          cols: req.cols,
          rows: req.rows,
        },
      };
    case "AttachSession":
      return { AttachSession: { session_id: req.sessionId } };
    case "DetachSession":
      return { DetachSession: { session_id: req.sessionId } };
    case "WriteStdin":
      return { WriteStdin: { session_id: req.sessionId, bytes: toByteArray(req.bytes) } };
    case "Resize":
      return { Resize: { session_id: req.sessionId, cols: req.cols, rows: req.rows } };
    case "KillSession":
      return { KillSession: { session_id: req.sessionId } };
    case "GetSessionState":
      return { GetSessionState: { session_id: req.sessionId } };
    case "DaemonShutdown":
      return { DaemonShutdown: { drain: !!req.drain } };
    default:
      throw new Error(`unsupported request type ${req.t}`);
  }
}

/** Encode a `Frame::Request { id, req }` -> `{ "Request": { "id": <u64>, "req": <Request> } }`
 * (`crates/protocol/src/lib.rs::Frame`, externally tagged, snake_case fields). The harness only
 * ever sends `Request` frames — `Response`/`Push` are daemon -> client only. */
export function encodeFrame(frame) {
  if (frame.t !== "Request") throw new Error("harness only sends Request frames");
  const cborValue = { Request: { id: frame.id, req: encodeRequest(frame.req) } };
  const body = cborEncode(cborValue);
  const out = Buffer.alloc(4 + body.length);
  out.writeUInt32LE(body.length, 0);
  body.copy(out, 4);
  return out;
}

/** `SessionLifecycle` (`crates/protocol/src/lib.rs`): internally tagged on `kind`, camelCase.
 * `{ "kind": "atPrompt" | "typing" | "running" }` for unit variants, or
 * `{ "kind": "exited", "code": <u8>|null, "signal": <str>|null }` for the struct variant. Decoded
 * as-is (shape already matches the harness's existing `lifecycle.kind` consumption). */
function decodeLifecycle(shape) {
  return shape;
}

function decodeSessionMeta(m) {
  return {
    id: m.id,
    workspaceId: m.workspaceId,
    title: m.title,
    shell: m.shell,
    cwd: m.cwd,
    cols: m.cols,
    rows: m.rows,
    lifecycle: decodeLifecycle(m.lifecycle),
    waitingForInput: m.waitingForInput,
    isActive: m.isActive,
    createdAt: m.createdAt,
  };
}

function decodeWorkspace(w) {
  return { id: w.id, name: w.name, rootPath: w.rootPath };
}

/**
 * Decode a top-level `Frame` CBOR value (already parsed via `cborDecode`) into the harness's
 * internal `{ t, ... }` shape. `Frame` externally tagged: `{ "Response": {...} }` or
 * `{ "Push": <Push> }` (the harness never decodes `Request` frames — those are client -> daemon
 * only).
 */
export function decodeFrame(buf) {
  const value = cborDecode(buf);
  const keys = Object.keys(value);
  if (keys.length !== 1) {
    throw new Error(`decodeFrame: expected single-key Frame map, got keys ${JSON.stringify(keys)}`);
  }
  const [variant] = keys;
  if (variant === "Response") {
    const { id, res } = value.Response;
    return { t: "Response", id, res: decodeResponse(res) };
  }
  if (variant === "Push") {
    return { t: "Push", push: decodePush(value.Push) };
  }
  if (variant === "Request") {
    throw new Error("harness does not decode Request frames");
  }
  throw new Error(`decodeFrame: unknown Frame variant ${variant}`);
}

/**
 * Decode a `Response` (externally tagged; `crates/protocol/src/lib.rs::Response`). Unit variant
 * `Ack` arrives as the bare string `"Ack"`; every other variant is a single-key map.
 */
function decodeResponse(value) {
  if (value === "Ack") return { t: "Ack" };
  const keys = Object.keys(value);
  if (keys.length !== 1) {
    throw new Error(`decodeResponse: expected single-key map, got ${JSON.stringify(value)}`);
  }
  const [variant] = keys;
  const inner = value[variant];
  switch (variant) {
    case "Workspaces":
      return { t: "Workspaces", value: inner.map(decodeWorkspace) };
    case "Workspace":
      return { t: "Workspace", value: decodeWorkspace(inner) };
    case "Sessions":
      return { t: "Sessions", value: inner.map(decodeSessionMeta) };
    case "Session":
      return { t: "Session", value: decodeSessionMeta(inner) };
    case "Error":
      return { t: "Error", code: inner.code, message: inner.message };
    default:
      throw new Error(`decodeResponse: unknown Response variant ${variant}`);
  }
}

/**
 * Decode a `Push` (externally tagged; `crates/protocol/src/lib.rs::Push`) — always a single-key
 * map (`Push` has no unit variants).
 */
function decodePush(value) {
  const keys = Object.keys(value);
  if (keys.length !== 1) {
    throw new Error(`decodePush: expected single-key map, got ${JSON.stringify(value)}`);
  }
  const [variant] = keys;
  const inner = value[variant];
  switch (variant) {
    case "Replay":
      return {
        t: "Replay",
        sessionId: inner.session_id,
        cols: inner.cols,
        rows: inner.rows,
        content: inner.content, // number[] per the Vec<u8> wire rule
      };
    case "Output":
      return { t: "Output", sessionId: inner.session_id, bytes: inner.bytes };
    case "StateChanged":
      return {
        t: "StateChanged",
        sessionId: inner.session_id,
        lifecycle: decodeLifecycle(inner.lifecycle),
        waitingForInput: inner.waiting_for_input,
        cwd: inner.cwd,
      };
    case "ChildExited":
      return {
        t: "ChildExited",
        sessionId: inner.session_id,
        code: inner.code ?? null,
        signal: inner.signal ?? null,
      };
    case "SessionCreated":
      return { t: "SessionCreated", meta: decodeSessionMeta(inner.meta) };
    case "WorkspaceCreated":
      return { t: "WorkspaceCreated", workspace: decodeWorkspace(inner.workspace) };
    case "Error":
      return {
        t: "Error",
        sessionId: inner.session_id ?? null,
        code: inner.code,
        message: inner.message,
      };
    default:
      throw new Error(`decodePush: unknown Push variant ${variant}`);
  }
}

// ---- socket path resolution (spec §8.1) ----

export function resolveSocketPath() {
  const runtime = process.env.XDG_RUNTIME_DIR;
  const dir =
    runtime && runtime.length > 0 ? path.join(runtime, "bpa") : path.join("/tmp", `bpa-${os.userInfo().uid}`);
  return path.join(dir, "d.sock");
}

// ---- socket connection: preamble handshake, then length-prefixed CBOR framing ----

/**
 * Connect to `sockPath`, perform the v2 preamble handshake, then install the CBOR frame-stream
 * reader for the remainder of the connection's life. Resolves with a `conn` object once the
 * handshake has completed (`conn.daemonBuild`/`conn.chosenVersion` carry the negotiated reply) —
 * mirroring the old harness's `connect()` + separate `hello()` step, but folded into one function
 * since the preamble is no longer a framed `Request`/`Response` round-trip (it precedes framing
 * entirely, per Pv2 §4.2).
 */
export function connect(sockPath) {
  return new Promise((resolve, reject) => {
    const sock = net.connect(sockPath);
    sock.once("connect", async () => {
      try {
        const { chosen, daemonBuild, leftover } = await preambleHandshake(sock);
        const conn = {
          sock,
          // Seed the frame-stream buffer with any bytes read past the preamble reply in the same
          // chunk(s) — the daemon may pipeline the start of the CBOR stream right behind its
          // preamble reply, and `preambleHandshake` hands those bytes back rather than dropping
          // them (see its doc comment).
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
        resolve(conn);
      } catch (e) {
        sock.destroy();
        reject(e);
      }
    });
    sock.once("error", reject);
  });
}

let nextId = 1;

export function request(conn, req, id = nextId++) {
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

/**
 * Spawn `bpa-sessiond` detached (out-of-band, as launchd would) bound to `sockPath`, in its own
 * process group (`detached: true` on POSIX also makes the child its own session/group leader —
 * `setsid()` under the hood — so `killProcessGroup` below can signal the whole group, including any
 * shell children spawned under it, via a single negative-PID `kill`).
 *
 * `envOverrides` (e.g. `{ XDG_RUNTIME_DIR: <tmp>/xdg, HOME: <tmp>/home }`) is merged over
 * `process.env` so tests can fully isolate the daemon's lockfile/socket-dir resolution
 * (`crates/sessiond/src/singleton.rs` `socket_dir()`, keyed off `XDG_RUNTIME_DIR`) AND its durable
 * state dir (`crates/sessiond/src/boot.rs` `app_support_dir()`, keyed off `HOME` —
 * `~/Library/Application Support/ai.builderpro.desktop/bpa.db`) from the real user paths, even
 * though `--socket sockPath` already pins the *socket* itself.
 */
export function spawnDaemon(binPath, sockPath, envOverrides = {}) {
  fs.mkdirSync(path.dirname(sockPath), { recursive: true, mode: 0o700 });
  const child = spawn(binPath, ["--socket", sockPath], {
    stdio: "ignore",
    detached: true,
    env: { ...process.env, ...envOverrides },
  });
  child.unref();
  return child;
}

/**
 * Kill an entire process group by its leader pid (negative-PID `kill`, POSIX) — used to reap a
 * `spawnDaemon`-started daemon AND every child it spawned (the session's shell, and anything the
 * shell itself forked) in one signal, rather than relying on the daemon's own SIGTERM handler to
 * cascade the kill (which it deliberately does NOT do for live sessions — spec §7/§13 "sessions
 * keep running" on a client disconnect, but a leaked daemon during test cleanup must not leave its
 * shell orphaned either). Falls back to a single-pid kill if the group signal fails (e.g. already
 * reaped) — never throws, since this only runs during best-effort cleanup.
 */
export function killProcessGroup(pid, signal = "SIGTERM") {
  try {
    process.kill(-Number(pid), signal);
  } catch {
    try {
      process.kill(Number(pid), signal);
    } catch {
      /* already gone */
    }
  }
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
