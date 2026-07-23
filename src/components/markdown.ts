/**
 * Honest minimal markdown parser for the Docs preview (SCN-054 step 3: "switches a doc between
 * edit and rendered-preview modes"). The repo deliberately carries NO markdown-renderer
 * dependency — this small pure module covers exactly the block constructs SCN-054's preview
 * promises (headings, lists, paragraphs, fenced code) plus the four inline marks (`**bold**`,
 * `*em*`/`_em_`, `` `code` ``, `[text](url)`). `parseMarkdown` splits BLOCK structure; the block
 * `text` stays the raw source string, and `renderInline` (below) turns that string into an array
 * of React nodes at render time — `DocsPanel.tsx` maps blocks to plain JSX and feeds paragraph /
 * heading / list-item text through `renderInline`. Everything is JSX only: no HTML strings and no
 * raw-HTML sink of any kind (the smoke test's injection-sink guard stays clean, and any literal
 * `<tag>` in doc content is escaped by React), so doc content can never inject markup.
 *
 * Block parsing rules (line-oriented, one pass):
 * - ``` opens a fenced code block; everything up to the closing ``` is code verbatim (and is NOT
 *   run through `renderInline`). An UNCLOSED fence swallows the rest of the input as code —
 *   matching how the author sees the document mid-edit, rather than guessing at recovery.
 * - `#`–`######` + space ⇒ a heading of that level.
 * - `- ` / `* ` / `+ ` ⇒ an unordered list item; `N. ` ⇒ an ordered one. Consecutive items of
 *   the same kind coalesce into one list block.
 * - A blank line closes the current paragraph/list; consecutive plain lines join into one
 *   paragraph with single spaces (markdown soft-wrap semantics).
 */

import { createElement, type CSSProperties, type ReactNode } from "react";

export type MdBlock =
  | { kind: "heading"; level: 1 | 2 | 3 | 4 | 5 | 6; text: string }
  | { kind: "paragraph"; text: string }
  | { kind: "list"; ordered: boolean; items: string[] }
  | { kind: "code"; text: string };

const HEADING_RE = /^(#{1,6})\s+(.*)$/;
const UNORDERED_ITEM_RE = /^[-*+]\s+(.*)$/;
const ORDERED_ITEM_RE = /^\d+\.\s+(.*)$/;

export function parseMarkdown(md: string): MdBlock[] {
  const blocks: MdBlock[] = [];
  /** Plain lines accumulated toward the current paragraph. */
  let paragraph: string[] = [];
  /** Items accumulated toward the current list block, or `null` when no list is open. */
  let list: { ordered: boolean; items: string[] } | null = null;

  function flushParagraph(): void {
    if (paragraph.length > 0) {
      blocks.push({ kind: "paragraph", text: paragraph.join(" ") });
      paragraph = [];
    }
  }

  function flushList(): void {
    if (list !== null) {
      blocks.push({ kind: "list", ordered: list.ordered, items: list.items });
      list = null;
    }
  }

  const lines = md.split("\n");
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];

    if (line.trimEnd().startsWith("```")) {
      flushParagraph();
      flushList();
      const code: string[] = [];
      i += 1;
      while (i < lines.length && !lines[i].trimEnd().startsWith("```")) {
        code.push(lines[i]);
        i += 1;
      }
      i += 1; // skip the closing fence (or run past the end on an unclosed one)
      blocks.push({ kind: "code", text: code.join("\n") });
      continue;
    }

    const heading = HEADING_RE.exec(line);
    if (heading !== null) {
      flushParagraph();
      flushList();
      blocks.push({
        kind: "heading",
        level: heading[1].length as 1 | 2 | 3 | 4 | 5 | 6,
        text: heading[2].trim(),
      });
      i += 1;
      continue;
    }

    const unordered = UNORDERED_ITEM_RE.exec(line);
    const ordered = unordered === null ? ORDERED_ITEM_RE.exec(line) : null;
    if (unordered !== null || ordered !== null) {
      flushParagraph();
      const isOrdered = ordered !== null;
      const item = (unordered ?? ordered)![1].trim();
      if (list === null || list.ordered !== isOrdered) {
        flushList();
        list = { ordered: isOrdered, items: [] };
      }
      list.items.push(item);
      i += 1;
      continue;
    }

    if (line.trim() === "") {
      flushParagraph();
      flushList();
      i += 1;
      continue;
    }

    // A plain line: closes any open list (a list item's continuation lines are out of this
    // renderer's minimal scope) and joins the current paragraph.
    flushList();
    paragraph.push(line.trim());
    i += 1;
  }

  flushParagraph();
  flushList();
  return blocks;
}

/** Inline `<code>` styling — monospace on a subtly raised chip so it reads against the preview's
 * `--panel-2` surface (the block-code register, one step lighter). */
const inlineCodeStyle: CSSProperties = {
  fontFamily: "var(--font-mono)",
  fontSize: "0.9em",
  background: "var(--panel)",
  borderRadius: "var(--r-sm)",
  padding: "0 4px",
};

