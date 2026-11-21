/**
 * Pure data shape of a document template.
 *
 * All coordinates / sizes are stored in **millimetres** so the schema is
 * resolution-independent: the editor uses 4 px/mm, the print path uses
 * a higher pixel ratio, but the underlying JSON stays the same.
 *
 * Anywhere a string is used as "rich content" (text, image src, …), it may
 * contain `{{path.to.var}}` tokens that are resolved at render time against
 * a flat data object — see `interpolate.ts`.
 */

export type ElementType = "text" | "image" | "table" | "rect";

export interface BaseElement {
  id: string;
  type: ElementType;
  /** Top-left X position on the page, in millimetres. */
  x: number;
  /** Top-left Y position on the page, in millimetres. */
  y: number;
  /** Width in millimetres. */
  w: number;
  /** Height in millimetres. */
  h: number;
}

export type TextAlign = "left" | "center" | "right";
export type FontWeight = "normal" | "bold";

export interface TextEl extends BaseElement {
  type: "text";
  /** May contain `{{path}}` tokens. */
  content: string;
  /** Font size in points (1 pt ≈ 0.353 mm). */
  fontSize: number;
  align: TextAlign;
  fontWeight: FontWeight;
  /** CSS color string. */
  color: string;
}

export interface ImageEl extends BaseElement {
  type: "image";
  /** URL or `{{path}}` token resolving to a URL / data URL. */
  src: string;
  /** Object-fit-like behaviour: 'contain' preserves ratio, 'fill' stretches. */
  fit: "contain" | "fill";
}

export interface TableColumn {
  /** Header label (may contain `{{path}}` tokens). */
  header: string;
  /** Sub-field of each list item to render, e.g. `"label"` for `items[].label`. */
  binding: string;
  /** Column width as a percentage of the table's total width (0-100). */
  widthPct: number;
  /** Optional formatting hint applied at render time. */
  format?: "text" | "qty" | "price" | "percent";
  align?: TextAlign;
}

export interface TableEl extends BaseElement {
  type: "table";
  /** Path to the list variable, e.g. `"items"`. */
  listBinding: string;
  columns: TableColumn[];
  /** Row height in millimetres. */
  rowHeight: number;
  /** Header row height in millimetres. */
  headerHeight: number;
  fontSize: number;
}

export interface RectEl extends BaseElement {
  type: "rect";
  stroke: string;
  strokeWidth: number;
  fill: string;
}

export type Element = TextEl | ImageEl | TableEl | RectEl;

export interface Page {
  /** Page width in millimetres. */
  width: number;
  /** Page height in millimetres. */
  height: number;
  /** Inner margin in millimetres (advisory, not enforced). */
  margin: number;
}

export interface Template {
  version: 1;
  page: Page;
  elements: Element[];
}

// ---------- Constants ---------------------------------------------------

export const PAGE_A4_PORTRAIT: Page = {
  width: 210,
  height: 297,
  margin: 15,
};

export const PAGE_A4_LANDSCAPE: Page = {
  width: 297,
  height: 210,
  margin: 15,
};

/** Number of screen pixels per millimetre in the editor (4 px/mm = 96 DPI-ish). */
export const EDITOR_PX_PER_MM = 4;

/** Higher pixel ratio used by the print path so PDFs come out crisp. */
export const PRINT_PX_PER_MM = 6;

// ---------- Factories ---------------------------------------------------

export function emptyTemplate(): Template {
  return {
    version: 1,
    page: { ...PAGE_A4_PORTRAIT },
    elements: [],
  };
}

let _uidCounter = 0;
export function uid(prefix = "el"): string {
  _uidCounter += 1;
  return `${prefix}_${Date.now().toString(36)}_${_uidCounter.toString(36)}`;
}

export function newText(x = 20, y = 20, content = "Texte"): TextEl {
  return {
    id: uid("txt"),
    type: "text",
    x, y, w: 80, h: 10,
    content,
    fontSize: 11,
    align: "left",
    fontWeight: "normal",
    color: "#1a1a1a",
  };
}

export function newImage(x = 20, y = 20, src = ""): ImageEl {
  return {
    id: uid("img"),
    type: "image",
    x, y, w: 40, h: 40,
    src,
    fit: "contain",
  };
}

export function newTable(x = 20, y = 60): TableEl {
  return {
    id: uid("tbl"),
    type: "table",
    x, y, w: 170, h: 80,
    listBinding: "items",
    rowHeight: 7,
    headerHeight: 8,
    fontSize: 10,
    columns: [
      { header: "Désignation", binding: "label",    widthPct: 50, format: "text",  align: "left"  },
      { header: "Qté",         binding: "qty",      widthPct: 12, format: "qty",   align: "right" },
      { header: "PU HT",       binding: "price_ht", widthPct: 18, format: "price", align: "right" },
      { header: "Total TTC",   binding: "total_ttc",widthPct: 20, format: "price", align: "right" },
    ],
  };
}

export function newRect(x = 20, y = 20): RectEl {
  return {
    id: uid("rct"),
    type: "rect",
    x, y, w: 40, h: 20,
    stroke: "#1a1a1a",
    strokeWidth: 0.3,
    fill: "transparent",
  };
}

// ---------- Type guards -------------------------------------------------

export const isText  = (e: Element): e is TextEl  => e.type === "text";
export const isImage = (e: Element): e is ImageEl => e.type === "image";
export const isTable = (e: Element): e is TableEl => e.type === "table";
export const isRect  = (e: Element): e is RectEl  => e.type === "rect";
