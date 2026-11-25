import { describe, expect, it } from "vitest";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, resolve } from "node:path";
import { SETTINGS } from "./registry";

/**
 * A setting nobody reads is a promise the app does not keep: 24 of 58 used to be
 * like that (language, currency, notifications, opening hours…). This test keeps
 * the registry honest: every key has to be read by some code other than the
 * registry itself, or it does not belong in the list.
 */
function sourceFiles(dir: string, out: string[] = []): string[] {
  for (const name of readdirSync(dir)) {
    if (name === "node_modules" || name === "target" || name === "dist") continue;
    const path = join(dir, name);
    if (statSync(path).isDirectory()) sourceFiles(path, out);
    else if (/\.(ts|svelte|rs)$/.test(name)) out.push(path);
  }
  return out;
}

describe("settings registry", () => {
  const desktop = resolve(__dirname, "../../..");
  const registry = resolve(__dirname, "registry.ts");
  // The interface, the Tauri shell, and the data service (its own crate, beside the desktop app).
  const roots = [
    join(desktop, "src"),
    join(desktop, "src-tauri/src"),
    resolve(desktop, "../crates/motodesk-server/src"),
  ];
  const files = roots.flatMap((root) => sourceFiles(root)).filter(
    (f) => f !== registry && !f.endsWith(".test.ts"),
  );
  const text = files.map((f) => readFileSync(f, "utf8")).join("\n");

  it("lists only settings that something reads", () => {
    const unread = SETTINGS.filter((s) => !text.includes(`"${s.key}"`)).map((s) => s.key);
    expect(unread).toEqual([]);
  });
});
