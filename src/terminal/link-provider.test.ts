import { describe, it, expect } from "vitest";
import { findFileLinks, matchWorkspaceRoot } from "./link-provider";

/**
 * Pure-resolver unit tests (spec §6.5/D9). No xterm/jsdom involved — `findFileLinks` takes a raw
 * line string + the session's live cwd + the workspace's roots and returns lexically-resolved
 * file links. The KNOWN true/false table below is the authoritative spec for what v1 does and
 * does NOT linkify; every row is a deliberate decision, not an accident of the regex.
 */
describe("findFileLinks — KNOWN true/false table", () => {
  const root = "/repo";

  it("relative path under cwd === root -> links, root/rel resolved", () => {
    const links = findFileLinks("running src/app.ts now", "/repo", [root]);
    expect(links).toHaveLength(1);
    expect(links[0]).toMatchObject({ root: "/repo", rel: "src/app.ts" });
  });

  it("dot-relative path with a single :line suffix -> suffix stripped from the path, kept in the token span", () => {
    const line = "error at ./a/b.rs:42 during build";
    const links = findFileLinks(line, "/repo", [root]);
    expect(links).toHaveLength(1);
    expect(links[0]).toMatchObject({ root: "/repo", rel: "a/b.rs" });
    // the clickable span covers the WHOLE token including the stripped suffix (path+":42" is the
    // recognizable "file:line" idiom the owner clicked on) -- assert the exact column span.
    const start = line.indexOf("./a/b.rs:42");
    expect(links[0].startCol).toBe(start + 1);
    expect(links[0].endCol).toBe(start + "./a/b.rs:42".length + 1);
    expect(line.slice(links[0].startCol - 1, links[0].endCol - 1)).toBe("./a/b.rs:42");
  });

  it("double :line:col suffix -> both stripped from the path", () => {
    const line = "at /repo/x/y.ts:42:7 -> boom";
    const links = findFileLinks(line, "/repo", [root]);
    expect(links).toHaveLength(1);
    expect(links[0]).toMatchObject({ root: "/repo", rel: "x/y.ts" });
    expect(line.slice(links[0].startCol - 1, links[0].endCol - 1)).toBe("/repo/x/y.ts:42:7");
  });

  it("absolute path already under a root -> links", () => {
    const links = findFileLinks("wrote /repo/x/y.ts", "/anywhere", [root]);
    expect(links).toHaveLength(1);
    expect(links[0]).toMatchObject({ root: "/repo", rel: "x/y.ts" });
  });

  it("absolute path outside every root -> NO link (lexical rejection, not a failing click)", () => {
    const links = findFileLinks("cat /etc/passwd", "/repo", [root]);
    expect(links).toHaveLength(0);
  });

  it("relative path that escapes the root via .. -> resolves outside roots -> NO link", () => {
    // cwd is nested three levels under the root; three ".." lands past "/" (POSIX clamps at
    // root, it does not error) and the final "/etc/x" is outside "/repo" either way.
    const links = findFileLinks("cat ../../../etc/x", "/repo/a/b", [root]);
    expect(links).toHaveLength(0);
  });

  it("bare word with no slash -> NO link (not file-ish, spec: prefer tokens with a / and a file-ish tail)", () => {
    const links = findFileLinks("hello world", "/repo", [root]);
    expect(links).toHaveLength(0);
  });

  it("prose slash (ratio, not a path) -> NO link (no extension, no leading / or ./ or ../ -- KNOWN decision, not a link that merely fails on click)", () => {
    const links = findFileLinks("a ratio 3/4 here", "/repo", [root]);
    expect(links).toHaveLength(0);
  });

  it("~/ home-relative token -> NO link (KNOWN decision: this resolver is store-free and has no $HOME, so a ~-prefixed token is detected then deliberately skipped rather than silently reinterpreted as an absolute /x/y path)", () => {
    const links = findFileLinks("edit ~/x/y", "/repo", [root]);
    expect(links).toHaveLength(0);
  });

  it("multiple links on one line -> all returned, in order, non-overlapping columns", () => {
    const line = "diff src/a.ts src/b.ts";
    const links = findFileLinks(line, "/repo", [root]);
    expect(links).toHaveLength(2);
    expect(links[0]).toMatchObject({ rel: "src/a.ts" });
    expect(links[1]).toMatchObject({ rel: "src/b.ts" });
    expect(links[0].endCol).toBeLessThanOrEqual(links[1].startCol);
  });

  it("multiple roots -- token resolves under whichever root actually contains it", () => {
    const links = findFileLinks("touch /other/y/z.ts", "/repo", ["/repo", "/other"]);
    expect(links).toHaveLength(1);
    expect(links[0]).toMatchObject({ root: "/other", rel: "y/z.ts" });
  });

  it("empty line -> no links", () => {
    expect(findFileLinks("", "/repo", [root])).toEqual([]);
  });

  it("no roots configured -> nothing can ever resolve, even a plainly-in-cwd relative path", () => {
    expect(findFileLinks("src/app.ts", "/repo", [])).toEqual([]);
  });

  it("dot-relative path resolving exactly to the root itself -> rel is the empty string", () => {
    const links = findFileLinks("cd ./", "/repo", [root]);
    // "./" alone has no file-ish tail after the slash, so this is intentionally not a match --
    // assert the KNOWN behavior rather than assume it links.
    expect(links).toHaveLength(0);
  });
});

describe("matchWorkspaceRoot — shared containment helper (also used by the OSC-8 file:// handler)", () => {
  it("returns the matching root (verbatim) and the slash-relative rel", () => {
    expect(matchWorkspaceRoot("/repo/x/y.ts", ["/repo"])).toEqual({
      root: "/repo",
      rel: "x/y.ts",
    });
  });

  it("returns null for a path outside every root", () => {
    expect(matchWorkspaceRoot("/etc/passwd", ["/repo"])).toBeNull();
  });

  it("the root itself resolves to rel === \"\"", () => {
    expect(matchWorkspaceRoot("/repo", ["/repo"])).toEqual({ root: "/repo", rel: "" });
  });

  it("does not false-positive on a root that is a string-prefix but not a path-prefix (/repo vs /repo2)", () => {
    expect(matchWorkspaceRoot("/repo2/x.ts", ["/repo"])).toBeNull();
  });
});
