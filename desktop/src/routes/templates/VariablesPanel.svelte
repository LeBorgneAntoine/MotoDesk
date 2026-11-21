<script lang="ts">
  import { Heading, ListRow, PanelSection } from "$lib/components";
  import {
    groupVariables,
    listsForKind,
    scalarsForKind,
    type VariableDef,
  } from "$lib/templates/variables";
  import type { TemplateKind } from "$lib/api/templates";

  interface Props {
    kind: TemplateKind;
    /** True when a TextEl is currently selected — variable buttons become
     *  "insert into selected element" instead of "copy to clipboard". */
    canInsert: boolean;
    onInsert: (token: string) => void;
  }

  let { kind, canInsert, onInsert }: Props = $props();

  const groups = $derived(groupVariables(scalarsForKind(kind)));
  const lists = $derived(listsForKind(kind));

  // Everything but Document / Client starts folded — the list is long.
  let collapsed = $state<Record<string, boolean>>({});

  function isCollapsed(group: string): boolean {
    return collapsed[group] ?? !(group === "Document" || group === "Client");
  }

  function handleClick(v: VariableDef) {
    if (canInsert) {
      onInsert(v.token);
    } else {
      try {
        void navigator.clipboard?.writeText(v.token);
      } catch {
        /* clipboard not available */
      }
    }
  }
</script>

<div class="vars">
  <Heading level={4} size="xs" caps>Variables</Heading>
  <p class="vars__hint">
    {#if canInsert}
      Cliquez pour insérer dans le texte sélectionné.
    {:else}
      Sélectionnez un texte pour insérer, ou cliquez pour copier.
    {/if}
  </p>

  <div class="vars__groups">
    {#each groups as g (g.group)}
      <PanelSection
        title={g.group}
        subtitle={String(g.items.length)}
        collapsible
        flush
        collapsed={isCollapsed(g.group)}
        onToggle={(next) => (collapsed[g.group] = next)}
      >
        <ul class="vgroup__list">
          {#each g.items as v (v.path)}
            <li>
              <ListRow
                dense
                icon="braces"
                title={v.label}
                ariaLabel={v.token}
                onclick={() => handleClick(v)}
              />
            </li>
          {/each}
        </ul>
      </PanelSection>
    {/each}

    {#if lists.length > 0}
      <PanelSection title="Listes" icon="table" flush>
        <ul class="vgroup__list">
          {#each lists as l (l.path)}
            <li class="vlist">
              <div class="vlist__name">{l.label}</div>
              <div class="vlist__path">
                Liaison: <code>{l.path}</code>
              </div>
              <div class="vlist__fields">
                {#each l.fields as f (f.binding)}
                  <span class="vlist__field" title={`${l.path}[].${f.binding}`}>{f.label}</span>
                {/each}
              </div>
              <p class="vlist__hint">
                Glissez l'outil « Tableau » sur la page, puis liez-le à « {l.path} » dans le panneau de droite.
              </p>
            </li>
          {/each}
        </ul>
      </PanelSection>
    {/if}
  </div>
</div>

<style>
  .vars {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding: var(--space-3);
    border-top: var(--border-w) solid var(--color-border-subtle);
    flex: 1 1 auto;
    min-height: 0;
  }
  .vars__hint {
    font-size: var(--fs-xs);
    color: var(--color-text-muted);
    line-height: 1.4;
    margin: 0;
  }

  .vars__groups {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    overflow-y: auto;
    flex: 1 1 auto;
    min-height: 0;
  }

  .vgroup__list {
    list-style: none;
    margin: 0;
    padding: var(--space-1);
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .vgroup__list :global(.lrow__icon) { color: var(--color-accent); }

  .vlist {
    padding: var(--space-2);
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .vlist__name {
    font-size: var(--fs-sm);
    font-weight: var(--fw-medium);
    color: var(--color-text-strong);
  }
  .vlist__path {
    font-size: var(--fs-xs);
    color: var(--color-text-muted);
  }
  .vlist__path code {
    background: var(--color-bg-app);
    padding: 1px 4px;
    font-family: var(--font-mono);
    font-size: var(--fs-micro);
  }
  .vlist__fields {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .vlist__field {
    background: var(--color-bg-app);
    color: var(--color-text-muted);
    padding: 2px 6px;
    font-size: var(--fs-xs);
    line-height: 1;
  }
  .vlist__hint {
    font-size: var(--fs-xs);
    color: var(--color-text-muted);
    margin: var(--space-1) 0 0;
    line-height: 1.4;
  }
</style>
