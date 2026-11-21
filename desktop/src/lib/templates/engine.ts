/**
 * Konva rendering engine for document templates.
 *
 * `renderToStage(stage, tpl, data, opts)` rebuilds the visual content of
 * a Konva.Stage from a template + data dict. It is reused by:
 *   - the editor (interactive=true: nodes are draggable & expose `name="el"`)
 *   - the preview/print path (interactive=false: static nodes for screenshot)
 *
 * Coordinates in the template are stored in millimetres; we scale to pixels
 * via `pxPerMm` so the same template renders identically at any DPI.
 */

import Konva from "konva";
import type {
  Element,
  ImageEl,
  RectEl,
  TableEl,
  Template,
  TextEl,
} from "./schema";
import { isImage, isRect, isTable, isText } from "./schema";
import {
  extractBareToken,
  formatValue,
  interpolate,
  isBareToken,
  resolve,
  tokenize,
  type FormatHint,
  type TextSegment,
} from "./interpolate";
import { humanLabelForPath } from "./variables";

const PT_TO_MM = 0.3527777778;

export interface RenderOptions {
  pxPerMm: number;
  /** If true, attach drag/selection handles + names for editor wiring. */
  interactive?: boolean;
  /**
   * Which page of a long document to draw, from 0. Tables show the slice of
   * rows that belongs to that page; everything else repeats.
   */
  tablePage?: number;
}

export interface RenderResult {
  /** Selectable element nodes keyed by element id (top-level Konva.Node). */
  nodesById: Map<string, Konva.Node>;
}

const imageCache = new Map<string, HTMLImageElement>();

function loadImage(src: string): Promise<HTMLImageElement | null> {
  if (!src) return Promise.resolve(null);
  if (imageCache.has(src)) return Promise.resolve(imageCache.get(src)!);
  return new Promise((resolve) => {
    const img = new window.Image();
    img.crossOrigin = "anonymous";
    img.onload = () => {
      imageCache.set(src, img);
      resolve(img);
    };
    img.onerror = () => resolve(null);
    img.src = src;
  });
}

function mmToPx(mm: number, ppm: number): number {
  return mm * ppm;
}

function ptToPx(pt: number, ppm: number): number {
  return pt * PT_TO_MM * ppm;
}

// ---------- Element renderers -------------------------------------------

/**
 * Adds an invisible, hit-only rect on top of an element's children so the
 * containing Group becomes draggable/clickable across its whole bounding
 * box, regardless of which inner shape is opaque under the cursor. Without
 * this, a Konva.Group with `listening:false` children has no hit area and
 * neither click nor drag would fire.
 */
function addHitArea(group: Konva.Group, w: number, h: number): void {
  group.add(
    new Konva.Rect({
      x: 0,
      y: 0,
      width: w,
      height: h,
      fill: "rgba(0,0,0,0.001)", // invisible but hit-testable
      strokeEnabled: false,
      listening: true,
      name: "hit-area",
    }),
  );
}

const TEXT_FONT_FAMILY =
  "Segoe UI, system-ui, -apple-system, Helvetica Neue, Arial, sans-serif";

function renderText(
  el: TextEl,
  data: unknown,
  ppm: number,
  interactive: boolean,
): Konva.Group {
  const w = mmToPx(el.w, ppm);
  const h = mmToPx(el.h, ppm);
  const group = new Konva.Group({
    id: el.id,
    name: interactive ? "el" : undefined,
    x: mmToPx(el.x, ppm),
    y: mmToPx(el.y, ppm),
    width: w,
    height: h,
    draggable: interactive,
  });
  group.setAttr("elementId", el.id);
  group.setAttr("elementType", "text");

  if (interactive) {
    // Editor: render with inline variable badges so users can see at a
    // glance which parts of the text are dynamic.
    paintRichText(group, el, data, ppm, w);
    addHitArea(group, w, h);
  } else {
    // Preview / print: flat interpolated string, no badges.
    const text = new Konva.Text({
      x: 0,
      y: 0,
      width: w,
      height: h,
      text: interpolate(el.content, data),
      fontSize: ptToPx(el.fontSize, ppm),
      align: el.align,
      fontStyle: el.fontWeight === "bold" ? "bold" : "normal",
      fontFamily: TEXT_FONT_FAMILY,
      fill: el.color,
      lineHeight: 1.25,
      listening: false,
    });
    group.add(text);
  }
  return group;
}

