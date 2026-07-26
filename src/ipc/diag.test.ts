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
    // FE-2: a bare string IS the human message (shown verbatim by `reportError`), so it classifies
    // as "message" with no separate detail — not as an unclassifiable "unknown" throw.
    expect(classifyError("nope")).toEqual({ kind: "message", detail: null });
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

  it("redacts known credential prefixes", () => {
    expect(scrubSecrets("lin_api_AAbb11")).toBe("«redacted-key»");
    expect(scrubSecrets("sk-abcdefghijklmnop")).toBe("«redacted-key»");
  });

  it("redacts the apple app-specific password shape ONLY with apple context nearby (REL-4)", () => {
    // REL-4: the bare xxxx-xxxx-xxxx-xxxx shape collided with ordinary hyphenated English, so it
    // now redacts only when `apple` / `app-specific` sits nearby (either side of the password).
    expect(scrubSecrets("this-word-four-times")).toBe("this-word-four-times");
    expect(scrubSecrets("bcvp-zaww-phyp-ohwi")).toBe("bcvp-zaww-phyp-ohwi");
    expect(scrubSecrets("use bcvp-zaww-phyp-ohwi for your Apple ID")).toBe(
      "use «redacted-pw» for your Apple ID",
    );
    expect(scrubSecrets("app-specific password bcvp-zaww-phyp-ohwi")).toBe(
      "app-specific password «redacted-pw»",
    );
  });

  it("collapses the home-dir username", () => {
    expect(scrubSecrets("open /Users/alice/DATA/x failed")).toBe("open /Users/«user»/DATA/x failed");
  });

  it("leaves clean text untouched", () => {
    expect(scrubSecrets("connection refused at 127.0.0.1")).toBe("connection refused at 127.0.0.1");
  });
});

// REL-4: the probe corpus flipped into a real test — every row must come out REDACTED now (the
// "NOT caught" rows were the finding: JSON-quoted keys, bare JWTs, vendor prefixes, URL userinfo,
// Cookie headers, PEM material, multi-word quoted values).
describe("scrubSecrets REL-4 differential corpus", () => {
  const REDACTED_MARKERS = ["«redacted»", "«redacted-key»", "«redacted-pw»", "/Users/«user»"];
  const isRedacted = (s: string) => REDACTED_MARKERS.some((m) => s.includes(m));

  const cases: Array<{ name: string; input: string; expectRedacted: boolean }> = [
    { name: "Bearer token", input: "Authorization: Bearer abc.def-ghi_123", expectRedacted: true },
    { name: "unquoted key:value", input: "access_token: abc123", expectRedacted: true },
    { name: "unquoted key=value", input: "token=abc123xyz", expectRedacted: true },
    { name: "sk- prefixed", input: "key sk-abcdefghijklmnop leaked", expectRedacted: true },
    { name: "ghp_ PAT (20+ chars)", input: "ghp_abcdefghijklmnopqrstuvwxyz", expectRedacted: true },
    { name: "home path", input: "open /Users/alice/x failed", expectRedacted: true },
    { name: "JSON-quoted key", input: `"access_token": "abc123"`, expectRedacted: true },
    { name: "bare JWT", input: "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.abc", expectRedacted: true },
    { name: "GitHub OAuth gho_", input: "gho_abcdefghijklmnopqrstuvwxyz", expectRedacted: true },
    { name: "GitHub user-to-server ghu_", input: "ghu_abcdefghijklmnopqrstuvwxyz", expectRedacted: true },
    { name: "GitHub server-to-server ghs_", input: "ghs_abcdefghijklmnopqrstuvwxyz", expectRedacted: true },
    { name: "GitHub refresh ghr_", input: "ghr_abcdefghijklmnopqrstuvwxyz", expectRedacted: true },
    {
      name: "GitHub fine-grained github_pat_",
      input: "github_pat_11ABCDEFGHIJKLMNOPQRST",
      expectRedacted: true,
    },
    { name: "GitLab PAT glpat-", input: "glpat-xyzabcdef123", expectRedacted: true },
    { name: "AWS access key AKIA", input: "AKIAIOSFODNN7EXAMPLE", expectRedacted: true },
    { name: "Google API key AIza", input: "AIza" + "A".repeat(35), expectRedacted: true },
    { name: "npm token", input: "npm_abcdefghijklmnopqrstuvwxyz0123456789", expectRedacted: true },
    { name: "PyPI token", input: "pypi-AgEIcHlwaS5vcmcjEWJkYzA1", expectRedacted: true },
    {
      name: "Slack webhook URL",
      input: "https://hooks.slack.com/services/T00000000/B00000000/XXXXXXXXXXXXXXXXXXXXXXXX",
      expectRedacted: true,
    },
    { name: "URL userinfo", input: "https://user:pass@host/repo", expectRedacted: true },
    { name: "Cookie header", input: "Cookie: session=abc123", expectRedacted: true },
    { name: "PEM private key header", input: "-----BEGIN PRIVATE KEY-----", expectRedacted: true },
    {
      name: "PEM private key block (multiline)",
      input: "-----BEGIN PRIVATE KEY-----\nMIIEvQIBADANBgkqhkiG9w0BAQEFAASC\n-----END PRIVATE KEY-----",
      expectRedacted: true,
    },
    { name: "password with space", input: `password: "two words"`, expectRedacted: true },
    { name: "single-word quoted password", input: `password: "oneword"`, expectRedacted: true },
  ];

  for (const c of cases) {
    it(`${c.name}: REDACTED`, () => {
      expect(isRedacted(scrubSecrets(c.input))).toBe(c.expectRedacted);
    });
  }

  it("does not over-redact ordinary prose (REL-4 false-positive guards)", () => {
    expect(scrubSecrets("this-word-four-times")).toBe("this-word-four-times");
    expect(scrubSecrets("visit https://example.com/docs for details")).toBe(
      "visit https://example.com/docs for details",
    );
    expect(scrubSecrets("cookie: recipes for beginners")).toBe("cookie: recipes for beginners");
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
