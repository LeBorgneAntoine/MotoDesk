/**
 * Minimal hash-based router using Svelte 5 runes.
 *
 * Routes are identified by string ids (e.g. "login", "dashboard"). We keep it
 * intentionally tiny — no dependency, no regex matching. Modules just push
 * the next route id onto the stack via `router.go(id)`.
 *
 * A route may carry query parameters (`#/clients?panel=client:9f3a`). They are
 * used for deep links into a screen's state — most importantly the open panel
 * stack — so reloading the window restores what the user was looking at.
 */

export type RouteParams = Record<string, string>;

interface ParsedHash {
  id: string;
  params: RouteParams;
}

class Router {
  current = $state<string>("login");
  params = $state<RouteParams>({});

  constructor() {
    const parsed = this.readHash();
    this.current = parsed.id || "login";
    this.params = parsed.params;

    if (typeof window !== "undefined") {
      window.addEventListener("hashchange", () => {
        const next = this.readHash();
        this.current = next.id || "login";
        this.params = next.params;
      });
    }
  }

  private readHash(): ParsedHash {
    if (typeof window === "undefined") return { id: "", params: {} };
    const raw = window.location.hash || "";
    const body = raw.startsWith("#/") ? raw.slice(2) : raw.replace(/^#/, "");
    const q = body.indexOf("?");
    if (q < 0) return { id: body, params: {} };

    const params: RouteParams = {};
    for (const [key, value] of new URLSearchParams(body.slice(q + 1))) {
      params[key] = value;
    }
    return { id: body.slice(0, q), params };
  }

  private buildHash(id: string, params: RouteParams): string {
    const entries = Object.entries(params).filter(([, v]) => v !== "" && v !== undefined);
    if (entries.length === 0) return `#/${id}`;
    const search = new URLSearchParams(entries).toString();
    return `#/${id}?${search}`;
  }

  go(id: string, params: RouteParams = {}): void {
    this.current = id;
    this.params = params;
    if (typeof window !== "undefined") {
      window.location.hash = this.buildHash(id, params);
    }
  }

  replace(id: string, params: RouteParams = {}): void {
    this.current = id;
    this.params = params;
    if (typeof window !== "undefined") {
      const url = new URL(window.location.href);
      url.hash = this.buildHash(id, params);
      window.history.replaceState(null, "", url.toString());
    }
  }

  /**
   * Patch the current route's parameters without adding a history entry.
   * Passing `null` for a value removes it.
   */
  setParams(patch: Record<string, string | null>): void {
    const next: RouteParams = { ...this.params };
    for (const [key, value] of Object.entries(patch)) {
      if (value === null || value === "") delete next[key];
      else next[key] = value;
    }
    this.replace(this.current, next);
  }

  param(key: string): string | undefined {
    return this.params[key];
  }
}

export const router = new Router();
