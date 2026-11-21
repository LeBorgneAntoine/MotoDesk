import { Marked } from "marked";

/** Allow only navigable, non-script URLs in rendered markdown. */
export function safeHref(href: string): string {
  const trimmed = href.trim();
  if (/^(https?:\/\/|mailto:|#|\/)/i.test(trimmed)) return trimmed;
  return "";
}

function escapeAttr(value: string): string {
  return value
    .replace(/&/g, "&amp;")
    .replace(/"/g, "&quot;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");
}

const parser = new Marked({
  gfm: true,
  breaks: false,
  renderer: {
    html() {
      return "";
    },
    link({ href, title, tokens }) {
      const body = this.parser.parseInline(tokens);
      const safe = safeHref(href ?? "");
      if (!safe) return body;
      const titleAttr = title ? ` title="${escapeAttr(title)}"` : "";
      return `<a href="${escapeAttr(safe)}" rel="noreferrer"${titleAttr}>${body}</a>`;
    },
  },
});

/** Compile markdown to sanitized HTML for the canvas document view. */
export function renderMarkdown(src: string): string {
  const out = parser.parse(src, { async: false });
  return typeof out === "string" ? out : "";
}
