/**
 * Universal print/preview entry point.
 *
 * Renders a template + data into a hidden Konva stage at a high pixel
 * density, exports the result as a PNG data URL, and either:
 *   - opens a hidden iframe pre-sized to A4 and triggers `print()`, or
 *   - returns the data URL (for the in-editor "Aperçu" pane).
 *
 * The same `printTemplate(tpl, data)` call is what the future Facturation
 * screens will use to push a real invoice to paper or PDF.
 */

import Konva from "konva";
import type { Template } from "./schema";
import { PRINT_PX_PER_MM } from "./schema";
import { drawPageBackground, renderToStage, tablePageCount } from "./engine";

export interface RenderImageOptions {
  /** Pixels per millimetre. Defaults to print density (PRINT_PX_PER_MM). */
  pxPerMm?: number;
  /** Whether to show the editor grid in the export. Should usually be false. */
  showGrid?: boolean;
  /** Multiplied on top of pxPerMm for the final raster — keep at 1 unless
   *  the output is going to be enlarged. */
  pixelRatio?: number;
  /**
   * Word stamped diagonally across the page. Used for DUPLICATA, which the
   * fiscal rules require on every copy of a document already issued, and for
   * SIMULATION on anything produced outside real trading.
   */
  watermark?: string | null;
  /** Page of a long document to draw, from 0 (see `tablePageCount`). */
  tablePage?: number;
  /** Small label at the foot of the page, for example "Page 2 / 3". */
  pageLabel?: string | null;
}

/** Renders a template + data offscreen and returns a PNG data URL. */
export async function renderTemplateImage(
  tpl: Template,
  data: unknown,
  opts: RenderImageOptions = {},
): Promise<string> {
  const pxPerMm = opts.pxPerMm ?? PRINT_PX_PER_MM;
  const showGrid = opts.showGrid ?? false;
  const pixelRatio = opts.pixelRatio ?? 1;

  // Create a hidden container so Konva has somewhere to mount.
  const container = document.createElement("div");
  container.style.position = "fixed";
  container.style.left = "-10000px";
  container.style.top = "-10000px";
  container.style.pointerEvents = "none";
  document.body.appendChild(container);

  const widthPx = Math.round(tpl.page.width * pxPerMm);
  const heightPx = Math.round(tpl.page.height * pxPerMm);

  const stage = new Konva.Stage({
    container,
    width: widthPx,
    height: heightPx,
  });

  try {
    drawPageBackground(stage, tpl, pxPerMm, showGrid);
    renderToStage(stage, tpl, data, { pxPerMm, interactive: false, tablePage: opts.tablePage ?? 0 });
    if (opts.watermark) drawWatermark(stage, opts.watermark);
    if (opts.pageLabel) drawPageLabel(stage, opts.pageLabel, pxPerMm);

    // Give async image loaders a chance to complete.
    await waitForImagesToSettle();

    return stage.toDataURL({ pixelRatio, mimeType: "image/png" });
  } finally {
    stage.destroy();
    container.remove();
  }
}

/**
 * Open the print window. Call this straight from the click, before any await:
 * a window opened later is no longer tied to the user's gesture and can be
 * blocked, and a blocked window has to be known before anything is journalled.
 */
export function openPrintWindow(): Window | null {
  const win = window.open("", "_blank", "width=900,height=1200");
  if (!win) return null;
  win.document.open();
  win.document.write(
    `<!doctype html><meta charset="utf-8"><title>Impression</title>` +
      `<p style="font:14px system-ui,sans-serif;padding:2em;color:#444">Préparation de l'impression…</p>`,
  );
  win.document.close();
  return win;
}

/**
 * Draw the document into an already open print window and start printing. A
 * document longer than its table is split over as many pages as it needs.
 *
 * Printing is started from here rather than by a script inside the page: the
 * app's content security policy does not allow inline scripts.
 */
