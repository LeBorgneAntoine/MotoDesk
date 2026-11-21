<script lang="ts">
  /**
   * PosPayBar — the three primary tenders at the till.
   * Labels come from the screen (kit stays language-agnostic).
   */
  import PosKey from "./PosKey.svelte";

  interface Props {
    cashLabel: string;
    cardLabel: string;
    chequeLabel: string;
    disabled?: boolean;
    onCash?: () => void;
    onCard?: () => void;
    onCheque?: () => void;
  }

  let { cashLabel, cardLabel, chequeLabel, disabled = false, onCash, onCard, onCheque }: Props = $props();
</script>

<div class="bar">
  <PosKey primary disabled={disabled} onclick={onCash} label={cashLabel} />
  <PosKey disabled={disabled} onclick={onCard} label={cardLabel} />
  <PosKey muted disabled={disabled} onclick={onCheque} label={chequeLabel} />
</div>

<style>
  .bar {
    display: grid;
    grid-template-columns: 1fr 1fr 1fr;
    gap: var(--space-2);
  }
</style>
