/**
 * Honest minimal markdown block parser for the Docs preview (SCN-054 step 3: "switches a doc
 * between edit and rendered-preview modes"). The repo deliberately carries NO markdown-renderer
 * dependency — this small pure function covers exactly the block constructs SCN-054's preview
 * promises (headings, lists, paragraphs, fenced code) and nothing more. Inline markup (`**`,
 * `_`, links, …) is intentionally passed through verbatim: rendering a subset of inline syntax
 * would silently mangle the rest, and showing the literal source is the honest degradation
 * (design-system.md §1 "Honest state, always"). `DocsPanel.tsx` maps the returned blocks to
 * plain JSX — no HTML strings and no raw-HTML sink of any kind (the smoke test's injection-sink
 * guard stays clean), so doc content can never inject markup.
 *
 * Parsing rules (line-oriented, one pass):
 * - ``` opens a fenced code block; everything up to the closing ``` is code verbatim. An
 *   UNCLOSED fence swallows the rest of the input as code — matching how the author sees the
 *   document mid-edit, rather than guessing at recovery.
 * - `#`–`######` + space ⇒ a heading of that level.
 * - `- ` / `* ` / `+ ` ⇒ an unordered list item; `N. ` ⇒ an ordered one. Consecutive items of
 *   the same kind coalesce into one list block.
 * - A blank line closes the current paragraph/list; consecutive plain lines join into one
 *   paragraph with single spaces (markdown soft-wrap semantics).
 */

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
