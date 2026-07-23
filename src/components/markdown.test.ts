// @vitest-environment jsdom
// SCN-054: unit floor for the Docs preview's minimal markdown parser — every BLOCK construct the
// preview promises (headings, lists, paragraphs, code fences) plus the honest edge cases (unclosed
// fence, empty input), and (PRN-14) the INLINE tokenizer `renderInline` (bold/em/code/link, plus
// tolerant fallback and the no-raw-HTML guarantee).
import { describe, it, expect, afterEach } from "vitest";
import { render, cleanup } from "@testing-library/react";
import { createElement } from "react";
import { parseMarkdown, renderInline } from "./markdown";

afterEach(cleanup);

/** Mount `renderInline`'s node array under a container so assertions can read the produced DOM
 * (proving JSX elements, not raw source or HTML strings). */
function renderNodes(text: string): HTMLElement {
  return render(createElement("div", null, ...renderInline(text))).container;
}

describe("parseMarkdown", () => {
  it("returns no blocks for empty and whitespace-only input", () => {
    expect(parseMarkdown("")).toEqual([]);
    expect(parseMarkdown("\n  \n\n")).toEqual([]);
  });

  it("parses headings at every level 1-6", () => {
    const blocks = parseMarkdown("# One\n###### Six\n");
    expect(blocks).toEqual([
      { kind: "heading", level: 1, text: "One" },
      { kind: "heading", level: 6, text: "Six" },
    ]);
  });

  it("a #-run without a following space is a paragraph, not a heading", () => {
    expect(parseMarkdown("#not-a-heading")).toEqual([
      { kind: "paragraph", text: "#not-a-heading" },
    ]);
  });

  it("joins consecutive plain lines into one paragraph and splits on blank lines", () => {
    const blocks = parseMarkdown("first line\nsecond line\n\nnext paragraph\n");
    expect(blocks).toEqual([
      { kind: "paragraph", text: "first line second line" },
      { kind: "paragraph", text: "next paragraph" },
    ]);
  });

  it("coalesces unordered items (-, *, +) into one list block", () => {
    const blocks = parseMarkdown("- a\n* b\n+ c\n");
    expect(blocks).toEqual([{ kind: "list", ordered: false, items: ["a", "b", "c"] }]);
  });

  it("parses ordered lists and keeps them separate from unordered ones", () => {
    const blocks = parseMarkdown("1. first\n2. second\n- bullet\n");
    expect(blocks).toEqual([
      { kind: "list", ordered: true, items: ["first", "second"] },
      { kind: "list", ordered: false, items: ["bullet"] },
    ]);
  });

  it("captures a fenced code block verbatim, without rendering its markdown", () => {
    const blocks = parseMarkdown("```\n# not a heading\n- not a list\n```\nafter\n");
    expect(blocks).toEqual([
      { kind: "code", text: "# not a heading\n- not a list" },
      { kind: "paragraph", text: "after" },
    ]);
  });

  it("an unclosed fence swallows the rest of the input as code (honest mid-edit state)", () => {
    const blocks = parseMarkdown("before\n```\nstill code\nmore code");
    expect(blocks).toEqual([
      { kind: "paragraph", text: "before" },
      { kind: "code", text: "still code\nmore code" },
    ]);
  });

  it("passes inline markup through verbatim (no half-rendered subset)", () => {
    expect(parseMarkdown("uses **bold** and `code`")).toEqual([
      { kind: "paragraph", text: "uses **bold** and `code`" },
    ]);
  });

  it("parses a mixed document in order", () => {
    const md = "# Title\n\nintro text\n\n- one\n- two\n\n```\nlet x = 1;\n```\n\n## Sub\ntail\n";
    expect(parseMarkdown(md)).toEqual([
      { kind: "heading", level: 1, text: "Title" },
      { kind: "paragraph", text: "intro text" },
      { kind: "list", ordered: false, items: ["one", "two"] },
      { kind: "code", text: "let x = 1;" },
      { kind: "heading", level: 2, text: "Sub" },
      { kind: "paragraph", text: "tail" },
    ]);
  });

  it("a plain line after a list closes the list into its own block", () => {
    expect(parseMarkdown("- item\nplain\n")).toEqual([
      { kind: "list", ordered: false, items: ["item"] },
      { kind: "paragraph", text: "plain" },
    ]);
  });
});