/**
 * Paints text content into a Konva.Group with inline pill-badges for any
 * `{{path|hint}}` variable. Layout is line-by-line (split on \n) using a
 * horizontal cursor — no auto-wrap, which keeps the badge rectangles
 * coherent with their text.
 */
function paintRichText(
  group: Konva.Group,
  el: TextEl,
  data: unknown,
  ppm: number,
  boxWidthPx: number,
): void {
  const fontSize = ptToPx(el.fontSize, ppm);
  const fontStyle = el.fontWeight === "bold" ? "bold" : "normal";
  const lineH = fontSize * 1.25;

  const lines = (el.content || "").split("\n");
  let y = 0;
  for (const rawLine of lines) {
    const segs = tokenize(rawLine);
    // -- measure each segment up-front so we can align the whole line --
    type Measured =
      | { kind: "text"; text: string; w: number }
      | { kind: "var";  value: string; w: number; h: number; padX: number; raw: string };
    const measured: Measured[] = [];
    let totalW = 0;
    const VAR_PAD_X = 4;
    const VAR_GAP = 1;

    for (const seg of segs as TextSegment[]) {
      if (seg.type === "text") {
        if (!seg.text) continue;
        const probe = new Konva.Text({
          text: seg.text, fontSize, fontStyle, fontFamily: TEXT_FONT_FAMILY,
        });
        const w = probe.getTextWidth();
        probe.destroy();
        measured.push({ kind: "text", text: seg.text, w });
        totalW += w;
      } else {
        // Editor: badge label = "Nom (Client)" — never the resolved value,
        // so the page reads as a *template* and the user can't confuse
        // sample data with literal text.
        const label = humanLabelForPath(seg.path);
        const probe = new Konva.Text({
          text: label, fontSize, fontStyle, fontFamily: TEXT_FONT_FAMILY,
        });
        const tw = probe.getTextWidth();
        probe.destroy();
        const w = tw + VAR_PAD_X * 2;
        measured.push({
          kind: "var",
          value: label,
          w,
          h: fontSize * 1.18,
          padX: VAR_PAD_X,
          raw: seg.raw,
        });
        totalW += w + VAR_GAP;
      }
    }

    // -- alignment offset --
    let x = 0;
    if (el.align === "center") x = Math.max(0, (boxWidthPx - totalW) / 2);
    else if (el.align === "right") x = Math.max(0, boxWidthPx - totalW);

    for (const m of measured) {
      if (m.kind === "text") {
        group.add(
          new Konva.Text({
            x,
            y,
            text: m.text,
            fontSize,
            fontStyle,
            fontFamily: TEXT_FONT_FAMILY,
            fill: el.color,
            lineHeight: 1,
            listening: false,
          }),
        );
        x += m.w;
      } else {
        const badgeY = y - (m.h - fontSize) / 2;
        group.add(
          new Konva.Rect({
            x,
            y: badgeY,
            width: m.w,
            height: m.h,
            fill: "rgba(25, 118, 210, 0.10)",
            stroke: "rgba(25, 118, 210, 0.55)",
            strokeWidth: 0.5,
            cornerRadius: 3,
            listening: false,
          }),
        );
        group.add(
          new Konva.Text({
            x: x + m.padX,
            y,
            text: m.value,
            fontSize,
            fontStyle,
            fontFamily: TEXT_FONT_FAMILY,
            fill: "#0d4d8c",
            lineHeight: 1,
            listening: false,
          }),
        );
        x += m.w + VAR_GAP;
      }
    }

    y += lineH;
  }
}

function renderRect(el: RectEl, ppm: number, interactive: boolean): Konva.Group {
  const w = mmToPx(el.w, ppm);
  const h = mmToPx(el.h, ppm);
  const group = new Konva.Group({
    id: el.id,
    name: interactive ? "el" : undefined,
    x: mmToPx(el.x, ppm),
    y: mmToPx(el.y, ppm),
    width: w,
    height: h,
    draggable: interactive,
  });
  group.setAttr("elementId", el.id);
  group.setAttr("elementType", "rect");

  const rect = new Konva.Rect({
    x: 0,
    y: 0,
    width: w,
    height: h,
    stroke: el.stroke,
    strokeWidth: Math.max(0.5, mmToPx(el.strokeWidth, ppm)),
    fill: el.fill === "transparent" ? undefined : el.fill,
    listening: false,
  });
  group.add(rect);
  if (interactive) addHitArea(group, w, h);
  return group;
}

