<script lang="ts" module>
  /** Imperative API exposed via `onReady`. */
  export interface RichTextEditorApi {
    /** Insert a `{{path|hint}}` token at the current caret (focuses first). */
    insertToken: (token: string) => void;
    /** Move focus into the editor. */
    focus: () => void;
    /** Return the raw template string (with `{{tokens}}`). */
    getValue: () => string;
  }
</script>

<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { tokenize } from "$lib/templates/interpolate";
  import { humanLabelForPath } from "$lib/templates/variables";

  interface Props {
    /** Raw template string (with `{{tokens}}`). */
    value: string;
    onChange?: (value: string) => void;
    onFocus?: () => void;
    onBlur?: () => void;
    /** Ctrl/Cmd+Enter triggers this (used by the inline overlay to commit). */
    onSubmit?: () => void;
    /** Escape triggers this (used by the inline overlay to cancel). */
    onCancel?: () => void;
    placeholder?: string;
    multiline?: boolean;
    rows?: number;
    /** Compact mode is used for the inline overlay (no internal border). */
    flush?: boolean;
    onReady?: (api: RichTextEditorApi) => void;
  }

  let {
    value,
    onChange,
    onFocus,
    onBlur,
    onSubmit,
    onCancel,
    placeholder = "",
    multiline = true,
    rows = 3,
    flush = false,
    onReady,
  }: Props = $props();

  let editor: HTMLDivElement | undefined = $state();
  let mounted = false;

  // -------------------------------------------------------------------
  // Conversion: template string ⇄ DOM
  // -------------------------------------------------------------------

  function escapeHtml(s: string): string {
    return s
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;")
      .replace(/\n/g, "<br>");
  }
  function escapeAttr(s: string): string {
    return s.replace(/&/g, "&amp;").replace(/"/g, "&quot;");
  }

  /** Build the DOM HTML for a raw template string. */
  function buildHTML(text: string): string {
    const parts = tokenize(text);
    let html = "";
    for (const p of parts) {
      if (p.type === "text") {
        html += escapeHtml(p.text);
      } else {
        const label = humanLabelForPath(p.path);
        html += `<span class="rte-chip" data-token="${escapeAttr(p.raw)}" contenteditable="false">${escapeHtml(label)}</span>`;
      }
    }
    if (html === "") html = "<br>"; // keep the contenteditable line-height
    return html;
  }

  /** Serialize the DOM back to a raw template string. */
  function serialize(): string {
    if (!editor) return value;
    const lines: string[] = [];
    let buf = "";

    const flushLine = () => {
      lines.push(buf);
      buf = "";
    };

    const visit = (node: Node) => {
      if (node.nodeType === Node.TEXT_NODE) {
        buf += node.textContent ?? "";
        return;
      }
      if (!(node instanceof HTMLElement)) return;

      // Variable chip
      if (node.classList.contains("rte-chip")) {
        buf += node.getAttribute("data-token") ?? "";
        return;
      }
      // <br> = newline
      if (node.tagName === "BR") {
        flushLine();
        return;
      }
      // <div>/<p> at top level = paragraph (contenteditable wraps lines on Enter)
      const isLineBlock =
        node !== editor &&
        (node.tagName === "DIV" || node.tagName === "P");

      if (isLineBlock && buf !== "") flushLine();

      for (const c of [...node.childNodes]) visit(c);

      if (isLineBlock) flushLine();
    };
    for (const c of [...editor.childNodes]) visit(c);

    if (buf !== "") flushLine();
    // Trim trailing empty line introduced by terminal <br>.
    while (lines.length && lines[lines.length - 1] === "") lines.pop();
    return lines.join("\n");
  }

  // -------------------------------------------------------------------
  // Mount / external value sync
  // -------------------------------------------------------------------

  onMount(() => {
    if (!editor) return;
    editor.innerHTML = buildHTML(value);
    mounted = true;
    onReady?.({
      insertToken,
      focus: () => editor?.focus(),
      getValue: serialize,
    });
  });

  // Keep the DOM in sync with `value` when the parent passes a new string,
  // but only when the editor is NOT focused — otherwise we'd nuke the
  // user's caret mid-edit.
  $effect(() => {
    value;
    if (!editor || !mounted) return;
    if (document.activeElement === editor) return;
    untrack(() => {
      if (!editor) return;
      const current = serialize();
      if (current !== value) editor.innerHTML = buildHTML(value);
    });
  });

  function notify() {
    onChange?.(serialize());
  }

  // -------------------------------------------------------------------
  // Imperative API
  // -------------------------------------------------------------------

  function insertToken(token: string): void {
    if (!editor) return;
    editor.focus();

    const m = token.match(/^\{\{\s*([^}|]+?)(?:\s*\|\s*[a-z]+)?\s*\}\}$/);
    const path = m ? m[1].trim() : token;
    const label = humanLabelForPath(path);

    const chip = document.createElement("span");
    chip.className = "rte-chip";
    chip.setAttribute("data-token", token);
    chip.setAttribute("contenteditable", "false");
    chip.textContent = label;

    const sel = window.getSelection();
    if (sel && sel.rangeCount > 0 && editor.contains(sel.anchorNode)) {
      const r = sel.getRangeAt(0);
      r.deleteContents();
      r.insertNode(chip);
      // Place caret after the chip.
      const after = document.createRange();
      after.setStartAfter(chip);
      after.collapse(true);
      sel.removeAllRanges();
      sel.addRange(after);
    } else {
      editor.appendChild(chip);
      // Caret at end.
      const r = document.createRange();
      r.selectNodeContents(editor);
      r.collapse(false);
      sel?.removeAllRanges();
      sel?.addRange(r);
    }
    notify();
  }

  // -------------------------------------------------------------------
  // DOM event handlers
  // -------------------------------------------------------------------

  function onInput() {
    notify();
  }

  function onPaste(e: ClipboardEvent) {
    e.preventDefault();
    const text = e.clipboardData?.getData("text/plain") ?? "";
    if (!text) return;
    const sel = window.getSelection();
    if (sel && sel.rangeCount > 0 && editor && editor.contains(sel.anchorNode)) {
      const r = sel.getRangeAt(0);
      r.deleteContents();
      // Plain text only — paste any pre-existing `{{...}}` as chips, otherwise as text.
      const fragment = document.createDocumentFragment();
      const parts = tokenize(text);
      for (const p of parts) {
        if (p.type === "text") {
          const lines = p.text.split("\n");
          for (let i = 0; i < lines.length; i++) {
            if (lines[i]) fragment.appendChild(document.createTextNode(lines[i]));
            if (i < lines.length - 1) fragment.appendChild(document.createElement("br"));
          }
        } else {
          const chip = document.createElement("span");
          chip.className = "rte-chip";
          chip.setAttribute("data-token", p.raw);
          chip.setAttribute("contenteditable", "false");
          chip.textContent = humanLabelForPath(p.path);
          fragment.appendChild(chip);
        }
      }
      r.insertNode(fragment);
      r.collapse(false);
      sel.removeAllRanges();
      sel.addRange(r);
      notify();
    }
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Enter") {
      if (e.ctrlKey || e.metaKey) {
        e.preventDefault();
        onSubmit?.();
        return;
      }
      if (!multiline) {
        e.preventDefault();
        onSubmit?.();
        return;
      }
    }
    if (e.key === "Escape") {
      e.preventDefault();
      onCancel?.();
      return;
    }
    // Tab = insert literal tab is rarely useful in a text element; stop here so
    // focus moves to the next form control instead.
  }