export async function printInto(
  win: Window,
  tpl: Template,
  data: unknown,
  opts: RenderImageOptions = {},
): Promise<void> {
  const count = tablePageCount(tpl, data);
  const pages: string[] = [];
  for (let i = 0; i < count; i++) {
    pages.push(
      await renderTemplateImage(tpl, data, {
        ...opts,
        tablePage: i,
        pageLabel: count > 1 ? `Page ${i + 1} / ${count}` : null,
      }),
    );
  }

  const w = tpl.page.width;
  const h = tpl.page.height;
  win.document.open();
  win.document.write(`<!doctype html>
<html lang="fr">
<head>
  <meta charset="utf-8" />
  <title>Aperçu d'impression</title>
  <style>
    @page { size: ${w}mm ${h}mm; margin: 0; }
    html, body { margin: 0; padding: 0; background: #f3f3f3; }
    .page {
      width: ${w}mm;
      height: ${h}mm;
      margin: 8mm auto;
      background: #ffffff;
      box-shadow: 0 4px 16px rgba(0,0,0,.12);
      page-break-after: always;
    }
    .page:last-child { page-break-after: auto; }
    .page img { width: 100%; height: 100%; display: block; }
    @media print {
      body { background: #ffffff; }
      .page { margin: 0; box-shadow: none; }
    }
  </style>
</head>
<body>
${pages.map((url) => `  <div class="page"><img src="${url}" alt="" /></div>`).join("\n")}
</body>
</html>`);
  win.document.close();
  setTimeout(() => {
    win.focus();
    win.print();
  }, 300);
}

/**
 * Open a print-ready window for the given template + data. The user gets
 * the OS print dialog, which on a Tauri-bundled app exposes "Save as PDF".
 */
export async function printTemplate(
  tpl: Template,
  data: unknown,
  opts: RenderImageOptions = {},
): Promise<void> {
  const win = openPrintWindow();
  if (!win) {
    throw new Error("La fenêtre d'impression a été bloquée.");
  }
  try {
    await printInto(win, tpl, data, opts);
  } catch (err) {
    win.close();
    throw err;
  }
}

/**
 * Capture a small thumbnail (data URL PNG) of a template for the grid view.
 * Uses a modest pixel ratio so the resulting string stays under a few hundred KB.
 */
export async function renderTemplateThumbnail(
  tpl: Template,
  data: unknown,
): Promise<string> {
  return renderTemplateImage(tpl, data, {
    pxPerMm: 2,
    showGrid: false,
    pixelRatio: 1,
  });
}

// ---------- Internal ----------------------------------------------------

/**
 * Diagonal stamp across the whole page, drawn on top of everything else.
 *
 * It has to be visible on a photocopy and unmistakable at a glance, but must
 * not hide the figures underneath — an unreadable duplicate is worth nothing to
 * the client holding it.
 */
function drawWatermark(stage: Konva.Stage, text: string): void {
  const width = stage.width();
  const height = stage.height();

  const layer = new Konva.Layer({ listening: false });
  const label = new Konva.Text({
    text: text.toUpperCase(),
    fontFamily: "Helvetica, Arial, sans-serif",
    fontStyle: "bold",
    fill: "#d92d20",
    opacity: 0.16,
    align: "center",
    // Sized so the longest word still fits the diagonal of the page.
    fontSize: Math.hypot(width, height) / (text.length * 0.62),
  });

  label.rotation((Math.atan2(height, width) * 180) / Math.PI);
  label.offsetX(label.width() / 2);
  label.offsetY(label.height() / 2);
  label.position({ x: width / 2, y: height / 2 });

  layer.add(label);
  stage.add(layer);
  layer.draw();
}

/** "Page 2 / 3" at the foot of a multi-page document. */
function drawPageLabel(stage: Konva.Stage, text: string, pxPerMm: number): void {
  const layer = new Konva.Layer({ listening: false });
  layer.add(
    new Konva.Text({
      text,
      x: 0,
      y: stage.height() - 8 * pxPerMm,
      width: stage.width() - 10 * pxPerMm,
      align: "right",
      fontFamily: "Helvetica, Arial, sans-serif",
      fontSize: 3 * pxPerMm,
      fill: "#555555",
    }),
  );
  stage.add(layer);
  layer.draw();
}

function waitForImagesToSettle(): Promise<void> {
  // Two RAFs + a short delay let Konva.Image instances finish loading
  // from `loadImage` callbacks before we screenshot the stage.
  return new Promise((resolve) => {
    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        setTimeout(resolve, 80);
      });
    });
  });
}