function renderImage(
  el: ImageEl,
  data: unknown,
  ppm: number,
  interactive: boolean,
): Konva.Group {
  const w = mmToPx(el.w, ppm);
  const h = mmToPx(el.h, ppm);
  const group = new Konva.Group({
    id: el.id,
    name: interactive ? "el" : undefined,
    x: mmToPx(el.x, ppm),
    y: mmToPx(el.y, ppm),
    width: w,
    height: h,
    draggable: interactive,
  });
  group.setAttr("elementId", el.id);
  group.setAttr("elementType", "image");

  // Placeholder (drawn until / unless the image loads).
  const placeholder = new Konva.Rect({
    x: 0,
    y: 0,
    width: w,
    height: h,
    stroke: "#c8c8c8",
    strokeWidth: 1,
    dash: [4, 3],
    fill: "rgba(0,0,0,0.02)",
    listening: false,
  });
  const placeholderLabel = new Konva.Text({
    x: 0,
    y: h / 2 - 8,
    width: w,
    height: 16,
    text: "Image",
    align: "center",
    fontSize: 11,
    fill: "#8a8a8a",
    listening: false,
  });
  group.add(placeholder);
  group.add(placeholderLabel);

  const resolvedSrc = isBareToken(el.src)
    ? String(resolve(extractBareToken(el.src)!, data) ?? "")
    : interpolate(el.src, data);

  if (resolvedSrc) {
    loadImage(resolvedSrc).then((img) => {
      if (!img || !group.getStage()) return;
      placeholder.destroy();
      placeholderLabel.destroy();

      let drawW = w;
      let drawH = h;
      let offX = 0;
      let offY = 0;
      if (el.fit === "contain") {
        const ratio = Math.min(w / img.width, h / img.height);
        drawW = img.width * ratio;
        drawH = img.height * ratio;
        offX = (w - drawW) / 2;
        offY = (h - drawH) / 2;
      }
      const node = new Konva.Image({
        x: offX,
        y: offY,
        width: drawW,
        height: drawH,
        image: img,
        listening: false,
      });
      group.add(node);
      // Keep the hit-area on top so the group remains draggable from any
      // transparent area of the image.
      const hit = group.findOne(".hit-area");
      if (hit) hit.moveToTop();
      group.getLayer()?.batchDraw();
    });
  }

  if (interactive) addHitArea(group, w, h);
  return group;
}

/** How many rows fit in a table element, from its geometry alone. */
function tableCapacity(el: TableEl): number {
  return Math.max(1, Math.floor((el.h - el.headerHeight) / el.rowHeight));
}

/** Rows a table element is bound to, in the data. */
function tableRows(el: TableEl, data: unknown): unknown[] {
  const list = resolve(el.listBinding, data);
  return Array.isArray(list) ? list : [];
}

/**
 * Number of pages a document needs: the longest table decides. A list that does
 * not fit its table used to be cut off silently, with the totals underneath
 * still counting every line.
 */
export function tablePageCount(tpl: Template, data: unknown): number {
  let pages = 1;
  for (const el of tpl.elements) {
    if (!isTable(el)) continue;
    pages = Math.max(pages, Math.ceil(tableRows(el, data).length / tableCapacity(el)));
  }
  return pages;
}

