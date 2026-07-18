import { describe, it, expect } from "vitest";
import {
  classifyError,
  scrubSecrets,
  pushCapped,
  toSupportBundle,
  DIAG_CAP,
  type DiagEvent,
} from "./diag";

describe("classifyError", () => {
  it("maps a daemon CommandError to its code + message detail", () => {
    expect(classifyError({ kind: "daemon", code: "Invariant", message: "last workspace" })).toEqual({
      kind: "Invariant",
      detail: "last workspace",
    });
  });

  it("falls back to 'orchd' kind when a daemon error has no code, and null detail when no message", () => {
    expect(classifyError({ kind: "daemon" })).toEqual({ kind: "orchd", detail: null });
  });

  it("classifies the two wire kinds with no detail", () => {
    expect(classifyError({ kind: "disconnected" }).kind).toBe("disconnected");
    expect(classifyError({ kind: "incompatibleOrchd" }).kind).toBe("incompatibleOrchd");
  });

  it("classifies a real Error as unknown and keeps a detail anchor", () => {
    const r = classifyError(new Error("boom"));
    expect(r.kind).toBe("unknown");
    expect(r.detail).toContain("boom");
  });

  it("classifies a bare string and a null", () => {
    expect(classifyError("nope")).toEqual({ kind: "unknown", detail: "nope" });
    expect(classifyError(null)).toEqual({ kind: "unknown", detail: null });
  });
});

describe("scrubSecrets", () => {
  it("redacts a Bearer token", () => {
    expect(scrubSecrets("Authorization: Bearer abc.def-123")).not.toContain("abc.def-123");
    expect(scrubSecrets("Bearer abc.def-123")).toBe("Bearer «redacted»");
  });

  it("redacts key=value / token: value pairs", () => {
    expect(scrubSecrets("api_key=SECRETVAL123")).toBe("api_key=«redacted»");
    expect(scrubSecrets("token: abc123")).toBe("token: «redacted»");
    expect(scrubSecrets('password="hunter2"')).not.toContain("hunter2");
  });

  it("redacts known credential prefixes and the apple app-specific password shape", () => {
    expect(scrubSecrets("lin_api_AAbb11")).toBe("«redacted-key»");
    expect(scrubSecrets("sk-abcdefghijklmnop")).toBe("«redacted-key»");
    expect(scrubSecrets("bcvp-zaww-phyp-ohwi")).toBe("«redacted-pw»");
  });

  it("collapses the home-dir username", () => {
    expect(scrubSecrets("open /Users/alice/DATA/x failed")).toBe("open /Users/«user»/DATA/x failed");
  });

  it("leaves clean text untouched", () => {
    expect(scrubSecrets("connection refused at 127.0.0.1")).toBe("connection refused at 127.0.0.1");
  });
});

describe("pushCapped", () => {
  it("prepends newest-first and never mutates the input", () => {
    const a = [1, 2];
    const b = pushCapped(a, 3, 10);
    expect(b).toEqual([3, 1, 2]);
    expect(a).toEqual([1, 2]); // untouched
  });

  it("drops the oldest beyond the cap", () => {
    let list: number[] = [];
    for (let i = 1; i <= DIAG_CAP + 5; i++) list = pushCapped(list, i, DIAG_CAP);
    expect(list.length).toBe(DIAG_CAP);
    expect(list[0]).toBe(DIAG_CAP + 5); // newest
    expect(list[list.length - 1]).toBe(6); // 1..5 dropped
  });
});

describe("toSupportBundle", () => {
  it("serializes events with a count into pretty JSON", () => {
    const events: DiagEvent[] = [
      { id: 2, ts: 1000, op: "refreshProjects", kind: "disconnected", message: "unavailable", detail: null },
    ];
    const bundle = toSupportBundle(events);
    expect(JSON.parse(bundle)).toMatchObject({ tool: "builder-pro-ai", count: 1 });
    expect(bundle).toContain("refreshProjects");
  });
});