</script>

<div
  class="rte"
  class:rte--flush={flush}
  contenteditable="true"
  role="textbox"
  tabindex="0"
  aria-multiline={multiline}
  data-placeholder={placeholder}
  style:min-height={`${Math.max(1, rows) * 1.4}em`}
  bind:this={editor}
  oninput={onInput}
  onpaste={onPaste}
  onfocus={() => onFocus?.()}
  onblur={() => onBlur?.()}
  onkeydown={onKeydown}
></div>

<style>
  .rte {
    width: 100%;
    padding: var(--space-2);
    border: var(--border-w) solid var(--color-border-input);
    background: var(--color-bg-surface);
    color: var(--color-text);
    font: inherit;
    font-size: var(--fs-md);
    line-height: 1.4;
    outline: none;
    overflow-wrap: anywhere;
    cursor: text;
  }
  .rte:focus {
    border-color: var(--color-border-focus);
    box-shadow: inset 0 -1px 0 0 var(--color-border-focus);
  }
  .rte--flush {
    border-color: var(--color-accent);
    border-width: var(--border-w-strong);
    box-shadow: var(--shadow-float);
  }
  .rte[data-placeholder]:empty::before {
    content: attr(data-placeholder);
    color: var(--color-text-subtle);
    pointer-events: none;
  }
  .rte :global(.rte-chip) {
    display: inline-block;
    background: var(--color-canvas-select);
    border: var(--border-w) solid var(--color-canvas-select-ink);
    color: var(--color-canvas-select-fg);
    padding: 0 6px;
    margin: 0 1px;
    border-radius: 3px;
    font-size: 0.92em;
    line-height: 1.35;
    user-select: none;
    cursor: default;
    white-space: nowrap;
  }
  .rte :global(.rte-chip:hover) {
    background: var(--color-accent-soft);
  }
</style>