function renderTable(
  el: TableEl,
  data: unknown,
  ppm: number,
  interactive: boolean,
  page = 0,
): Konva.Group {
  const totalW = mmToPx(el.w, ppm);
  const totalH = mmToPx(el.h, ppm);
  const group = new Konva.Group({
    id: el.id,
    name: interactive ? "el" : undefined,
    x: mmToPx(el.x, ppm),
    y: mmToPx(el.y, ppm),
    width: totalW,
    height: totalH,
    draggable: interactive,
  });
  group.setAttr("elementId", el.id);
  group.setAttr("elementType", "table");
  const headerH = mmToPx(el.headerHeight, ppm);
  const rowH = mmToPx(el.rowHeight, ppm);
  const fontSizePx = ptToPx(el.fontSize, ppm);
  const padX = 4;

  // Normalise column widths in case they don't sum to 100.
  const totalPct = el.columns.reduce((s, c) => s + (c.widthPct || 0), 0) || 100;
  const colWidths = el.columns.map((c) => (totalW * (c.widthPct || 0)) / totalPct);
  const colXs: number[] = [];
  {
    let cx = 0;
    for (const w of colWidths) {
      colXs.push(cx);
      cx += w;
    }
  }

  // Header background
  group.add(
    new Konva.Rect({
      x: 0,
      y: 0,
      width: totalW,
      height: headerH,
      fill: "#f3f3f3",
      stroke: "#1a1a1a",
      strokeWidth: 0.5,
      listening: false,
    }),
  );
  // Headers
  el.columns.forEach((col, i) => {
    group.add(
      new Konva.Text({
        x: colXs[i] + padX,
        y: 0,
        width: Math.max(0, colWidths[i] - padX * 2),
        height: headerH,
        text: interpolate(col.header, data),
        align: col.align ?? "left",
        verticalAlign: "middle",
        fontSize: fontSizePx,
        fontStyle: "bold",
        fontFamily: "Segoe UI, system-ui, sans-serif",
        fill: "#0d0d0d",
        listening: false,
      }),
    );
  });

  // Rows — pull live list or fall back to 3 sample rows for preview.
  const allRows = tableRows(el, data);

  const maxRows = Math.max(0, Math.floor((totalH - headerH) / rowH));
  // A table that has already finished on an earlier page is not repeated empty.
  if (page > 0 && allRows.length <= page * maxRows) {
    return new Konva.Group({ id: el.id, x: 0, y: 0, width: 0, height: 0, listening: false });
  }
  const rows = allRows.slice(page * maxRows, (page + 1) * maxRows);
  const drawnRows = Math.min(rows.length || (interactive && page === 0 ? 3 : 0), maxRows);

  for (let r = 0; r < drawnRows; r++) {
    const row = rows[r] ?? {};
    const y = headerH + r * rowH;

    if (r % 2 === 1) {
      group.add(
        new Konva.Rect({
          x: 0,
          y,
          width: totalW,
          height: rowH,
          fill: "#fafafa",
          listening: false,
        }),
      );
    }

    el.columns.forEach((col, i) => {
      const value = resolve(col.binding, row);
      const formatted = formatValue(value, (col.format ?? "text") as FormatHint);
      group.add(
        new Konva.Text({
          x: colXs[i] + padX,
          y,
          width: Math.max(0, colWidths[i] - padX * 2),
          height: rowH,
          text: formatted,
          align: col.align ?? "left",
          verticalAlign: "middle",
          fontSize: fontSizePx,
          fontFamily: "Segoe UI, system-ui, sans-serif",
          fill: "#1a1a1a",
          listening: false,
        }),
      );
    });
  }

  // Outer border + column separators
  const bottomY = Math.min(totalH, headerH + drawnRows * rowH);
  group.add(
    new Konva.Rect({
      x: 0,
      y: 0,
      width: totalW,
      height: bottomY,
      stroke: "#1a1a1a",
      strokeWidth: 0.5,
      listening: false,
    }),
  );
  // Column separators
  colXs.slice(1).forEach((x) => {
    group.add(
      new Konva.Line({
        points: [x, 0, x, bottomY],
        stroke: "#c8c8c8",
        strokeWidth: 0.5,
        listening: false,
      }),
    );
  });
  // Header underline
  group.add(
    new Konva.Line({
      points: [0, headerH, totalW, headerH],
      stroke: "#1a1a1a",
      strokeWidth: 0.5,
      listening: false,
    }),
  );

  if (interactive) addHitArea(group, totalW, totalH);
  return group;
}

function renderElement(
  el: Element,
  data: unknown,
  ppm: number,
  interactive: boolean,
  page = 0,
): Konva.Group {
  if (isText(el))  return renderText(el, data, ppm, interactive);
  if (isImage(el)) return renderImage(el, data, ppm, interactive);
  if (isTable(el)) return renderTable(el, data, ppm, interactive, page);
  if (isRect(el))  return renderRect(el, ppm, interactive);
  throw new Error(`Unknown element type: ${(el as Element).type}`);
}

// ---------- Public API --------------------------------------------------

/**
 * Re-render the contents layer of `stage`. Returns a map of element id →
 * Konva node so the editor can wire up selection/dragging without reaching
 * into the layer directly.
 *
 * The stage is expected to have ONE Konva.Layer named "content" — we
 * destroy and recreate every node in it on each call. This keeps the
 * editor's reactive flow simple (re-render the whole layer on schema
 * change). Number of elements per template stays small (< 100) so this
 * is plenty fast.
 */