describe("renderInline (PRN-14)", () => {
  it("renders **bold** as a <strong>, dropping the ** markers", () => {
    const el = renderNodes("say **hi** now");
    const strong = el.querySelector("strong");
    expect(strong?.textContent).toBe("hi");
    expect(el.textContent).toBe("say hi now");
    expect(el.textContent).not.toContain("*");
  });

  it("renders *em* and _em_ as an <em>, dropping the markers", () => {
    const star = renderNodes("an *italic* word");
    expect(star.querySelector("em")?.textContent).toBe("italic");
    expect(star.textContent).toBe("an italic word");
    expect(star.textContent).not.toContain("*");

    const under = renderNodes("an _italic_ word");
    expect(under.querySelector("em")?.textContent).toBe("italic");
    expect(under.textContent).toBe("an italic word");
    expect(under.textContent).not.toContain("_");
  });

  it("renders `code` as a <code> element, dropping the backticks", () => {
    const el = renderNodes("run `npm test` here");
    const code = el.querySelector("code");
    expect(code?.textContent).toBe("npm test");
    expect(el.textContent).toBe("run npm test here");
    expect(el.textContent).not.toContain("`");
  });

  it("renders [text](url) as a NON-navigating element (no href) with the url in the title", () => {
    const el = renderNodes("see [docs](https://example.com/x) please");
    // No navigable anchor is ever emitted (a real <a href> would move the Tauri webview).
    expect(el.querySelector("a")).toBeNull();
    expect(el.querySelector("[href]")).toBeNull();
    const link = el.querySelector("[title]") as HTMLElement | null;
    expect(link?.getAttribute("title")).toBe("https://example.com/x");
    expect(link?.textContent).toBe("docs");
    expect(el.textContent).toBe("see docs please");
    expect(el.textContent).not.toContain("]("); // the link syntax is gone
  });

  it("leaves an unbalanced ** as literal text (tolerant, no swallowing)", () => {
    const el = renderNodes("a **b and c");
    expect(el.querySelector("strong")).toBeNull();
    expect(el.textContent).toBe("a **b and c");
  });

  it("leaves lone / space-flanked marks as literal text (flanking + tolerant)", () => {
    expect(renderNodes("a ` b").textContent).toBe("a ` b");
    // Whitespace just inside the marks ⇒ not emphasis (GFM-style flanking).
    const spaced = renderNodes("2 * 3 * 4");
    expect(spaced.querySelector("em")).toBeNull();
    expect(spaced.textContent).toBe("2 * 3 * 4");
    // Underscore inside a word ⇒ literal.
    const under = renderNodes("path a_b_c stays literal");
    expect(under.querySelector("em")).toBeNull();
    expect(under.textContent).toBe("path a_b_c stays literal");
  });

  it("does not honor _ inside a word (GFM intraword rule) so snake_case survives", () => {
    const el = renderNodes("call do_the_thing() now");
    expect(el.querySelector("em")).toBeNull();
    expect(el.textContent).toBe("call do_the_thing() now");
  });

  it("never emits raw HTML — a literal <b> tag in source is escaped, not an element", () => {
    const el = renderNodes("danger <b>x</b> and **real** bold");
    // The literal tag is text, not a <b> element; only our own <strong> exists.
    expect(el.querySelector("b")).toBeNull();
    expect(el.querySelector("strong")?.textContent).toBe("real");
    expect(el.textContent).toBe("danger <b>x</b> and real bold");
  });

  it("returns a plain-text string array unchanged when there is no inline markup", () => {
    expect(renderInline("just words")).toEqual(["just words"]);
  });
});
