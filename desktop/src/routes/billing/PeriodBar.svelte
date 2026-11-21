<script lang="ts">
  /**
   * Period selector shared by the accounting reports. Presets cover the
   * questions actually asked (this month, last month, the quarter, the year);
   * the two date fields handle everything else.
   */
  import { Button, Icon, Input } from "$lib/components";
  import { isoDay } from "$lib/format";

  interface Props {
    from: string;
    to: string;
    busy?: boolean;
    onApply: (from: string, to: string) => void;
  }

  let { from = $bindable(), to = $bindable(), busy = false, onApply }: Props = $props();

  const iso = isoDay;

  function setRange(start: Date, end: Date): void {
    from = iso(start);
    to = iso(end);
    onApply(from, to);
  }

  function thisMonth(): void {
    const now = new Date();
    setRange(new Date(now.getFullYear(), now.getMonth(), 1), new Date(now.getFullYear(), now.getMonth() + 1, 0));
  }

  function lastMonth(): void {
    const now = new Date();
    setRange(new Date(now.getFullYear(), now.getMonth() - 1, 1), new Date(now.getFullYear(), now.getMonth(), 0));
  }

  function thisQuarter(): void {
    const now = new Date();
    const first = Math.floor(now.getMonth() / 3) * 3;
    setRange(new Date(now.getFullYear(), first, 1), new Date(now.getFullYear(), first + 3, 0));
  }

  function thisYear(): void {
    const year = new Date().getFullYear();
    setRange(new Date(year, 0, 1), new Date(year, 11, 31));
  }
</script>

<div class="pbar">
  <div class="pbar__presets">
    <Button variant="ghost" size="sm" onclick={thisMonth}>Ce mois</Button>
    <Button variant="ghost" size="sm" onclick={lastMonth}>Mois dernier</Button>
    <Button variant="ghost" size="sm" onclick={thisQuarter}>Ce trimestre</Button>
    <Button variant="ghost" size="sm" onclick={thisYear}>Cette année</Button>
  </div>

  <div class="pbar__range">
    <span class="pbar__label">Du</span>
    <Input bind:value={from} placeholder="AAAA-MM-JJ" />
    <span class="pbar__label">au</span>
    <Input bind:value={to} placeholder="AAAA-MM-JJ" />
    <Button variant="secondary" loading={busy} onclick={() => onApply(from, to)}>
      <Icon name="refresh-cw" size={13} /> Actualiser
    </Button>
  </div>
</div>

<style>
  .pbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: var(--space-3);
  }

  .pbar__presets,
  .pbar__range {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .pbar__range :global(.input) { width: 140px; }

  .pbar__label {
    font-size: var(--fs-sm);
    color: var(--color-text-muted);
  }
</style>