export function renderToStage(
  stage: Konva.Stage,
  tpl: Template,
  data: unknown,
  opts: RenderOptions,
): RenderResult {
  const layer = ensureContentLayer(stage);
  layer.destroyChildren();

  const nodesById = new Map<string, Konva.Node>();
  for (const el of tpl.elements) {
    const node = renderElement(el, data, opts.pxPerMm, opts.interactive ?? false, opts.tablePage ?? 0);
    nodesById.set(el.id, node);
    layer.add(node);
  }
  layer.batchDraw();
  return { nodesById };
}

/**
 * Re-lays-out the children of an element node at the given new dimensions
 * (in millimetres), without rebuilding the whole stage. Used by the
 * transformer to keep text and other shapes pixel-crisp during a resize
 * instead of letting Konva scale-distort the existing children.
 */
export function resizeElementInPlace(
  node: Konva.Group,
  el: Element,
  newWidthMm: number,
  newHeightMm: number,
  data: unknown,
  ppm: number,
): void {
  // Patch the schema-like object and re-render the children only.
  const updated = { ...el, w: newWidthMm, h: newHeightMm } as Element;
  const fresh = renderElement(updated, data, ppm, true);

  // Move new children onto the existing node and discard the temp group.
  node.destroyChildren();
  const newChildren = fresh.getChildren().slice();
  for (const c of newChildren) {
    c.remove();
    node.add(c);
  }
  fresh.destroy();

  node.width(mmToPx(newWidthMm, ppm));
  node.height(mmToPx(newHeightMm, ppm));
}

/** Returns the "content" layer, creating it if missing. */
export function ensureContentLayer(stage: Konva.Stage): Konva.Layer {
  let layer = stage.findOne<Konva.Layer>("#content-layer");
  if (!layer) {
    layer = new Konva.Layer({ id: "content-layer" });
    stage.add(layer);
  }
  return layer;
}

/** Returns the "background" layer (page + grid), creating it if missing. */
export function ensureBackgroundLayer(stage: Konva.Stage): Konva.Layer {
  let layer = stage.findOne<Konva.Layer>("#background-layer");
  if (!layer) {
    layer = new Konva.Layer({ id: "background-layer" });
    stage.add(layer);
    stage.find<Konva.Layer>("Layer").forEach((l) => l.moveToTop());
    layer.moveToBottom();
  }
  return layer;
}

/**
 * Paint the A4 sheet shadow + an optional gridded background. Call once
 * after creating the stage; safe to call again on resize.
 */
export function drawPageBackground(
  stage: Konva.Stage,
  tpl: Template,
  pxPerMm: number,
  showGrid: boolean,
): void {
  const layer = ensureBackgroundLayer(stage);
  layer.destroyChildren();

  const w = mmToPx(tpl.page.width, pxPerMm);
  const h = mmToPx(tpl.page.height, pxPerMm);

  layer.add(
    new Konva.Rect({
      x: 0,
      y: 0,
      width: w,
      height: h,
      fill: "#ffffff",
      listening: false,
    }),
  );

  if (showGrid) {
    const stepMm = 5;
    const stepPx = mmToPx(stepMm, pxPerMm);
    for (let x = stepPx; x < w; x += stepPx) {
      layer.add(
        new Konva.Line({
          points: [x, 0, x, h],
          stroke: x % (stepPx * 2) === 0 ? "#eeeeee" : "#f6f6f6",
          strokeWidth: 0.5,
          listening: false,
        }),
      );
    }
    for (let y = stepPx; y < h; y += stepPx) {
      layer.add(
        new Konva.Line({
          points: [0, y, w, y],
          stroke: y % (stepPx * 2) === 0 ? "#eeeeee" : "#f6f6f6",
          strokeWidth: 0.5,
          listening: false,
        }),
      );
    }
  }

  // Margin indicator
  const m = mmToPx(tpl.page.margin, pxPerMm);
  layer.add(
    new Konva.Rect({
      x: m,
      y: m,
      width: w - 2 * m,
      height: h - 2 * m,
      stroke: "#dcdcdc",
      strokeWidth: 0.5,
      dash: [4, 3],
      listening: false,
    }),
  );

  layer.batchDraw();
}
