/**
 * Terminal file-link resolver (spec §6.5/D9). PURE and STORE-FREE by design — this module never
 * imports `../store/store` or any Tauri/xterm API, so it is unit-testable with no jsdom/xterm
 * mocking at all (see `link-provider.test.ts`). `terminal-manager.ts` is the only caller: it
 * feeds this module a raw terminal buffer line + the session's live cwd (OSC-7 tracked,
 * `SessionMeta.cwd`) + that session's workspace's `roots`, and maps the returned
 * `ResolvedFileLink[]` onto xterm `ILink[]` (1-based `IBufferRange` columns, see below).
 *
 * This is a LEXICAL pre-filter only (spec §6.5: "candidate must lexically fall under one of the
 * active workspace's roots ... authoritative check happens in the core command on click"). A
 * token that looks like a path but doesn't actually exist still linkifies here; the click path
 * (`FilePreview`'s `readFilePreview`) is what surfaces the honest "not found" toast. This module
 * does zero filesystem I/O.
 *
 * ## What v1 does NOT linkify (deliberate, not accidental — see `link-provider.test.ts`'s
 * "KNOWN true/false table" for the executable spec):
 * - A bare word with no `/` at all (`hello`) — never file-ish enough on its own.
 * - A relative token with no leading `./`/`../` AND no file extension (`src/components`, or a
 *   prose fraction like `3/4`) — the same shape as an inline `numerator/denominator`, and
 *   distinguishing "directory reference" from "prose" without an extension or explicit dot-slash
 *   prefix is not attempted in v1 (spec: "be conservative").
 * - `~/…` (home-relative) tokens: THIS RESOLVER IS STORE-FREE AND HAS NO ACCESS TO `$HOME`, so a
 *   `~/`-prefixed token is detected (so it is never silently misread as the absolute path
 *   `/x/y`, dropping the `~`) and then explicitly skipped rather than guessed at. Threading a
 *   home directory in would require either a Tauri API call (breaking store-freedom) or trusting
 *   a value the terminal itself never confirms; deferred until a real need appears.
 */

/** One lexically-resolved file link inside a single terminal line. */
export interface ResolvedFileLink {
  /** 1-based column of the token's first character (inclusive) — matches xterm's
   * `IBufferCellPosition.x` convention directly, no further arithmetic needed by the caller. */
  startCol: number;
  /**
   * 1-based column ONE PAST the token's last character (EXCLUSIVE) — mirrors
   * `IBufferLine.translateToString`'s documented "endColumn ... exclusive" convention, so
   * `line.slice(startCol - 1, endCol - 1)` is exactly the matched token text. UNLIKE `startCol`,
   * this is NOT handed to `IBufferRange` unchanged: xterm's `IBufferRange.end.x` is 1-based
   * INCLUSIVE (its internal link-lookup/removal code loops `x <= range.end.x`), so the caller
   * (`terminal-manager.ts`'s `wireFileLinks`) must convert with `{ x: endCol - 1, y }` — passing
   * `endCol` straight through would make the link's hit-box extend one column past the token.
   * Spans the WHOLE token including a stripped `:line[:col]` suffix, if present — the owner
   * clicked on `path:42`, not just `path`, so the whole idiom underlines/activates together.
   */
  endCol: number;
  /** The matching workspace root, returned VERBATIM from the `roots` array passed in (not
   * re-normalized) so it compares `===` with `Workspace.roots` entries used elsewhere. */
  root: string;
  /** Path relative to `root`, forward-slash, matching `FsEntry.relPath`'s convention. Empty
   * string only in the (unusual) case a token resolves to the root directory itself. */
  rel: string;
}

/**
 * Path-like token patterns (spec §6.5, refined against the KNOWN true/false table in
 * `link-provider.test.ts`), in priority order:
 *   1. `~/a` or `~/a/b`      — home-relative (detected, then explicitly SKIPPED — see module doc)
 *   2. `/a/b`                — absolute, at least two segments after the leading `/`
 *   3. `./a` or `../a`       — dot-relative (the tail char class allows repeated `../` chains)
 *   4. `a/b.ext`              — bare relative, multi-segment, MUST end in a `.extension` (this is
 *                              what keeps a prose fraction like `3/4` from linkifying — no
 *                              extension, no match)
 * followed by an optional `:line[:col]` suffix, captured OUTSIDE group 1 so it is stripped from
 * the resolved path but still included in the overall token span (see `endCol` doc above).
 *
 * A fresh `RegExp` is constructed per `findFileLinks` call (rather than reusing one module-level
 * `g`-flagged instance) so `lastIndex` state can never leak across calls/lines.
 */
