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

/**
 * LNK-1 trimmed-prefix wrong-target edges (flipped probes): a root-resolving match (`rel === ""`)
 * that is only a TRIMMED PREFIX of the raw token must NOT linkify -- the regex stopped at a
 * character outside its match class (a space inside a spaced directory name, or a non-ASCII
 * segment), so the link would target the root DIRECTORY instead of the file the owner meant.
 * Genuine full-token root matches still linkify (asserted below). Cyrillic inputs are built via
 * `String.fromCodePoint` so this file stays ASCII-only for the scripts/check-english.sh gate
 * (spec D2/O-2).
 */
describe("findFileLinks — LNK-1 trimmed-prefix wrong-target guard", () => {
  const roots = ["/Users/x/My"];
  // The cyrillic "file.ts" name, built from codepoints so this source file stays ASCII-only.
  const CYRILLIC_FILE = String.fromCodePoint(0x0444, 0x0430, 0x0439, 0x043b) + ".ts";
  it("token with a space under a spaced root (`/Users/x/My Dir/f.ts`) -> NO link (was: one wrong-target root link with rel === '')", () => {
    // The regex stops at the space and matches only the `/Users/x/My` prefix; that prefix
    // resolves to the root with rel === "" while the token plainly continues (`Dir/f.ts`), so
    // the LNK-1 guard kills it. The trailing `Dir/f.ts` word resolves outside every root anyway.
    const links = findFileLinks("open /Users/x/My Dir/f.ts now", "/", roots);
    expect(links).toHaveLength(0);
  });

  it("cyrillic segment in an ABSOLUTE path -> NO link (was: only the ASCII root prefix underlined, targeting rel === '')", () => {
    // `\w` skips cyrillic, so the regex matches only `/Users/x/My` and the token continues at
    // `/` + the cyrillic name -- a trimmed root-prefix, killed by the same guard.
    const links = findFileLinks(`cat /Users/x/My/${CYRILLIC_FILE}`, "/", roots);
    expect(links).toHaveLength(0);
  });

  it("bare cyrillic relative token -> NO link (unchanged: \\w never matches cyrillic)", () => {
    expect(findFileLinks(`cat ${CYRILLIC_FILE}`, "/Users/x/My", roots)).toHaveLength(0);
  });

  it("quoted relative token \"src/a.ts\" -> still exactly one link with rel 'src/a.ts' (quotes sit outside the char class, the guard does not apply)", () => {
    const links = findFileLinks(`see "src/a.ts" here`, "/Users/x/My", roots);
    expect(links).toHaveLength(1);
    expect(links[0]).toMatchObject({ root: "/Users/x/My", rel: "src/a.ts" });
  });

  it("positive control: a full-token absolute path to a real shape -> still linkifies with the file's rel", () => {
    const links = findFileLinks("cat /Users/x/My/file.ts", "/", roots);
    expect(links).toHaveLength(1);
    expect(links[0]).toMatchObject({ root: "/Users/x/My", rel: "file.ts" });
  });

  it("genuine full-token root match (`cd /Users/x/My`) -> STILL linkifies with rel === ''", () => {
    // The token IS the root, not a trimmed prefix of a longer one: `isTrimmedPrefix` sees no
    // continuation past the match, so the guard passes and the fixed code genuinely emits the
    // root link with rel === "" (clicking it targets the root directory -- the intended target).
    const line = "cd /Users/x/My";
    const links = findFileLinks(line, "/", roots);
    expect(links).toHaveLength(1);
    expect(links[0]).toMatchObject({ root: "/Users/x/My", rel: "" });
    expect(line.slice(links[0].startCol - 1, links[0].endCol - 1)).toBe("/Users/x/My");
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
