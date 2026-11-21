<script lang="ts" module>
  export interface A4StageApi {
    captureDataUrl: (pixelRatio?: number) => string | undefined;
  }

  export interface ElementTransform {
    x: number;
    y: number;
    w?: number;
    h?: number;
  }
</script>

<script lang="ts">
  import { onMount, untrack } from "svelte";
  import Konva from "konva";

  import {
    drawPageBackground,
    ensureContentLayer,
    renderToStage,
    resizeElementInPlace,
  } from "$lib/templates/engine";
  import {
    EDITOR_PX_PER_MM,
    type Element,
    type ElementType,
    type Template,
  } from "$lib/templates/schema";

  interface Props {
    template: Template;
    /** Computed sample data merged with shop settings for live preview. */
    data: unknown;
    selectedId: string | null;
    zoom: number;
    /** Notify parent when an element is moved/resized via the transformer. */
    onTransformElement: (id: string, t: ElementTransform) => void;
    onSelectElement: (id: string | null) => void;
    /** Double-click on a text element → open inline editing overlay. */
    onEditText: (id: string, screenRect: { x: number; y: number; w: number; h: number }) => void;
    /** Mouse-wheel zoom (Ctrl+wheel). delta>0 = zoom in. */
    onZoomDelta?: (delta: number) => void;
    /** Drag & drop new element from the tool palette onto the canvas. */
    onDropElement?: (kind: ElementType, xMm: number, yMm: number) => void;
    /** Called once the stage is ready; lets the parent capture a thumbnail. */
    onReady?: (api: A4StageApi) => void;
  }

  let {
    template,
    data,
    selectedId,
    zoom,
    onTransformElement,
    onSelectElement,
    onEditText,
    onZoomDelta,
    onDropElement,
    onReady,
  }: Props = $props();

  let host: HTMLDivElement | undefined = $state();
  let stage: Konva.Stage | null = null;
  let guidesLayer: Konva.Layer | null = null;
  let selectionLayer: Konva.Layer | null = null;
  let transformer: Konva.Transformer | null = null;
  let dropHint: { x: number; y: number; w: number; h: number } | null = $state(null);

  // Effective pixels-per-mm used by the renderer. Re-rendering at the
  // zoomed resolution keeps text and lines pixel-crisp at any zoom level
  // (the Canvas-equivalent of vector-quality), rather than scaling a
  // pre-rasterised stage.
  const ppm = $derived(EDITOR_PX_PER_MM * zoom);

  // Minimum sizes for transform (in mm) so resize handles can't collapse
  // an element into nothingness.
  const MIN_W_MM = 5;
  const MIN_H_MM = 3;

  // Pixel threshold (in screen-px at current ppm) for the snap-to-guide
  // behaviour. Tuned to feel responsive without locking too aggressively.
  const SNAP_PX = 5;

  // ---------------------------------------------------------------------
  // Lifecycle
  // ---------------------------------------------------------------------

  onMount(() => {
    if (!host) return;

    host.addEventListener("wheel", onWheel, { passive: false });
    host.addEventListener("dragover", onDragOver);
    host.addEventListener("dragleave", onDragLeave);
    host.addEventListener("drop", onDrop);

    stage = new Konva.Stage({
      container: host,
      width: template.page.width * ppm,
      height: template.page.height * ppm,
    });

    stage.on("click tap", (e) => {
      if (e.target === stage) onSelectElement(null);
    });

    guidesLayer = new Konva.Layer({ id: "guides-layer", listening: false });
    stage.add(guidesLayer);

    selectionLayer = new Konva.Layer({ id: "selection-layer" });
    stage.add(selectionLayer);

    transformer = new Konva.Transformer({
      rotateEnabled: false,
      keepRatio: false,
      ignoreStroke: true,
      borderStroke: "#1976d2",
      borderStrokeWidth: 1.5,
      borderDash: [],
      anchorStroke: "#1976d2",
      anchorFill: "#ffffff",
      anchorSize: 8,
      anchorCornerRadius: 0,
      enabledAnchors: [
        "top-left", "top-center", "top-right",
        "middle-left", "middle-right",
        "bottom-left", "bottom-center", "bottom-right",
      ],
      boundBoxFunc: (_old, next) => ({
        ...next,
        width:  Math.max(MIN_W_MM * ppm, next.width),
        height: Math.max(MIN_H_MM * ppm, next.height),
      }),
    });
    selectionLayer.add(transformer);

    transformer.on("transform", liveResize);
    transformer.on("transformend", commitTransformer);

    drawPageBackground(stage, template, ppm, true);
    rebuildContent();
    syncStageSize();

    onReady?.({
      captureDataUrl: (pixelRatio = 0.3) => stage?.toDataURL({ pixelRatio }),
    });

    return () => {
      host?.removeEventListener("wheel", onWheel);
      host?.removeEventListener("dragover", onDragOver);
      host?.removeEventListener("dragleave", onDragLeave);
      host?.removeEventListener("drop", onDrop);
      stage?.destroy();
      stage = null;
      guidesLayer = null;
      selectionLayer = null;
      transformer = null;
    };
  });

  // ---------------------------------------------------------------------
  // Reactive updates
  // ---------------------------------------------------------------------

  $effect(() => {
    template.elements;
    data;
    if (!stage) return;
    untrack(() => {
      rebuildContent();
      drawSelection();
    });
  });

  $effect(() => {
    template.page;
    if (!stage) return;
    untrack(() => {
      syncStageSize();
      drawPageBackground(stage!, template, ppm, true);
      rebuildContent();
      drawSelection();
    });
  });

  $effect(() => {
    zoom;
    if (!stage) return;
    untrack(() => {
      syncStageSize();
      drawPageBackground(stage!, template, ppm, true);
      rebuildContent();
      drawSelection();
    });
  });

  $effect(() => {
    selectedId;
    if (!stage) return;
    untrack(() => drawSelection());
  });

  // ---------------------------------------------------------------------
  // Internals — stage / layers
  // ---------------------------------------------------------------------

  function syncStageSize() {
    if (!stage) return;
    stage.size({
      width: template.page.width * ppm,
      height: template.page.height * ppm,
    });
    const desiredPx = Math.max(1, window.devicePixelRatio || 1);
    stage.find<Konva.Layer>("Layer").forEach((l) => {
      try { l.canvas.setPixelRatio(desiredPx); } catch { /* noop */ }
    });
  }

  function rebuildContent() {
    if (!stage) return;
    const { nodesById } = renderToStage(stage, template, data, {
      pxPerMm: ppm,
      interactive: true,
    });

    for (const [id, node] of nodesById) {
      node.on("mousedown touchstart", () => {
        if (selectedId !== id) onSelectElement(id);
      });
      node.on("click tap", (e) => {
        e.cancelBubble = true;
        onSelectElement(id);
      });
      node.on("dblclick dbltap", (e) => {
        e.cancelBubble = true;
        const el = template.elements.find((x) => x.id === id);
        if (!el || el.type !== "text") return;
        const box = node.getClientRect({ relativeTo: stage! });
        onEditText(id, { x: box.x, y: box.y, w: box.width, h: box.height });
      });
      node.on("dragstart", () => {
        // Cache the element's mm dimensions for snapping against page/elements.
        node.setAttr("snap-ready", true);
      });
      node.on("dragmove", () => snapDuringDrag(node, id));
      node.on("dragend", () => {
        clearGuides();
        const xMm = Math.max(0, node.x() / ppm);
        const yMm = Math.max(0, node.y() / ppm);
        onTransformElement(id, {
          x: roundMm(xMm),
          y: roundMm(yMm),
        });
      });
    }
    selectionLayer?.moveToTop();
  }

  function drawSelection() {
    if (!stage || !transformer || !selectionLayer) return;
    if (!selectedId) {
      transformer.nodes([]);
      selectionLayer.batchDraw();
      return;
    }
    const layer = ensureContentLayer(stage);
    const node = layer.findOne(`#${cssEscape(selectedId)}`);
    if (!node) {
      transformer.nodes([]);
      selectionLayer.batchDraw();
      return;
    }
    transformer.nodes([node]);
    selectionLayer.batchDraw();
  }

  // ---------------------------------------------------------------------
  // Live resize (no distortion) — rebuilds children every frame
  // ---------------------------------------------------------------------

  function liveResize() {
    if (!transformer) return;
    const node = transformer.nodes()[0];
    if (!(node instanceof Konva.Group)) return;
    const id = node.id();
    const el = template.elements.find((x) => x.id === id);
    if (!el) return;

    const sx = node.scaleX();
    const sy = node.scaleY();
    const newWPx = Math.max(MIN_W_MM * ppm, node.width() * sx);
    const newHPx = Math.max(MIN_H_MM * ppm, node.height() * sy);
    // Reset the scale so future frames compare against width/height again.
    node.scale({ x: 1, y: 1 });
    node.width(newWPx);
    node.height(newHPx);

    resizeElementInPlace(node, el, newWPx / ppm, newHPx / ppm, data, ppm);
  }

  function commitTransformer() {
    if (!transformer) return;
    const node = transformer.nodes()[0];
    if (!(node instanceof Konva.Group)) return;
    const id = node.id();
    const el = template.elements.find((x) => x.id === id);
    if (!el) return;

    // After liveResize ran, width/height are already updated and scale is 1.
    const newWMm = node.width() / ppm;
    const newHMm = node.height() / ppm;
    const newX = Math.max(0, node.x() / ppm);
    const newY = Math.max(0, node.y() / ppm);

    onTransformElement(id, {
      x: roundMm(newX),
      y: roundMm(newY),
      w: roundMm(Math.max(MIN_W_MM, newWMm)),
      h: roundMm(Math.max(MIN_H_MM, newHMm)),
    });
  }

  // ---------------------------------------------------------------------
  // Snap-to-guides during drag
  // ---------------------------------------------------------------------

  interface SnapTargets { x: number[]; y: number[]; }

  function computeSnapTargets(exceptId: string): SnapTargets {
    const pageW = template.page.width  * ppm;
    const pageH = template.page.height * ppm;
    const m     = template.page.margin * ppm;

    const xs = new Set<number>([0, pageW, m, pageW - m, pageW / 2]);
    const ys = new Set<number>([0, pageH, m, pageH - m, pageH / 2]);

    for (const el of template.elements) {
      if (el.id === exceptId) continue;
      const ex = el.x * ppm;
      const ey = el.y * ppm;
      const ew = el.w * ppm;
      const eh = el.h * ppm;
      xs.add(ex);
      xs.add(ex + ew / 2);
      xs.add(ex + ew);
      ys.add(ey);
      ys.add(ey + eh / 2);
      ys.add(ey + eh);
    }
    return { x: [...xs], y: [...ys] };
  }

  function snapDuringDrag(node: Konva.Node, id: string) {
    if (!stage || !guidesLayer) return;
    const w = (node as Konva.Group).width();
    const h = (node as Konva.Group).height();
    const targets = computeSnapTargets(id);

    // Candidate match points on the moving node, in stage coords.
    const myXs = [node.x(), node.x() + w / 2, node.x() + w];
    const myYs = [node.y(), node.y() + h / 2, node.y() + h];

    // Best snap per axis.
    let bestDx = Infinity, snapX: number | null = null, fromXIdx = 0;
    for (let i = 0; i < myXs.length; i++) {
      for (const tx of targets.x) {
        const d = tx - myXs[i];
        if (Math.abs(d) < Math.abs(bestDx)) { bestDx = d; snapX = tx; fromXIdx = i; }
      }
    }
    let bestDy = Infinity, snapY: number | null = null, fromYIdx = 0;
    for (let i = 0; i < myYs.length; i++) {
      for (const ty of targets.y) {
        const d = ty - myYs[i];
        if (Math.abs(d) < Math.abs(bestDy)) { bestDy = d; snapY = ty; fromYIdx = i; }
      }
    }

    const hitX = snapX != null && Math.abs(bestDx) <= SNAP_PX;
    const hitY = snapY != null && Math.abs(bestDy) <= SNAP_PX;

    if (hitX) node.x(node.x() + bestDx);
    if (hitY) node.y(node.y() + bestDy);

    drawGuides(hitX ? snapX! : null, hitY ? snapY! : null, fromXIdx, fromYIdx);

    // Suppress unused-var noise from the index trackers.
    void fromXIdx; void fromYIdx;
  }

  function drawGuides(x: number | null, y: number | null, _xi: number, _yi: number) {
    if (!guidesLayer || !stage) return;
    guidesLayer.destroyChildren();
    const pageW = template.page.width * ppm;
    const pageH = template.page.height * ppm;

    if (x != null) {
      guidesLayer.add(new Konva.Line({
        points: [x, 0, x, pageH],
        stroke: "#f0399b",
        strokeWidth: 0.6,
        dash: [3, 3],
        listening: false,
      }));
    }
    if (y != null) {
      guidesLayer.add(new Konva.Line({
        points: [0, y, pageW, y],
        stroke: "#f0399b",
        strokeWidth: 0.6,
        dash: [3, 3],
        listening: false,
      }));
    }
    guidesLayer.batchDraw();
  }

  function clearGuides() {
    if (!guidesLayer) return;
    guidesLayer.destroyChildren();
    guidesLayer.batchDraw();
  }

  // ---------------------------------------------------------------------
  // Ctrl + wheel zoom
  // ---------------------------------------------------------------------

  function onWheel(e: WheelEvent) {
    if (!e.ctrlKey && !e.metaKey) return;
    e.preventDefault();
    onZoomDelta?.(-Math.sign(e.deltaY));
  }

  // ---------------------------------------------------------------------
  // Drag & drop from the tool palette
  // ---------------------------------------------------------------------

  const TOOL_MIME = "application/x-mg-tool";

  /** During `dragover`, `dataTransfer.getData()` returns "" by spec, so we
   *  must sniff the types array to know whether this drag is one of ours. */
  function isToolDrag(dt: DataTransfer | null): boolean {
    if (!dt) return false;
    const types = dt.types;
    if (!types) return false;
    // `DataTransfer.types` may be a real Array (Chromium) or a DOMStringList
    // (legacy) — convert to be safe.
    return Array.from(types as ArrayLike<string>).includes(TOOL_MIME);
  }

  function readKindFromDataTransfer(dt: DataTransfer | null): ElementType | null {
    if (!dt) return null;
    const raw = dt.getData(TOOL_MIME);
    if (!raw) return null;
    if (raw === "text" || raw === "image" || raw === "table" || raw === "rect") return raw;
    return null;
  }

  function onDragOver(e: DragEvent) {
    if (!isToolDrag(e.dataTransfer)) return;
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = "copy";

    const rect = host!.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const y = e.clientY - rect.top;
    // We don't know the kind yet (data is unreadable during dragover) so
    // use a neutral square ghost.
    const w = 60 * ppm;
    const h = 30 * ppm;
    dropHint = {
      x: x - w / 2,
      y: y - h / 2,
      w,
      h,
    };
  }

  function onDragLeave(e: DragEvent) {
    // Only clear if leaving the host itself, not a child element.
    if (e.target === host) dropHint = null;
  }

  function onDrop(e: DragEvent) {
    dropHint = null;
    const kind = readKindFromDataTransfer(e.dataTransfer);
    if (!kind) return;
    e.preventDefault();

    const rect = host!.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const y = e.clientY - rect.top;
    const ghost = ghostSizeFor(kind);
    const xMm = clampMm((x / ppm) - ghost.w / 2, 0, template.page.width  - ghost.w);
    const yMm = clampMm((y / ppm) - ghost.h / 2, 0, template.page.height - ghost.h);
    onDropElement?.(kind, roundMm(xMm), roundMm(yMm));
  }

  function ghostSizeFor(kind: ElementType): { w: number; h: number } {
    switch (kind) {
      case "text":  return { w: 80, h: 10 };
      case "image": return { w: 40, h: 25 };
      case "table": return { w: 180, h: 60 };
      case "rect":  return { w: 60, h: 30 };
    }
  }

  // ---------------------------------------------------------------------
  // Misc helpers
  // ---------------------------------------------------------------------

  function roundMm(n: number): number {
    return Math.round(n * 10) / 10;
  }

  function clampMm(n: number, lo: number, hi: number): number {
    if (hi < lo) return lo;
    return Math.min(hi, Math.max(lo, n));
  }

  function cssEscape(id: string): string {
    if (window.CSS?.escape) return window.CSS.escape(id);
    return id.replace(/[^a-zA-Z0-9_-]/g, "\\$&");
  }
</script>

<div class="a4stage" bind:this={host}>
  {#if dropHint}
    <div
      class="a4stage__drophint"
      style:left={`${dropHint.x}px`}
      style:top={`${dropHint.y}px`}
      style:width={`${dropHint.w}px`}
      style:height={`${dropHint.h}px`}
      aria-hidden="true"
    ></div>
  {/if}
</div>

<style>
  .a4stage {
    flex: 0 0 auto;
    position: relative;
    background: transparent;
  }
  .a4stage :global(canvas) { display: block; }

  .a4stage__drophint {
    position: absolute;
    background: var(--color-canvas-select);
    border: var(--border-w) dashed var(--color-canvas-select-ink);
    pointer-events: none;
    z-index: 2;
  }
</style>
