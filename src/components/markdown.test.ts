// SCN-054: unit floor for the Docs preview's minimal markdown block parser — every construct the
// preview promises (headings, lists, paragraphs, code fences) plus the honest edge cases
// (unclosed fence, empty input, inline markup passed through verbatim).
import { describe, it, expect } from "vitest";
import { parseMarkdown } from "./markdown";

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
