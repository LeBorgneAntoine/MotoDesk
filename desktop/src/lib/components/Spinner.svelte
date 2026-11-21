<script lang="ts">
  /**
   * Spinner — the single loading indicator for the whole app.
   * Plays the shared Lottie loader. Never re-declare a spin keyframe in a screen.
   */
  import { onDestroy } from "svelte";
  import type { AnimationItem } from "lottie-web";
  import { playLoadingAnimation } from "$lib/lottie/loading";

  interface Props {
    /** Accessible label, also shown next to the mark unless `hideLabel`. */
    label?: string;
    size?: "sm" | "md" | "lg";
    /** Render the mark on its own (label still announced to screen readers). */
    hideLabel?: boolean;
    /** `current` inherits the parent's colour — for spinners inside buttons. */
    tone?: "default" | "current";
  }

  let {
    label = "Chargement…",
    size = "md",
    hideLabel = false,
    tone = "default",
  }: Props = $props();

  let host: HTMLDivElement | null = $state(null);
  let anim: AnimationItem | null = null;

  function start(): void {
    if (!host) return;
    anim?.destroy();
    host.replaceChildren();
    const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    anim = playLoadingAnimation({ host, tone, reducedMotion: reduced });
  }

  $effect(() => {
    if (!host) return;
    void tone;
    start();
    const obs = new MutationObserver(start);
    obs.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["data-theme", "data-accent"],
    });
    return () => {
      obs.disconnect();
      anim?.destroy();
      anim = null;
    };
  });

  onDestroy(() => {
    anim?.destroy();
    anim = null;
  });
</script>

<div class="spin spin--{size} spin--{tone}" role="status" aria-label={label}>
  <div class="spin__anim" bind:this={host} aria-hidden="true"></div>
  {#if !hideLabel}<span class="spin__label">{label}</span>{/if}
</div>

<style>
  .spin {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    color: var(--color-text-muted);
    font-size: var(--fs-sm);
  }

  .spin__anim {
    flex: 0 0 auto;
    display: block;
    line-height: 0;
  }
  .spin__anim :global(svg) {
    display: block;
    width: 100%;
    height: 100%;
  }

  .spin--sm .spin__anim { width: 16px; height: 16px; }
  .spin--md .spin__anim { width: 28px; height: 28px; }
  .spin--lg .spin__anim { width: 32px; height: 32px; }

  .spin--current { color: inherit; }
</style>