const TOKEN_PATTERN =
  "(~/[\\w.@+-]+(?:/[\\w.@+-]+)*|/[\\w.@+-]+(?:/[\\w.@+-]+)+|\\.{1,2}/[\\w.@+-][\\w./@+-]*|[\\w.@+-]+(?:/[\\w.@+-]+)+\\.\\w+)(?::\\d+(?::\\d+)?)?";

/** Strip a single trailing slash from `root` (roots are documented as already-canonical, spec
 * `types.ts`: "Ordered, equal roots; canonical absolute paths" — this is defense in depth, not a
 * normalization this module relies on). Never touches the root path `"/"` itself. */
function stripTrailingSlash(root: string): string {
  return root !== "/" && root.endsWith("/") ? root.slice(0, -1) : root;
}

/**
 * Collapse `.`/`..` segments and duplicate slashes, POSIX-style, WITHOUT Node's `path` module —
 * this code runs in the Tauri webview (a browser context), not Node, and no other frontend module
 * in this codebase depends on `node:path` (confirmed: `FileTree.tsx`/`ipc/fs.ts` hand-roll their
 * own `dirnameOf`/join helpers for the same reason).
 *
 * A `..` that would climb above an already-empty ABSOLUTE stack is dropped (POSIX: `/` is its own
 * parent, `cd /..` stays at `/`) rather than kept, matching real filesystem semantics — this is
 * what makes the "escapes the root via `..`, ends up outside every root" rejection test correct
 * rather than accidentally correct.
 */
function normalizePosix(p: string): string {
  const isAbsolute = p.startsWith("/");
  const segments = p.split("/").filter((seg) => seg !== "" && seg !== ".");
  const out: string[] = [];
  for (const seg of segments) {
    if (seg === "..") {
      if (out.length > 0 && out[out.length - 1] !== "..") {
        out.pop();
      } else if (!isAbsolute) {
        out.push("..");
      }
      // else: absolute and already at/above root -> drop (POSIX clamps at "/")
    } else {
      out.push(seg);
    }
  }
  return (isAbsolute ? "/" : "") + out.join("/");
}

/** Join `base` (assumed already-normalized/absolute) with a relative tail, POSIX-style. */
function posixJoin(base: string, rel: string): string {
  if (rel === "") return base;
  return `${stripTrailingSlash(base)}/${rel}`;
}

/**
 * Match an already-resolved, normalized absolute path against `roots`; returns the FIRST
 * containing root (verbatim, as passed in) plus the path's slash-relative `rel`, or `null` if the
 * path lexically falls outside every root. Exported because it is ALSO the OSC-8 `file://`
 * handler's containment check in `terminal-manager.ts` — that handler starts from an
 * already-absolute URL path (no cwd/relative resolution involved), so it calls this directly
 * rather than going through the regex-driven `findFileLinks`.
 *
 * `root === path` resolves to `rel === ""`; otherwise a `root` is only a match when `path` starts
 * with `` `${root}/` `` (a plain string-prefix check would wrongly let `/repo2/x` match root
 * `/repo` — guarded against explicitly, see the test of the same name).
 */
export function matchWorkspaceRoot(
  absPath: string,
  roots: string[],
): { root: string; rel: string } | null {
  const resolved = normalizePosix(absPath);
  for (const root of roots) {
    const normalizedRoot = stripTrailingSlash(root);
    if (resolved === normalizedRoot) return { root, rel: "" };
    const prefix = normalizedRoot === "/" ? "/" : `${normalizedRoot}/`;
    if (resolved.startsWith(prefix)) return { root, rel: resolved.slice(prefix.length) };
  }
  return null;
}

/**
 * Find every file-like link in one raw terminal line (spec §6.5). See the module doc for what is
 * deliberately NOT linkified. `cwd` and every entry of `roots` are expected to already be
 * absolute, canonical paths (the store's contract — `SessionMeta.cwd` / `Workspace.roots`); this
 * function still normalizes both defensively before comparing.
 */
export function findFileLinks(line: string, cwd: string, roots: string[]): ResolvedFileLink[] {
  const links: ResolvedFileLink[] = [];
  if (roots.length === 0) return links;

  const normalizedCwd = normalizePosix(cwd);
  const regex = new RegExp(TOKEN_PATTERN, "g");
  let m: RegExpExecArray | null;
  while ((m = regex.exec(line)) !== null) {
    const pathToken = m[1];

    // Home-relative: detected so it is never silently reinterpreted as an absolute /x/y path
    // (dropping the leading "~"), then explicitly skipped — see module doc.
    if (pathToken.startsWith("~/")) continue;

    const absolute = pathToken.startsWith("/")
      ? pathToken
      : posixJoin(normalizedCwd, pathToken);

    const match = matchWorkspaceRoot(absolute, roots);
    if (!match) continue;

    links.push({
      startCol: m.index + 1,
      endCol: m.index + m[0].length + 1,
      root: match.root,
      rel: match.rel,
    });
  }
  return links;
}
