<script lang="ts">
  import { Composer, Heading, Icon, Prose, Spinner, TileButton } from "$lib/components";
  import { SUGGESTIONS } from "$lib/assistant/demo";
  import { assistantStore } from "$lib/assistant/store.svelte";
  import type { ChatMessage } from "$lib/assistant/types";

  interface Props {
    draft: string;
    onSend: (text: string) => void;
    onSuggest: (id: string) => void;
  }

  let { draft = $bindable(), onSend, onSuggest }: Props = $props();

  const conv = $derived(assistantStore.active);
  const empty = $derived(!conv || conv.messages.length === 0);
  const streaming = $derived(assistantStore.streaming);

  let logEl: HTMLDivElement | null = $state(null);

  $effect(() => {
    conv?.messages.length;
    conv?.messages.at(-1)?.content;
    if (!logEl) return;
    logEl.scrollTop = logEl.scrollHeight;
  });

  function who(msg: ChatMessage): string {
    return msg.role === "user" ? "Vous" : "Assistant";
  }
</script>

<section class="chat" class:chat--hero={empty} aria-label="Conversation">
  {#if empty}
    <div class="chat__hero">
      <Heading level={2} size="2xl">Comment puis-je aider ?</Heading>
      <p class="chat__lead">
        {#if assistantStore.configured}
          Posez une question : le modèle répond ici, le canevas à droite.
        {:else}
          Recherche, documents, stock, atelier — l'assistant agira ici. Les suggestions
          ci-dessous montrent le canevas tant qu'aucune clé n'est configurée.
        {/if}
      </p>
      <div class="chat__composer">
        <Composer
          bind:value={draft}
          placeholder="Demander à l'assistant…"
          ariaLabel="Message à l'assistant"
          loading={streaming}
          onSubmit={onSend}
        />
      </div>
      <div class="chat__suggestions">
        {#each SUGGESTIONS as item (item.id)}
          <TileButton
            icon="sparkles"
            label={item.label}
            description={item.description}
            onclick={() => onSuggest(item.id)}
          />
        {/each}
      </div>
    </div>
  {:else}
    <div class="chat__log" bind:this={logEl} role="log" aria-live="polite">
      {#each conv?.messages ?? [] as msg (msg.id)}
        <article class="chat__msg" class:chat__msg--user={msg.role === "user"}>
          <span class="chat__who">
            <Icon name={msg.role === "user" ? "user" : "bot"} size={12} />
            {who(msg)}
          </span>
          {#if msg.role === "assistant"}
            <div class="chat__body chat__body--prose">
              {#if !msg.content && streaming}
                <Spinner size="sm" hideLabel />
              {:else}
                <Prose compact source={msg.content} />
              {/if}
            </div>
          {:else}
            <p class="chat__body">{msg.content}</p>
          {/if}
        </article>
      {/each}
    </div>
    {#if assistantStore.streamError}
      <p class="chat__error" role="alert">{assistantStore.streamError}</p>
    {/if}
    <div class="chat__dock">
      <Composer
        bind:value={draft}
        placeholder="Demander à l'assistant…"
        ariaLabel="Message à l'assistant"
        loading={streaming}
        onSubmit={onSend}
      />
    </div>
  {/if}
</section>

<style>
  .chat {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    background: var(--color-bg-app);
    border-right: var(--border-w) solid var(--color-border);
  }
  .chat--hero {
    justify-content: center;
  }

  .chat__hero {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--space-4);
    padding: var(--space-6);
    max-width: 560px;
    margin: 0 auto;
    width: 100%;
  }
  .chat__lead {
    margin: 0;
    text-align: center;
    color: var(--color-text-muted);
    font-size: var(--fs-sm);
    line-height: var(--lh-relaxed);
  }
  .chat__composer {
    width: 100%;
  }
  .chat__suggestions {
    display: grid;
    grid-template-columns: 1fr 1fr;
    width: 100%;
    background: var(--color-bg-surface);
    border: var(--border-w) solid var(--color-border);
  }

  .chat__log {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    padding: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .chat__msg {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    max-width: 42rem;
  }
  .chat__msg--user {
    align-self: flex-end;
    text-align: right;
  }
  .chat__who {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    font-size: var(--fs-xs);
    font-weight: var(--fw-semibold);
    color: var(--color-text-muted);
    letter-spacing: var(--tracking-caps);
    text-transform: uppercase;
  }
  .chat__msg--user .chat__who {
    flex-direction: row-reverse;
  }
  .chat__body {
    margin: 0;
    padding: var(--space-2) var(--space-3);
    background: var(--color-bg-surface);
    border: var(--border-w) solid var(--color-border-subtle);
    color: var(--color-text);
    font-size: var(--fs-md);
    line-height: var(--lh-relaxed);
    white-space: pre-wrap;
    text-align: left;
  }
  .chat__body--prose {
    white-space: normal;
    user-select: text;
  }
  .chat__msg--user .chat__body {
    background: var(--color-accent-soft);
    border-color: transparent;
  }
  .chat__error {
    margin: 0;
    padding: var(--space-2) var(--space-4);
    color: var(--color-danger);
    font-size: var(--fs-sm);
  }

  .chat__dock {
    flex: 0 0 auto;
    padding: var(--space-3) var(--space-4);
    border-top: var(--border-w) solid var(--color-border);
    background: var(--color-bg-surface);
  }
</style>
