<script lang="ts">
  import { onMount, tick, untrack } from "svelte";
  import {
    Button,
    Icon,
    IconButton,
    Modal,
    Select,
    StateView,
    ToolbarButton,
    useTitleBarMenus,
    type SelectOption,
  } from "$lib/components";
  import {
    TEMPLATE_KIND_LABELS,
    createTemplate,
    updateTemplate,
    type Template as TemplateRow,
    type TemplateKind,
    type TemplateOrientation,
  } from "$lib/api/templates";
  import { ApiError } from "$lib/api/types";
  import { settingsStore } from "$lib/stores/settings.svelte";
  import {
    EDITOR_PX_PER_MM,
    PAGE_A4_LANDSCAPE,
    PAGE_A4_PORTRAIT,
    newImage,
    newRect,
    newTable,
    newText,
    type Element,
    type ElementType,
    type TextEl,
    type Template,
  } from "$lib/templates/schema";
  import { sampleDataFor } from "$lib/templates/sampleData";
  import { printTemplate, renderTemplateImage } from "$lib/templates/print";
  import A4Stage, { type A4StageApi, type ElementTransform } from "./A4Stage.svelte";
  import ToolPalette from "./ToolPalette.svelte";
  import VariablesPanel from "./VariablesPanel.svelte";
  import PropertiesPanel from "./PropertiesPanel.svelte";
  import LayersPanel from "./LayersPanel.svelte";
  import RichTextEditor, { type RichTextEditorApi } from "./RichTextEditor.svelte";
  import NewTemplateDialog from "./NewTemplateDialog.svelte";

  // =====================================================================
  // Props
  // =====================================================================

  interface Props {
    template: TemplateRow;
    onBack: () => void;
    onSaved: (saved: TemplateRow) => void;
  }

  let { template, onBack, onSaved }: Props = $props();

  // =====================================================================
  // Editing state
  // =====================================================================

  // Local mutable copy of the row's editable fields. We split metadata from
  // the visual schema so re-renders of the canvas only react to schema changes.
  // The `untrack` calls ensure we snapshot the initial prop value once and
  // do not reset the editor every time the parent re-passes the row.
  //
  // `templateId` doubles as the create-vs-update sentinel: an empty string
  // means the template only exists in memory (a draft) and the next save
  // must `POST` rather than `PATCH`. The parent (`Templates.svelte`)
  // builds a draft with `id: ""` when the user clicks "New template",
  // letting the editor open instantly without a backend round-trip.
  let templateId = $state(untrack(() => template.id));
  let name = $state(untrack(() => template.name));
  let kind = $state<TemplateKind>(untrack(() => template.kind));
  let orientation = $state<TemplateOrientation>(untrack(() => template.orientation));
  let schema = $state<Template>(untrack(() => deepCloneTemplate(template.schema)));

  const isDraft = $derived(templateId === "");

  // Metadata dialog (first-save + later renames go through the same UI).
  let metaDialogOpen = $state(false);
  let metaDialogBusy = $state(false);
  let metaDialogError = $state<string | null>(null);

  let selectedId = $state<string | null>(null);
  let zoom = $state(0.75);

  const ZOOM_MIN = 0.25;
  const ZOOM_MAX = 3;
  const ZOOM_STEPS = [0.25, 0.33, 0.5, 0.66, 0.75, 0.9, 1, 1.1, 1.25, 1.5, 2, 2.5, 3];

  let saving = $state(false);
  let saveError = $state<string | null>(null);
  let lastSavedAt = $state<string | null>(null);

  let stageApi: A4StageApi | null = null;
  let canvasScrollEl: HTMLDivElement | undefined = $state();

  // =====================================================================
  // Undo / redo
  // =====================================================================
  // Snapshot-based history. Each schema-mutating action pushes the *current*
  // schema onto `past` BEFORE applying its change. Snapshots that share a
  // `txKey` and happen within COALESCE_MS coalesce into a single entry so
  // typing a word counts as one undo step, not one per keystroke.

  interface HistoryEntry {
    schema: Template;
    txKey?: string;
    ts: number;
  }
  const MAX_HISTORY = 200;
  const COALESCE_MS = 500;

  let past = $state<HistoryEntry[]>([]);
  let future = $state<HistoryEntry[]>([]);
  const canUndo = $derived(past.length > 0);
  const canRedo = $derived(future.length > 0);

  function cloneT(t: Template): Template {
    return JSON.parse(JSON.stringify(t));
  }

  function snapshot(txKey?: string) {
    const now = Date.now();
    if (txKey && past.length > 0) {
      const last = past[past.length - 1];
      if (last.txKey === txKey && now - last.ts < COALESCE_MS) {
        last.ts = now;
        return; // already have a "before" snapshot for this transaction
      }
    }
    const entry: HistoryEntry = { schema: cloneT(schema), txKey, ts: now };
    past = past.length >= MAX_HISTORY
      ? [...past.slice(1), entry]
      : [...past, entry];
    if (future.length > 0) future = [];
  }

  function undo() {
    if (past.length === 0) return;
    const prev = past[past.length - 1];
    future = [{ schema: cloneT(schema), ts: Date.now() }, ...future];
    schema = cloneT(prev.schema);
    past = past.slice(0, -1);
    if (selectedId && !schema.elements.some((e) => e.id === selectedId)) {
      selectedId = null;
    }
  }

  function redo() {
    if (future.length === 0) return;
    const next = future[0];
    past = [...past, { schema: cloneT(schema), ts: Date.now() }];
    schema = cloneT(next.schema);
    future = future.slice(1);
    if (selectedId && !schema.elements.some((e) => e.id === selectedId)) {
      selectedId = null;
    }
  }

  /** Returns the first field that differs between `a` and `b`, or `null`. */
  function diffKey(a: Element, b: Element): string | null {
    const keys = new Set<string>([...Object.keys(a), ...Object.keys(b)]);
    for (const k of keys) {
      const av = (a as unknown as Record<string, unknown>)[k];
      const bv = (b as unknown as Record<string, unknown>)[k];
      if (av === bv) continue;
      const aIsObj = av !== null && typeof av === "object";
      const bIsObj = bv !== null && typeof bv === "object";
      if (aIsObj || bIsObj) {
        if (JSON.stringify(av) !== JSON.stringify(bv)) return k;
      } else {
        return k;
      }
    }
    return null;
  }

  // Sample data is computed from settings + the current kind. The editor
  // shows interpolated content so the user sees how the template will look
  // once filled with a real document.
  const sampleData = $derived(sampleDataFor(kind));

  // Wrap the user-facing schema so we keep orientation in sync. Pushed onto
  // history so undo reverts both the orientation control AND the page size.
  $effect(() => {
    const wanted = orientation === "portrait" ? PAGE_A4_PORTRAIT : PAGE_A4_LANDSCAPE;
    if (schema.page.width !== wanted.width || schema.page.height !== wanted.height) {
      untrack(() => snapshot("orientation"));
      schema = { ...schema, page: { ...wanted, margin: schema.page.margin } };
    }
  });

  // =====================================================================
  // Title bar menus
  // =====================================================================

  useTitleBarMenus(() => [
    {
      id: "file",
      label: "Fichier",
      items: [
        { label: "Enregistrer",          icon: "save",   shortcut: "Ctrl+S", onSelect: save },
        { label: "Propriétés du modèle…", icon: "pencil", onSelect: openMetadataDialog },
        { divider: true },
        { label: "Aperçu",               icon: "eye",    shortcut: "Ctrl+P", onSelect: openPreview },
        { divider: true },
        { label: "Retour à la liste",    icon: "arrow-left", onSelect: onBack },
      ],
    },
    {
      id: "edit",
      label: "Édition",
      items: [
        { label: "Annuler",     icon: "undo",  shortcut: "Ctrl+Z",       disabled: !canUndo, onSelect: undo },
        { label: "Rétablir",    icon: "redo",  shortcut: "Ctrl+Shift+Z", disabled: !canRedo, onSelect: redo },
        { divider: true },
        { label: "Dupliquer",   icon: "copy",  shortcut: "Ctrl+D", disabled: !selectedId, onSelect: duplicateSelected },
        { label: "Copier",                     shortcut: "Ctrl+C", disabled: !selectedId, onSelect: copySelected },
        { label: "Coller",                     shortcut: "Ctrl+V", disabled: !clipboard,  onSelect: pasteClipboard },
        { label: "Supprimer",   icon: "trash", shortcut: "Suppr",  disabled: !selectedId, onSelect: deleteSelected },
      ],
    },
    {
      id: "insert",
      label: "Insertion",
      items: [
        { label: "Texte",   icon: "type",   onSelect: () => addElement("text")  },
        { label: "Image",   icon: "image",  onSelect: () => addElement("image") },
        { label: "Tableau", icon: "table",  onSelect: () => addElement("table") },
        { label: "Cadre",   icon: "square", onSelect: () => addElement("rect")  },
      ],
    },
  ]);

  // =====================================================================
  // Lifecycle
  // =====================================================================

  onMount(() => {
    // Refresh settings so the live preview reflects the latest shop identity.
    if (settingsStore.loadedAt == null) {
      void settingsStore.load().catch(() => undefined);
    }
  });

  // =====================================================================
  // Editor actions
  // =====================================================================

  const selectedElement = $derived(
    schema.elements.find((e) => e.id === selectedId) ?? null,
  );

  function addElement(type: ElementType) {
    snapshot();
    const m = schema.page.margin;
    let el: Element;
    switch (type) {
      case "text":  el = newText(m, m + Math.random() * 10); break;
      case "image": el = newImage(m, m + Math.random() * 10); break;
      case "table": el = newTable(m, schema.page.height / 2); break;
      case "rect":  el = newRect(m, m + Math.random() * 10); break;
    }
    schema = { ...schema, elements: [...schema.elements, el] };
    selectedId = el.id;
  }

  function dropElement(type: ElementType, xMm: number, yMm: number) {
    snapshot();
    let el: Element;
    switch (type) {
      case "text":  el = newText(xMm, yMm); break;
      case "image": el = newImage(xMm, yMm); break;
      case "table": el = newTable(xMm, yMm); break;
      case "rect":  el = newRect(xMm, yMm); break;
    }
    schema = { ...schema, elements: [...schema.elements, el] };
    selectedId = el.id;
  }

  function updateElement(el: Element) {
    // Field-aware txKey so distinct field edits stay separate even when
    // they happen in quick succession; typing on the same field coalesces.
    const prev = schema.elements.find((x) => x.id === el.id);
    const changed = prev ? diffKey(prev, el) : null;
    snapshot(changed ? `update:${el.id}:${changed}` : `update:${el.id}`);
    schema = {
      ...schema,
      elements: schema.elements.map((x) => (x.id === el.id ? el : x)),
    };
  }

  function transformElement(id: string, t: ElementTransform) {
    snapshot();
    schema = {
      ...schema,
      elements: schema.elements.map((x2) => {
        if (x2.id !== id) return x2;
        return {
          ...x2,
          x: t.x,
          y: t.y,
          w: t.w ?? x2.w,
          h: t.h ?? x2.h,
        } as Element;
      }),
    };
  }

  function deleteSelected() {
    if (!selectedId) return;
    snapshot();
    schema = {
      ...schema,
      elements: schema.elements.filter((x) => x.id !== selectedId),
    };
    selectedId = null;
  }

  // ===== Layer (z-order) ops =========================================
  // Elements are rendered in array order; last element is on top.

  function moveLayer(delta: number) {
    if (!selectedId) return;
    const idx = schema.elements.findIndex((e) => e.id === selectedId);
    if (idx < 0) return;
    const target = Math.max(0, Math.min(schema.elements.length - 1, idx + delta));
    if (target === idx) return;
    snapshot();
    const next = schema.elements.slice();
    const [moved] = next.splice(idx, 1);
    next.splice(target, 0, moved);
    schema = { ...schema, elements: next };
  }
  function moveLayerTo(target: "front" | "back") {
    if (!selectedId) return;
    const idx = schema.elements.findIndex((e) => e.id === selectedId);
    if (idx < 0) return;
    snapshot();
    const next = schema.elements.slice();
    const [moved] = next.splice(idx, 1);
    if (target === "front") next.push(moved);
    else next.unshift(moved);
    schema = { ...schema, elements: next };
  }
  function reorderLayers(fromId: string, toId: string, position: "before" | "after") {
    const fromIdx = schema.elements.findIndex((e) => e.id === fromId);
    const toIdx   = schema.elements.findIndex((e) => e.id === toId);
    if (fromIdx < 0 || toIdx < 0 || fromIdx === toIdx) return;
    snapshot();
    const next = schema.elements.slice();
    const [moved] = next.splice(fromIdx, 1);
    const insertAt = next.findIndex((e) => e.id === toId);
    next.splice(position === "before" ? insertAt : insertAt + 1, 0, moved);
    schema = { ...schema, elements: next };
  }

  // ===== Clipboard / duplicate =======================================

  let clipboard = $state<Element | null>(null);

  function duplicateSelected() {
    if (!selectedId) return;
    const el = schema.elements.find((e) => e.id === selectedId);
    if (!el) return;
    snapshot();
    const copy = cloneElement(el, 4, 4);
    schema = { ...schema, elements: [...schema.elements, copy] };
    selectedId = copy.id;
  }

  function copySelected() {
    if (!selectedId) return;
    const el = schema.elements.find((e) => e.id === selectedId);
    if (!el) return;
    clipboard = el; // copy doesn't mutate schema, no snapshot needed
  }

  function pasteClipboard() {
    if (!clipboard) return;
    snapshot();
    const copy = cloneElement(clipboard, 4, 4);
    schema = { ...schema, elements: [...schema.elements, copy] };
    selectedId = copy.id;
  }

  function cloneElement(el: Element, dx: number, dy: number): Element {
    const copy = JSON.parse(JSON.stringify(el)) as Element;
    copy.id = el.type === "text"  ? `txt_${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 5)}`
            : el.type === "image" ? `img_${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 5)}`
            : el.type === "table" ? `tbl_${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 5)}`
            : `rct_${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 5)}`;
    copy.x = Math.min(schema.page.width  - copy.w, Math.max(0, el.x + dx));
    copy.y = Math.min(schema.page.height - copy.h, Math.max(0, el.y + dy));
    return copy;
  }

  // ===== Quick text-align toggle =====================================

  function setTextAlign(align: "left" | "center" | "right") {
    if (!selectedId) return;
    const el = schema.elements.find((e) => e.id === selectedId);
    if (!el || el.type !== "text") return;
    updateElement({ ...(el as TextEl), align });
  }

  function toggleBold() {
    if (!selectedId) return;
    const el = schema.elements.find((e) => e.id === selectedId);
    if (!el || el.type !== "text") return;
    const t = el as TextEl;
    updateElement({ ...t, fontWeight: t.fontWeight === "bold" ? "normal" : "bold" });
  }

  // ===== Arrow-key nudging ===========================================

  function nudge(dx: number, dy: number) {
    if (!selectedId) return;
    const el = schema.elements.find((e) => e.id === selectedId);
    if (!el) return;
    updateElement({
      ...el,
      x: Math.max(0, Math.min(schema.page.width  - el.w, el.x + dx)),
      y: Math.max(0, Math.min(schema.page.height - el.h, el.y + dy)),
    } as Element);
  }

  // --- Rich-text editor binding ---------------------------------------
  // Two RichTextEditor instances may exist: one in the properties panel
  // and one on the canvas overlay. The most recently focused one becomes
  // the "active" target for variable insertion.

  let panelEditor: RichTextEditorApi | null = null;
  let overlayEditor: RichTextEditorApi | null = null;
  let activeEditor: RichTextEditorApi | null = null;

  function insertVariable(token: string) {
    if (!selectedId) return;
    const target = schema.elements.find((e) => e.id === selectedId);
    if (!target || target.type !== "text") return;

    // Prefer cursor insertion via the currently focused rich editor.
    if (activeEditor) {
      activeEditor.insertToken(token);
      return;
    }

    // Fallback: append to the content.
    const next: TextEl = {
      ...(target as TextEl),
      content: (target as TextEl).content.length > 0
        ? `${(target as TextEl).content} ${token}`
        : token,
    };
    updateElement(next);
  }

  // ----- Inline text editing overlay -----------------------------------

  let textOverlay = $state<{ id: string; x: number; y: number; w: number; h: number } | null>(null);

  function openTextEditor(id: string, rect: { x: number; y: number; w: number; h: number }) {
    const el = schema.elements.find((e) => e.id === id);
    if (!el || el.type !== "text") return;
    textOverlay = { id, ...rect };
    queueMicrotask(() => overlayEditor?.focus());
  }

  function commitTextEditor() {
    if (!textOverlay || !overlayEditor) {
      textOverlay = null;
      return;
    }
    const id = textOverlay.id;
    const el = schema.elements.find((e) => e.id === id) as TextEl | undefined;
    const val = overlayEditor.getValue();
    if (el) updateElement({ ...el, content: val });
    textOverlay = null;
  }

  function cancelTextEditor() {
    textOverlay = null;
  }

  // Close the overlay when selection moves to a different element.
  $effect(() => {
    if (textOverlay && selectedId !== textOverlay.id) {
      untrack(() => { textOverlay = null; });
    }
  });

  // Click-outside-to-commit. The variables / properties panels are explicit
  // "safe" zones so users can click a variable badge to insert it without
  // dismissing the overlay first.
  function onGlobalMouseDown(e: MouseEvent) {
    if (!textOverlay) return;
    const t = e.target as HTMLElement | null;
    if (!t) return;
    if (t.closest(".editor__textoverlay")) return;
    if (t.closest('[data-keep-overlay="1"]')) return;
    commitTextEditor();
  }

  // =====================================================================
  // Save / preview
  // =====================================================================

  /** Capture a thumbnail of the current document, returning null if the
   *  off-screen render fails (so the save still goes through). */
  async function captureThumbnail(): Promise<string | null> {
    try {
      return await renderTemplateImage(schema, sampleData, {
        pxPerMm: 2,
        showGrid: false,
      });
    } catch (err) {
      console.warn("[editor] thumbnail capture failed:", err);
      return null;
    }
  }

  async function save() {
    // First save of a draft: collect name + type before we can POST.
    if (isDraft) {
      if (!name.trim()) {
        metaDialogError = null;
        metaDialogOpen = true;
        return;
      }
      await submitMetadata({ name: name.trim(), kind });
      return;
    }
    saving = true;
    saveError = null;
    try {
      const thumbnail = await captureThumbnail();
      const saved = await updateTemplate(templateId, {
        name: name.trim(),
        kind,
        page_size: template.page_size,
        orientation,
        schema,
        thumbnail_data_url: thumbnail,
        is_default: template.is_default,
        is_archived: template.is_archived,
      });
      lastSavedAt = new Date().toISOString();
      onSaved(saved);
    } catch (err) {
      saveError = humanizeError(err, "Enregistrement impossible.");
    } finally {
      saving = false;
    }
  }

  /** Dialog submit handler. In draft mode it POSTs a new template and
   *  promotes the editor out of draft state; in normal mode it just
   *  updates the local metadata (next regular save persists the change). */
  async function submitMetadata(input: { name: string; kind: TemplateKind }) {
    metaDialogBusy = true;
    metaDialogError = null;
    try {
      if (isDraft) {
        const thumbnail = await captureThumbnail();
        const created = await createTemplate({
          name: input.name,
          kind: input.kind,
          page_size: template.page_size,
          orientation,
          schema,
          thumbnail_data_url: thumbnail,
        });
        templateId = created.id;
        name = input.name;
        kind = input.kind;
        lastSavedAt = new Date().toISOString();
        metaDialogOpen = false;
        onSaved(created);
      } else {
        name = input.name;
        kind = input.kind;
        metaDialogOpen = false;
        await save();
      }
    } catch (err) {
      metaDialogError = humanizeError(err, "Enregistrement impossible.");
    } finally {
      metaDialogBusy = false;
    }
  }

  function openMetadataDialog() {
    metaDialogError = null;
    metaDialogOpen = true;
  }

  // ----- Preview --------------------------------------------------------

  let previewOpen = $state(false);
  let previewLoading = $state(false);
  let previewImage = $state<string | null>(null);

  async function openPreview() {
    previewOpen = true;
    previewLoading = true;
    previewImage = null;
    try {
      const img = await renderTemplateImage(schema, sampleData, {
        pxPerMm: 3,
        showGrid: false,
      });
      previewImage = img;
    } catch (err) {
      console.warn("[preview] failed:", err);
      previewImage = null;
    } finally {
      previewLoading = false;
    }
  }

  async function printFromPreview() {
    try {
      await printTemplate(schema, sampleData);
    } catch (err) {
      console.warn("[print] failed:", err);
    }
  }

  // =====================================================================
  // Keyboard shortcuts
  // =====================================================================

  function isEditingText(target: EventTarget | null): boolean {
    const t = target as HTMLElement | null;
    if (!t) return false;
    if (t.isContentEditable) return true;
    const tag = t.tagName?.toLowerCase();
    return tag === "input" || tag === "textarea" || tag === "select";
  }

  function onKeydown(e: KeyboardEvent) {
    if (textOverlay) {
      // Undo / redo must still work even while the inline overlay is open —
      // user expectation is that Ctrl+Z is always available.
      const mod = e.ctrlKey || e.metaKey;
      if (mod && e.key.toLowerCase() === "z") { e.preventDefault(); e.shiftKey ? redo() : undo(); return; }
      if (mod && e.key.toLowerCase() === "y") { e.preventDefault(); redo(); return; }
      return;
    }
    const mod = e.ctrlKey || e.metaKey;

    if (mod && e.key.toLowerCase() === "s") { e.preventDefault(); void save(); return; }
    if (mod && e.key.toLowerCase() === "p") { e.preventDefault(); void openPreview(); return; }
    if (e.key === "Escape") { selectedId = null; return; }

    // Undo / redo work even when a form field has focus (matches OS conventions).
    if (mod && e.key.toLowerCase() === "z") { e.preventDefault(); e.shiftKey ? redo() : undo(); return; }
    if (mod && e.key.toLowerCase() === "y") { e.preventDefault(); redo(); return; }

    if (isEditingText(e.target)) return;  // don't hijack while typing in a form field

    // Clipboard / duplicate
    if (mod && e.key.toLowerCase() === "c" && selectedId) { e.preventDefault(); copySelected();    return; }
    if (mod && e.key.toLowerCase() === "v")               { e.preventDefault(); pasteClipboard();  return; }
    if (mod && e.key.toLowerCase() === "d" && selectedId) { e.preventDefault(); duplicateSelected(); return; }

    // Layer order: Ctrl + ] / [
    if (mod && e.key === "]" && selectedId) { e.preventDefault(); moveLayer( 1); return; }
    if (mod && e.key === "[" && selectedId) { e.preventDefault(); moveLayer(-1); return; }

    // Delete
    if ((e.key === "Delete" || e.key === "Backspace") && selectedId) {
      e.preventDefault();
      deleteSelected();
      return;
    }

    // Arrow-nudge (1mm normal, 10mm with Shift)
    if (selectedId && (e.key === "ArrowLeft" || e.key === "ArrowRight" || e.key === "ArrowUp" || e.key === "ArrowDown")) {
      const step = e.shiftKey ? 10 : 1;
      e.preventDefault();
      if (e.key === "ArrowLeft")  nudge(-step, 0);
      if (e.key === "ArrowRight") nudge( step, 0);
      if (e.key === "ArrowUp")    nudge(0, -step);
      if (e.key === "ArrowDown")  nudge(0,  step);
    }
  }

  // =====================================================================
  // Misc helpers
  // =====================================================================

  function deepCloneTemplate(t: Template | undefined | null): Template {
    if (!t) {
      return {
        version: 1,
        page: { ...PAGE_A4_PORTRAIT },
        elements: [],
      };
    }
    return JSON.parse(JSON.stringify(t));
  }

  function humanizeError(err: unknown, fallback: string): string {
    if (err instanceof ApiError) {
      if (err.details?.length) return err.details.join("\n");
      return err.message || fallback;
    }
    if (err instanceof TypeError) return "Connexion au serveur impossible.";
    return fallback;
  }

  const orientationOptions: SelectOption<TemplateOrientation>[] = [
    { value: "portrait",  label: "Portrait"  },
    { value: "landscape", label: "Paysage"   },
  ];

  function zoomBy(direction: number) {
    if (direction === 0) return;
    let target: number;
    if (direction > 0) {
      const next = ZOOM_STEPS.find((s) => s > zoom + 1e-6);
      target = next ?? ZOOM_MAX;
    } else {
      const next = [...ZOOM_STEPS].reverse().find((s) => s < zoom - 1e-6);
      target = next ?? ZOOM_MIN;
    }
    setZoomKeepingCenter(target);
  }

  function zoomToFit() {
    setZoomKeepingCenter(0.75);
  }

  /** Apply a new zoom while keeping the document's visible center pinned
   *  to the same point of the viewport. Works in both regimes — document
   *  fits inside the viewport (flex-centered) and document overflows
   *  (scroll-anchored) — because we operate on the post-layout
   *  bounding-rect delta rather than on assumed scrollable geometry.
   *
   *  Burst-safe: when many wheel events arrive faster than `tick()`
   *  resolves, only the FIRST call in the burst snapshots the original
   *  geometry and only the LAST call applies the scroll compensation.
   *  This prevents partial / overlapping snapshots from drifting the
   *  document on rapid zoom-out. */
  let zoomBurstSnapshot: { dcX: number; dcY: number } | null = null;
  let zoomCompensationToken = 0;

  async function setZoomKeepingCenter(nextZoom: number) {
    if (nextZoom === zoom) return;
    const scrollEl = canvasScrollEl;
    if (!scrollEl) {
      zoom = nextZoom;
      return;
    }
    const stageWrap = scrollEl.querySelector<HTMLElement>(".editor__stage-wrap");
    if (!stageWrap) {
      zoom = nextZoom;
      return;
    }

    if (zoomBurstSnapshot == null) {
      const vp = scrollEl.getBoundingClientRect();
      const dr = stageWrap.getBoundingClientRect();
      zoomBurstSnapshot = {
        dcX: dr.left + dr.width  / 2 - vp.left,
        dcY: dr.top  + dr.height / 2 - vp.top,
      };
    }

    zoom = nextZoom;
    const myToken = ++zoomCompensationToken;
    await tick();
    if (myToken !== zoomCompensationToken) return;

    const snap = zoomBurstSnapshot;
    zoomBurstSnapshot = null;
    if (!snap) return;

    const wrap = scrollEl.querySelector<HTMLElement>(".editor__stage-wrap");
    if (!wrap) return;
    const vp = scrollEl.getBoundingClientRect();
    const dr = wrap.getBoundingClientRect();
    const newDcX = dr.left + dr.width  / 2 - vp.left;
    const newDcY = dr.top  + dr.height / 2 - vp.top;

    scrollEl.scrollLeft += newDcX - snap.dcX;
    scrollEl.scrollTop  += newDcY - snap.dcY;
  }

  function fmtSavedAt(iso: string | null): string {
    if (!iso) return "Pas encore enregistré";
    const d = new Date(iso);
    return `Enregistré à ${d.toLocaleTimeString("fr-FR", { hour: "2-digit", minute: "2-digit" })}`;
  }
