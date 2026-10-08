import { describe, expect, it } from "vitest";
import { renderMarkdown } from "../markdown";

describe("renderMarkdown", () => {
  it("renders headings and paragraphs", () => {
    expect(renderMarkdown("# Title\n\nSome *text*.")).toBe(
      "<h1>Title</h1>\n<p>Some <em>text</em>.</p>\n",
    );
  });

  it("renders bullet lists", () => {
    expect(renderMarkdown("- one\n- two")).toBe("<ul>\n<li>one</li>\n<li>two</li>\n</ul>\n");
  });

  it("renders GFM tables", () => {
    expect(renderMarkdown("| a | b |\n| - | - |\n| 1 | 2 |")).toBe(
      "<table>\n<thead>\n<tr>\n<th>a</th>\n<th>b</th>\n</tr>\n</thead>\n<tbody>\n<tr>\n<td>1</td>\n<td>2</td>\n</tr>\n</tbody>\n</table>\n",
    );
  });

  it("escapes raw HTML", () => {
    expect(renderMarkdown("<script>alert(1)</script>")).toBe(
      "<p>&lt;script&gt;alert(1)&lt;/script&gt;</p>\n",
    );
  });

  it("does not render javascript: links", () => {
    expect(renderMarkdown("[x](javascript:alert(1))")).toBe("<p>[x](javascript:alert(1))</p>\n");
  });

  it("renders images as links without loading them", () => {
    const html = renderMarkdown("![alt](https://e.com/i.png)");
    expect(html).toBe('<p><a href="https://e.com/i.png">alt</a></p>\n');
    expect(html).not.toContain("<img");
  });

  it("falls back to the image source as link text when alt is empty", () => {
    expect(renderMarkdown("![](https://e.com/i.png)")).toBe(
      '<p><a href="https://e.com/i.png">https://e.com/i.png</a></p>\n',
    );
  });

  it("linkifies bare URLs", () => {
    expect(renderMarkdown("https://example.com")).toBe(
      '<p><a href="https://example.com">https://example.com</a></p>\n',
    );
  });

  it("keeps task list markers as literal text", () => {
    expect(renderMarkdown("- [ ] todo")).toBe("<ul>\n<li>[ ] todo</li>\n</ul>\n");
  });
});
