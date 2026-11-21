<script lang="ts">
  /**
   * Prose — rendered markdown for document-style surfaces (assistant canvas,
   * later: notes). Headings, tables, links and code are HTML inside this
   * component so routes never emit raw <h1>/<table>.
   */
  import { renderMarkdown } from "$lib/markdown/render";

  interface Props {
    source: string;
    /** Tighter type for chat bubbles. */
    compact?: boolean;
  }

  let { source, compact = false }: Props = $props();

  const html = $derived(renderMarkdown(source));
</script>

<div class="prose" class:prose--compact={compact}>
  {@html html}
</div>

<style>
  .prose {
    max-width: 46rem;
    margin: 0 auto;
    color: var(--color-text);
    font-size: var(--fs-md);
    line-height: var(--lh-relaxed);
    user-select: text;
    cursor: text;
  }

  .prose :global(> :first-child) {
    margin-top: 0;
  }
  .prose :global(> :last-child) {
    margin-bottom: 0;
  }

  .prose :global(h1),
  .prose :global(h2),
  .prose :global(h3),
  .prose :global(h4) {
    color: var(--color-text-strong);
    font-weight: var(--fw-semibold);
    letter-spacing: var(--tracking-tight);
    line-height: var(--lh-tight);
  }
  .prose :global(h1) {
    font-size: var(--fs-3xl);
    margin: 0 0 var(--space-5);
  }
  .prose :global(h2) {
    font-size: var(--fs-xl);
    margin: var(--space-6) 0 var(--space-3);
  }
  .prose :global(h3) {
    font-size: var(--fs-lg);
    margin: var(--space-5) 0 var(--space-2);
  }
  .prose :global(h4) {
    font-size: var(--fs-md);
    margin: var(--space-4) 0 var(--space-2);
  }

  .prose :global(p) {
    margin: 0 0 var(--space-4);
  }
  .prose :global(strong) {
    font-weight: var(--fw-semibold);
    color: var(--color-text-strong);
  }
  .prose :global(em) {
    font-style: italic;
  }

  .prose :global(ul),
  .prose :global(ol) {
    margin: 0 0 var(--space-4);
    padding-left: var(--space-5);
  }
  .prose :global(li) {
    margin: 0 0 var(--space-1);
  }
  .prose :global(li) :global(> p) {
    margin: 0;
  }

  .prose :global(blockquote) {
    margin: 0 0 var(--space-4);
    padding: var(--space-2) var(--space-4);
    border-left: 3px solid var(--color-border);
    color: var(--color-text-muted);
    background: var(--color-bg-app);
  }

  .prose :global(hr) {
    margin: var(--space-6) 0;
    border: 0;
    border-top: var(--border-w) solid var(--color-border);
  }

  .prose :global(a) {
    color: var(--color-accent);
    text-decoration: none;
  }
  .prose :global(a:hover) {
    color: var(--color-accent-hover);
    text-decoration: underline;
  }

  .prose :global(code) {
    font-family: var(--font-mono);
    font-size: var(--fs-sm);
    padding: 1px var(--space-1);
    background: var(--color-bg-app);
    border: var(--border-w) solid var(--color-border-subtle);
    border-radius: var(--radius-control);
  }
  .prose :global(pre) {
    margin: 0 0 var(--space-4);
    padding: var(--space-3) var(--space-4);
    background: var(--color-bg-app);
    border: var(--border-w) solid var(--color-border);
    overflow-x: auto;
    border-radius: var(--radius-control);
  }
  .prose :global(pre code) {
    padding: 0;
    background: transparent;
    border: 0;
  }

  .prose :global(table) {
    width: 100%;
    border-collapse: collapse;
    margin: 0 0 var(--space-5);
    font-size: var(--fs-sm);
  }
  .prose :global(th),
  .prose :global(td) {
    text-align: left;
    padding: var(--space-2) var(--space-3);
    border-bottom: var(--border-w) solid var(--color-border);
    vertical-align: top;
  }
  .prose :global(th) {
    font-weight: var(--fw-semibold);
    color: var(--color-text-strong);
    background: var(--color-bg-app);
    border-bottom-color: var(--color-border);
  }
  .prose :global(tr:last-child td) {
    border-bottom: 0;
  }

  .prose--compact {
    max-width: none;
    margin: 0;
    font-size: var(--fs-md);
  }
  .prose--compact :global(h1) {
    font-size: var(--fs-lg);
    margin: 0 0 var(--space-2);
  }
  .prose--compact :global(h2),
  .prose--compact :global(h3),
  .prose--compact :global(h4) {
    font-size: var(--fs-md);
    margin: var(--space-3) 0 var(--space-1);
  }
  .prose--compact :global(p),
  .prose--compact :global(ul),
  .prose--compact :global(ol),
  .prose--compact :global(blockquote),
  .prose--compact :global(pre),
  .prose--compact :global(table) {
    margin-bottom: var(--space-2);
  }
</style>
