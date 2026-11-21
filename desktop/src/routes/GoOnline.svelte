<script lang="ts">
  /**
   * Put a standalone installation online. The cloud creates this company under the ids it
   * already has, with the administrator's e-mail and a password for the cloud account, and
   * the normal sync then uploads everything entered so far. See docs/standalone.md.
   */
  import { Alert, Button, Checkbox, FormField, Input, SectionCard } from "$lib/components";
  import { authStore } from "$lib/stores/auth.svelte";
  import { ApiError } from "$lib/api/types";
  import { toast } from "$lib/stores/toast.svelte";
  import { validateNewLocalAccount } from "$lib/api/localAccount";

  interface Props {
    /** The cloud address this build points at, shown so there is no surprise. */
    cloudUrl?: string;
    onDone?: () => void;
  }

  let { cloudUrl = "", onDone }: Props = $props();

  let email = $state("");
  let password = $state("");
  let confirm = $state("");
  let companyName = $state("");
  let understood = $state(false);
  let submitting = $state(false);
  let error = $state<string | null>(null);

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    if (submitting) return;
    error = null;
    if (!email.includes("@")) {
      error = "Saisissez une adresse e-mail valide.";
      return;
    }
    // The password rules are the cloud's: the same 8 to 128 characters as a local one.
    const problem = validateNewLocalAccount("x".repeat(3), password, confirm);
    if (problem) {
      error = problem;
      return;
    }
    submitting = true;
    try {
      await authStore.goOnline({
        email: email.trim(),
        password,
        confirm,
        company_name: companyName.trim() || undefined,
      });
      toast.success("Installation en ligne : vos données sont envoyées au cloud.");
      onDone?.();
    } catch (err) {
      error = err instanceof ApiError ? err.message : "Erreur inattendue, veuillez réessayer.";
    } finally {
      submitting = false;
    }
  }
</script>

<SectionCard
  title="Passer en ligne"
  subtitle="Le réseau local de l'atelier fonctionne sans cloud."
  icon="upload"
>
  <form class="go" onsubmit={submit}>
    <p class="go__text">
      L'entreprise est créée sur le cloud avec ses données actuelles (clients, stock, ventes, journal
      fiscal), puis envoyée par la synchronisation. Vous vous connecterez ensuite avec l'adresse
      e-mail et le mot de passe ci-dessous, depuis cet ordinateur ou un autre.
    </p>

    {#if cloudUrl}
      <p class="go__text is-muted">Serveur cloud : <code>{cloudUrl}</code></p>
    {/if}

    {#if error}
      <Alert variant="danger">{error}</Alert>
    {/if}

    <FormField label="Adresse e-mail du compte en ligne" htmlFor="go-email" required>
      <Input id="go-email" type="email" autocomplete="email" required bind:value={email} />
    </FormField>
    <FormField
      label="Mot de passe du compte en ligne"
      htmlFor="go-password"
      required
      hint="Minimum 8 caractères"
    >
      <Input
        id="go-password"
        type="password"
        autocomplete="new-password"
        required
        bind:value={password}
      />
    </FormField>
    <FormField label="Confirmer le mot de passe" htmlFor="go-confirm" required>
      <Input
        id="go-confirm"
        type="password"
        autocomplete="new-password"
        required
        bind:value={confirm}
      />
    </FormField>
    <FormField
      label="Nom de l'entreprise sur le cloud"
      htmlFor="go-company"
      hint="Laissez vide pour reprendre le nom saisi dans Réglages › Entreprise."
    >
      <Input id="go-company" bind:value={companyName} />
    </FormField>

    <Checkbox bind:checked={understood} label="Je comprends que cette étape est définitive : l'installation ne revient pas en mode local." />

    <div>
      <Button type="submit" variant="primary" loading={submitting} disabled={!understood}>
        Passer en ligne
      </Button>
    </div>
  </form>
</SectionCard>

<style>
  .go {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    max-width: 480px;
  }
  .go__text {
    font-size: var(--fs-sm);
    line-height: 1.5;
  }
  .is-muted {
    color: var(--color-text-muted);
  }
</style>