/** Inline link styling. The preview runs inside a Tauri webview, so a real `<a href>` would
 * navigate the whole app away — links render as a NON-navigating accent-underlined span with the
 * destination in a `title` tooltip instead (honest: the URL is visible, nothing is clickable). */
const inlineLinkStyle: CSSProperties = {
  color: "var(--accent)",
  textDecoration: "underline",
  cursor: "default",
  wordBreak: "break-all",
};

const WORD_CHAR_RE = /[A-Za-z0-9]/;

/** Whitespace (or out-of-bounds "") — used by the emphasis flanking rule: a `*`/`_`/`**` run is
 * only a delimiter when it has no whitespace immediately inside it (so `2 * 3 * 4` and `** x **`
 * stay literal, matching how a reader expects loose asterisks in prose to render). */
function isSpace(c: string): boolean {
  return c === "" || c === " " || c === "\t";
}

/**
 * Turns one text string into an array of React nodes, rendering the four inline marks the Docs
 * preview promises — dependency-free, single pass, and tolerant (any unmatched/unbalanced mark
 * falls through as literal text rather than swallowing the rest of the line):
 *
 * - `` `code` `` → `<code>` (monospace); code wins over every other mark inside it.
 * - `**bold**` → `<strong>`.
 * - `*em*` / `_em_` → `<em>`. Underscore emphasis honors the GFM intraword rule (a `_` touching a
 *   word character on the relevant side is literal), so `snake_case` / `a_b_c` are never mangled.
 * - `[text](url)` → a non-navigating link span (see `inlineLinkStyle`) with `title={url}`.
 *
 * The content of a mark is emitted as a plain string child (no nested inline parsing) — enough for
 * the preview, and it keeps the tokenizer small and predictable. NEVER emits HTML strings; React
 * escapes any literal markup in the source, so this cannot become a raw-HTML sink.
 */
export function renderInline(text: string): ReactNode[] {
  const nodes: ReactNode[] = [];
  let buffer = "";
  let key = 0;

  function flush(): void {
    if (buffer !== "") {
      nodes.push(buffer);
      buffer = "";
    }
  }

  let i = 0;
  const n = text.length;
  while (i < n) {
    const ch = text[i];

    // `code` — highest precedence; content is verbatim, no inner marks.
    if (ch === "`") {
      const close = text.indexOf("`", i + 1);
      if (close > i + 1) {
        flush();
        nodes.push(createElement("code", { key: key++, style: inlineCodeStyle }, text.slice(i + 1, close)));
        i = close + 1;
        continue;
      }
      buffer += ch;
      i += 1;
      continue;
    }

    // **bold** — checked before single-`*` emphasis. Flanking: the run must be non-empty with no
    // whitespace just inside it (`** x **` stays literal).
    if (ch === "*" && text[i + 1] === "*") {
      if (!isSpace(text[i + 2] ?? "")) {
        let from = i + 2;
        let matched = false;
        while (from < n) {
          const close = text.indexOf("**", from);
          if (close < i + 2) break;
          if (close > i + 2 && !isSpace(text[close - 1] ?? "")) {
            flush();
            nodes.push(createElement("strong", { key: key++ }, text.slice(i + 2, close)));
            i = close + 2;
            matched = true;
            break;
          }
          from = close + 2;
        }
        if (matched) continue;
      }
      buffer += "**";
      i += 2;
      continue;
    }

    // *em* / _em_. Flanking (no whitespace just inside the marks) plus, for `_`, the GFM intraword
    // rule (a `_` touching a word character on that side is literal) so `snake_case` / paths survive.
    if (ch === "*" || ch === "_") {
      const validOpen =
        !isSpace(text[i + 1] ?? "") &&
        !(ch === "_" && WORD_CHAR_RE.test(i > 0 ? text[i - 1] : ""));
      if (validOpen) {
        let from = i + 1;
        let matched = false;
        while (from < n) {
          const close = text.indexOf(ch, from);
          if (close <= i) break;
          const validClose =
            close > i + 1 &&
            !isSpace(text[close - 1] ?? "") &&
            !(ch === "_" && WORD_CHAR_RE.test(close + 1 < n ? text[close + 1] : ""));
          if (validClose) {
            flush();
            nodes.push(createElement("em", { key: key++ }, text.slice(i + 1, close)));
            i = close + 1;
            matched = true;
            break;
          }
          from = close + 1;
        }
        if (matched) continue;
      }
      buffer += ch;
      i += 1;
      continue;
    }

    // [text](url) — non-navigating link span.
    if (ch === "[") {
      const closeBracket = text.indexOf("]", i + 1);
      if (closeBracket > i && text[closeBracket + 1] === "(") {
        const closeParen = text.indexOf(")", closeBracket + 2);
        if (closeParen > closeBracket + 1) {
          const linkText = text.slice(i + 1, closeBracket);
          const url = text.slice(closeBracket + 2, closeParen);
          flush();
          nodes.push(
            createElement("span", { key: key++, style: inlineLinkStyle, title: url }, linkText),
          );
          i = closeParen + 1;
          continue;
        }
      }
      buffer += ch;
      i += 1;
      continue;
    }

    buffer += ch;
    i += 1;
  }

  flush();
  return nodes;
}
