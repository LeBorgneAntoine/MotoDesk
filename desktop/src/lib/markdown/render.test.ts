import { describe, expect, it } from "vitest";
import { renderMarkdown, safeHref } from "./render";

describe("safeHref", () => {
  it("keeps http(s), mailto and in-page hash links", () => {
    expect(safeHref("https://openrouter.ai")).toBe("https://openrouter.ai");
    expect(safeHref("http://localhost:17300/api")).toBe("http://localhost:17300/api");
    expect(safeHref("mailto:atelier@example.fr")).toBe("mailto:atelier@example.fr");
    expect(safeHref("#atelier")).toBe("#atelier");
  });

  it("drops javascript and unknown schemes", () => {
    expect(safeHref("javascript:alert(1)")).toBe("");
    expect(safeHref("data:text/html,hi")).toBe("");
  });
});

describe("renderMarkdown", () => {
  it("turns a title into a heading", () => {
    const html = renderMarkdown("# Activité du mois");
    expect(html).toContain("<h1>");
    expect(html).toContain("Activité du mois");
  });

  it("renders GFM tables", () => {
    const html = renderMarkdown("| Canal | CA |\n| --- | --- |\n| Atelier | 7 200 € |");
    expect(html).toContain("<table>");
    expect(html).toContain("<th>");
    expect(html).toContain("Atelier");
  });

  it("renders a safe link and bold text", () => {
    const html = renderMarkdown("Voir le **journal** sur [service-public](https://www.service-public.fr).");
    expect(html).toContain("<strong>journal</strong>");
    expect(html).toContain('href="https://www.service-public.fr"');
    expect(html).toContain("rel=\"noreferrer\"");
  });

  it("renders inline code", () => {
    const html = renderMarkdown("Fiche `clients`.");
    expect(html).toContain("<code>clients</code>");
  });

  it("does not pass through raw HTML or javascript links", () => {
    const html = renderMarkdown('<script>alert(1)</script>\n[x](javascript:alert(1))');
    expect(html).not.toContain("<script>");
    expect(html).not.toContain("javascript:");
  });
});