</script>

<svelte:window onkeydown={onKeydown} onmousedown={onGlobalMouseDown} />

<div class="editor">
  <!-- ============== TOP BAR ============================================ -->
  <header class="editor__top">
    <div class="editor__top-left">
      <IconButton title="Retour à la liste" onclick={onBack}>
        <Icon name="arrow-left" size={14} />
      </IconButton>
      <span class="editor__top-sep" aria-hidden="true"></span>
      <ToolPalette compact onAdd={addElement} />
      <span class="editor__top-sep" aria-hidden="true"></span>
      <ToolbarButton
        title="Modifier le nom et le type du modèle"
        label={`${name.trim() ? name : "Nouveau modèle"} · ${TEMPLATE_KIND_LABELS[kind]}${isDraft ? " · brouillon" : ""}`}
        icon="pencil"
        onclick={openMetadataDialog}
      />
    </div>

    <div class="editor__top-right">
      <div class="editor__history" role="group" aria-label="Historique">
        <IconButton title="Annuler (Ctrl+Z)"   disabled={!canUndo} onclick={undo}>
          <Icon name="undo" size={13} />
        </IconButton>
        <IconButton title="Rétablir (Ctrl+Shift+Z)" disabled={!canRedo} onclick={redo}>
          <Icon name="redo" size={13} />
        </IconButton>
      </div>
      {#if saveError}
        <span class="editor__saveerr" role="alert">{saveError}</span>
      {:else}
        <span class="editor__savemeta">{fmtSavedAt(lastSavedAt)}</span>
      {/if}
      <Button variant="secondary" onclick={openPreview}>
        <Icon name="eye" size={13} /> Aperçu
      </Button>
      <Button variant="primary" loading={saving} onclick={save}>
        <Icon name="save" size={13} /> Enregistrer
      </Button>
    </div>
  </header>

  <!-- ============== BODY ============================================== -->
  <div class="editor__body">
    <!-- Left column -->
    <aside class="editor__col editor__col--left" aria-label="Variables" data-keep-overlay="1">
      <VariablesPanel
        {kind}
        canInsert={selectedElement?.type === "text"}
        onInsert={insertVariable}
      />
    </aside>

    <!-- Canvas -->
    <main class="editor__canvas" aria-label="Page A4">
      <div class="editor__canvas-toolbar">
        <div class="editor__canvas-meta">
          <span class="editor__page-label">A4 · {schema.page.width} × {schema.page.height} mm</span>
          <div class="editor__orientation">
            <Select bind:value={orientation} options={orientationOptions} />
          </div>
        </div>

        {#if selectedElement}
          <div class="editor__ctxbar" role="toolbar" aria-label="Actions sur l'élément">
            {#if selectedElement.type === "text"}
              <div class="editor__ctxgroup">
                <IconButton title="Aligner à gauche" active={(selectedElement as TextEl).align === "left"}   onclick={() => setTextAlign("left")}>
                  <Icon name="align-left" size={13} />
                </IconButton>
                <IconButton title="Centrer"            active={(selectedElement as TextEl).align === "center"} onclick={() => setTextAlign("center")}>
                  <Icon name="align-center" size={13} />
                </IconButton>
                <IconButton title="Aligner à droite" active={(selectedElement as TextEl).align === "right"}  onclick={() => setTextAlign("right")}>
                  <Icon name="align-right" size={13} />
                </IconButton>
                <IconButton title="Gras" active={(selectedElement as TextEl).fontWeight === "bold"} onclick={toggleBold}>
                  <span class="editor__bold">B</span>
                </IconButton>
              </div>
              <span class="editor__ctxsep" aria-hidden="true"></span>
            {/if}
            <div class="editor__ctxgroup">
              <IconButton title="Mettre au premier plan"           onclick={() => moveLayerTo("front")}><Icon name="to-front"   size={13} /></IconButton>
              <IconButton title="Avancer (Ctrl+])"                  onclick={() => moveLayer( 1)}><Icon name="layer-up"    size={13} /></IconButton>
              <IconButton title="Reculer (Ctrl+[)"                  onclick={() => moveLayer(-1)}><Icon name="layer-down"  size={13} /></IconButton>
              <IconButton title="Mettre à l'arrière-plan"           onclick={() => moveLayerTo("back")}><Icon name="to-back" size={13} /></IconButton>
            </div>
            <span class="editor__ctxsep" aria-hidden="true"></span>
            <div class="editor__ctxgroup">
              <IconButton title="Dupliquer (Ctrl+D)" onclick={duplicateSelected}><Icon name="copy"  size={13} /></IconButton>
              <IconButton title="Supprimer"           variant="danger" onclick={deleteSelected}><Icon name="trash" size={13} /></IconButton>
            </div>
          </div>
        {/if}

        <div class="editor__zoom" role="group" aria-label="Zoom">
          <IconButton title="Zoom arrière" onclick={() => zoomBy(-1)}>
            <Icon name="minus" size={12} />
          </IconButton>
          <ToolbarButton
            title="Ajuster le zoom à la page"
            label={`${Math.round(zoom * 100)} %`}
            onclick={zoomToFit}
          />
          <IconButton title="Zoom avant" onclick={() => zoomBy(1)}>
            <Icon name="plus" size={12} />
          </IconButton>
          <span class="editor__zoom-hint">Ctrl + molette</span>
        </div>
      </div>
      <div class="editor__canvas-scroll" bind:this={canvasScrollEl}>
        <div class="editor__canvas-pad">
          <div class="editor__stage-wrap" style:width={`${schema.page.width * EDITOR_PX_PER_MM * zoom}px`} style:height={`${schema.page.height * EDITOR_PX_PER_MM * zoom}px`}>
            <A4Stage
              template={schema}
              data={sampleData}
              {selectedId}
              {zoom}
              onTransformElement={transformElement}
              onSelectElement={(id) => (selectedId = id)}
              onEditText={openTextEditor}
              onZoomDelta={zoomBy}
              onDropElement={dropElement}
              onReady={(api) => (stageApi = api)}
            />

            {#if textOverlay}
              {@const overlayEl = schema.elements.find((e) => e.id === textOverlay!.id) as TextEl | undefined}
              <div
                class="editor__textoverlay"
                style:left={`${textOverlay.x}px`}
                style:top={`${textOverlay.y}px`}
                style:min-width={`${Math.max(120, textOverlay.w)}px`}
                style:min-height={`${textOverlay.h}px`}
              >
                <RichTextEditor
                  flush
                  value={overlayEl?.content ?? ""}
                  rows={Math.max(1, Math.floor(textOverlay.h / 16))}
                  placeholder="Texte ou variable"
                  onChange={(v) => {
                    const el = schema.elements.find((e) => e.id === textOverlay!.id) as TextEl | undefined;
                    if (el) updateElement({ ...el, content: v });
                  }}
                  onSubmit={commitTextEditor}
                  onCancel={cancelTextEditor}
                  onReady={(api) => { overlayEditor = api; activeEditor = api; }}
                  onFocus={() => { activeEditor = overlayEditor; }}
                />
              </div>
            {/if}
          </div>
        </div>
      </div>
    </main>

    <!-- Right column -->
    <aside class="editor__col editor__col--right" aria-label="Calques et propriétés">
      <LayersPanel
        elements={schema.elements}
        {selectedId}
        onSelect={(id) => (selectedId = id)}
        onReorder={reorderLayers}
        onDuplicate={(id) => { selectedId = id; duplicateSelected(); }}
        onDelete={(id) => { selectedId = id; deleteSelected(); }}
      />
      <div class="editor__props">
        <PropertiesPanel
          element={selectedElement}
          {kind}
          onChange={updateElement}
          onDelete={deleteSelected}
          onContentEditorReady={(api) => { panelEditor = api; if (api) activeEditor = api; }}
          onContentEditorFocus={(f) => { if (f) activeEditor = panelEditor; }}
        />
      </div>
    </aside>
  </div>
</div>

<Modal
  open={metaDialogOpen}
  title={isDraft ? "Enregistrer le modèle" : "Propriétés du modèle"}
  width="480px"
  onClose={() => !metaDialogBusy && (metaDialogOpen = false)}
>
  {#if metaDialogOpen}
    <NewTemplateDialog
      submitting={metaDialogBusy}
      errorMessage={metaDialogError}
      initialName={name}
      initialKind={kind}
      submitLabel={isDraft ? "Créer et enregistrer" : "Mettre à jour"}
      onSubmit={submitMetadata}
      onCancel={() => (metaDialogOpen = false)}
    />
  {/if}
</Modal>

<Modal
  open={previewOpen}
  title="Aperçu du modèle"
  width="720px"
  onClose={() => (previewOpen = false)}
>
  {#if previewOpen}
    <div class="editor__preview">
      {#if previewLoading}
        <StateView kind="loading" compact title="Génération de l'aperçu…" />
      {:else if previewImage}
        <img class="editor__preview-img" src={previewImage} alt="Aperçu" />
      {:else}
        <StateView
          kind="empty"
          compact
          icon="image"
          title="Aperçu indisponible"
          body="Enregistrez le modèle puis réessayez."
        />
      {/if}

      <div class="editor__preview-actions">
        <span class="editor__preview-hint">
          Les variables sont remplacées par des valeurs d'exemple basées sur vos paramètres.
        </span>
        <Button variant="secondary" onclick={printFromPreview}>
          <Icon name="receipt" size={13} /> Imprimer un exemple
        </Button>
      </div>
    </div>
  {/if}
</Modal>

<style>
  .editor {
    display: flex;
    flex-direction: column;
    flex: 1 1 auto;
    min-height: 0;
    min-width: 0;
    background: var(--color-bg-app);
  }

  /* ============== Top bar ============================================ */
  .editor__top {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    padding: var(--space-2) var(--space-3);
    background: var(--color-bg-surface);
    border-bottom: var(--border-w) solid var(--color-border);
    flex: 0 0 auto;
  }
  .editor__top-left,
  .editor__top-right {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
  }
  .editor__top-left { flex: 1 1 auto; overflow: hidden; }
  .editor__top-sep {
    width: 1px;
    align-self: stretch;
    background: var(--color-border-subtle);
    margin: 0 2px;
  }
  /* Clickable title that doubles as a "rename" affordance. Renders as a
   * borderless pill on hover so it doesn't clutter the toolbar at rest. */
  .editor__history {
    display: inline-flex;
    align-items: center;
    gap: 0;
    border: var(--border-w) solid var(--color-border-subtle);
    padding: 1px;
  }
  .editor__savemeta {
    font-size: var(--fs-xs);
    color: var(--color-text-muted);
  }
  .editor__saveerr {
    font-size: var(--fs-xs);
    color: var(--color-danger);
    max-width: 240px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* ============== Body / 3-col layout =============================== */
  .editor__body {
    flex: 1 1 auto;
    display: grid;
    grid-template-columns: 220px 1fr 280px;
    min-height: 0;
  }
  .editor__col {
    background: var(--color-bg-surface);
    display: flex;
    flex-direction: column;
    min-height: 0;
    overflow: hidden;
  }
  .editor__col--left  { border-right: var(--border-w) solid var(--color-border); }
  .editor__col--right { border-left:  var(--border-w) solid var(--color-border); }
  .editor__props {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
  }

  /* ============== Canvas ============================================ */
  .editor__canvas {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    background: var(--color-bg-app);
  }
  .editor__canvas-toolbar {
    flex: 0 0 auto;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    padding: var(--space-1) var(--space-3);
    background: var(--color-bg-surface);
    border-bottom: var(--border-w) solid var(--color-border-subtle);
  }
  .editor__canvas-meta {
    display: inline-flex;
    align-items: center;
    gap: var(--space-3);
  }
  .editor__page-label {
    font-size: var(--fs-xs);
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: var(--tracking-caps);
  }
  .editor__orientation :global(.sel-wrap) { width: 140px; }
  .editor__ctxbar {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    margin-left: auto;
    margin-right: var(--space-3);
  }
  .editor__ctxgroup {
    display: inline-flex;
    align-items: center;
    gap: 2px;
  }
  .editor__ctxsep {
    width: 1px;
    height: 18px;
    background: var(--color-border);
  }
  .editor__bold {
    font-weight: var(--fw-bold);
    font-size: var(--fs-xs);
    line-height: 1;
    color: inherit;
  }

  .editor__zoom {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
  }
  .editor__zoom-hint {
    font-size: var(--fs-xs);
    color: var(--color-text-muted);
    margin-left: var(--space-2);
  }

  .editor__canvas-scroll {
    flex: 1 1 auto;
    overflow: auto;
    min-height: 0;
  }
  .editor__canvas-pad {
    min-width: max-content;
    min-height: 100%;
    padding: var(--space-5);
    display: flex;
    /* `safe center` keeps the document visually centered along both axes
     * when it fits inside the viewport, and falls back to start-anchored
     * when it overflows so the user can scroll to its top-left corner. */
    align-items: safe center;
    justify-content: safe center;
  }
  .editor__stage-wrap {
    position: relative;
    background: var(--color-paper);
    flex-shrink: 0;
    box-shadow: var(--shadow-page);
  }
  .editor__textoverlay {
    position: absolute;
    z-index: var(--z-overlay);
    font-size: var(--fs-md);
    max-width: min(70%, 540px);
  }

  /* ============== Preview modal ===================================== */
  .editor__preview {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .editor__preview-img {
    width: 100%;
    height: auto;
    background: var(--color-paper);
    box-shadow: var(--shadow-float);
  }
  .editor__preview-actions {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    padding-top: var(--space-2);
  }
  .editor__preview-hint {
    font-size: var(--fs-xs);
    color: var(--color-text-muted);
    flex: 1 1 auto;
  }

  /* ============== Spinner ============================================ */
</style>
