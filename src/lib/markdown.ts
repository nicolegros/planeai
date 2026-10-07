import MarkdownIt from "markdown-it";

const md = new MarkdownIt({ html: false, linkify: true });

// Images become links so rendering never triggers remote fetches (tracking pixels, IP leaks).
md.renderer.rules.image = (tokens, idx) => {
  const token = tokens[idx]!;
  const src = String(token.attrGet("src") ?? "");
  const alt = md.utils.escapeHtml(token.content || src);
  if (!src || !md.validateLink(src)) return alt;
  return `<a href="${md.utils.escapeHtml(src)}">${alt}</a>`;
};

export function renderMarkdown(source: string): string {
  return md.render(source);
}
