#!/usr/bin/env node
/**
 * lint:ui — guards the design system rules that a type checker cannot see.
 *
 *   1. raw-html      no raw interactive/heading HTML in routes; use the kit
 *   2. color-literal no hex/rgb/hsl in CSS outside lib/styles; use tokens
 *   3. font-size     no raw px font sizes in CSS outside lib/styles; use the ramp
 *   4. spinner       no local spin keyframes; use <Spinner />
 *
 * Escape hatch: put `lint-ui-allow: <why>` in a comment on the offending line or
 * the line above it. Use it for genuinely non-themed output (printed documents,
 * canvas rendering), not to dodge the rule.
 *
 * See .cursor/rules/design-system.mdc and .cursor/rules/ui-kit.mdc.
 */

import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative, sep } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = fileURLToPath(new URL("..", import.meta.url));
const SRC = join(ROOT, "src");
const ROUTES = join(SRC, "routes");
const TOKENS = join(SRC, "lib", "styles");
const KIT = join(SRC, "lib", "components");

/** Files whose colours describe printed documents rather than the UI theme. */
const CANVAS_FILES = [
  join(SRC, "lib", "templates"),
];

/** Lowercase only — `<Button>` is the kit component, `<button>` is the violation. */
const RAW_HTML = /<(button|input|select|textarea|table|h[1-6])(\s|>|\/)/g;
const COLOR_LITERAL =
  /(?<![\w{&])#(?:[0-9a-fA-F]{8}|[0-9a-fA-F]{6}|[0-9a-fA-F]{4}|[0-9a-fA-F]{3})\b|\b(?:rgba?|hsla?)\s*\(/g;
const FONT_SIZE_PX = /font-size:\s*[\d.]+px/g;
const SPIN_KEYFRAMES = /@keyframes\s+[\w-]*spin[\w-]*/gi;

const findings = [];

function walk(dir) {
  const out = [];
  for (const entry of readdirSync(dir)) {
    if (entry === "node_modules" || entry.startsWith(".")) continue;
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) out.push(...walk(full));
    else out.push(full);
  }
  return out;
}

function isUnder(file, dir) {
  return file === dir || file.startsWith(dir + sep);
}

/** Byte ranges of every `<script>` / `<style>` block in a Svelte file. */
function blockRanges(source, tag) {
  const ranges = [];
  const re = new RegExp(`<${tag}\\b[^>]*>`, "gi");
  let open;
  while ((open = re.exec(source)) !== null) {
    const start = open.index + open[0].length;
    const end = source.indexOf(`</${tag}>`, start);
    ranges.push([start, end < 0 ? source.length : end]);
  }
  return ranges;
}

function inRanges(index, ranges) {
  return ranges.some(([start, end]) => index >= start && index < end);
}

function lineInfo(source, index) {
  const before = source.slice(0, index);
  const line = before.split("\n").length;
  const lineStart = before.lastIndexOf("\n") + 1;
  const lineEnd = source.indexOf("\n", index);
  return {
    line,
    text: source.slice(lineStart, lineEnd < 0 ? source.length : lineEnd).trim(),
  };
}

/** `lint-ui-allow` on the offending line or the one above it. */
function isAllowed(lines, lineNumber) {
  const here = lines[lineNumber - 1] ?? "";
  const above = lines[lineNumber - 2] ?? "";
  return here.includes("lint-ui-allow") || above.includes("lint-ui-allow");
}

function scan(file, source, rule, pattern, ranges, message) {
  const lines = source.split("\n");
  pattern.lastIndex = 0;
  let match;
  while ((match = pattern.exec(source)) !== null) {
    if (ranges && !inRanges(match.index, ranges)) continue;
    const { line, text } = lineInfo(source, match.index);
    if (isAllowed(lines, line)) continue;
    findings.push({
      file: relative(ROOT, file).split(sep).join("/"),
      line,
      rule,
      message,
      text,
    });
  }
}

for (const file of walk(SRC)) {
  const isSvelte = file.endsWith(".svelte");
  const isCss = file.endsWith(".css");
  if (!isSvelte && !isCss) continue;

  const source = readFileSync(file, "utf8");
  const styleRanges = isSvelte ? blockRanges(source, "style") : null;
  const scriptRanges = isSvelte ? blockRanges(source, "script") : [];

  if (isSvelte && isUnder(file, ROUTES)) {
    const markupOnly = [];
    // Everything that is neither <script> nor <style> is markup.
    const excluded = [...scriptRanges, ...(styleRanges ?? [])].sort((a, b) => a[0] - b[0]);
    let cursor = 0;
    for (const [start, end] of excluded) {
      if (start > cursor) markupOnly.push([cursor, start]);
      cursor = Math.max(cursor, end);
    }
    markupOnly.push([cursor, source.length]);

    scan(
      file,
      source,
      "raw-html",
      RAW_HTML,
      markupOnly,
      "raw HTML control in a route — use the $lib/components equivalent",
    );
  }

  if (isUnder(file, TOKENS)) continue;
  const isCanvas = CANVAS_FILES.some((dir) => isUnder(file, dir));

  if (!isCanvas) {
    scan(
      file,
      source,
      "color-literal",
      COLOR_LITERAL,
      styleRanges,
      "colour literal outside lib/styles — use a var(--color-*) token",
    );
  }

  scan(
    file,
    source,
    "font-size",
    FONT_SIZE_PX,
    styleRanges,
    "raw px font size — use a var(--fs-*) token",
  );

  // Kit components own their animations; screens must not invent spinners.
  if (!isUnder(file, KIT)) {
    scan(
      file,
      source,
      "spinner",
      SPIN_KEYFRAMES,
      styleRanges,
      "local spin keyframes — use <Spinner /> or Button loading",
    );
  }
}

if (findings.length === 0) {
  console.log("lint:ui — no design-system violations");
  process.exit(0);
}

findings.sort((a, b) => a.file.localeCompare(b.file) || a.line - b.line);
for (const f of findings) {
  console.error(`${f.file}:${f.line}  [${f.rule}] ${f.message}\n    ${f.text}`);
}
console.error(`\nlint:ui — ${findings.length} violation(s)`);
process.exit(1);
