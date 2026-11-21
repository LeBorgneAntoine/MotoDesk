<script lang="ts">
  /**
   * First run without a cloud: create the shop's local network by creating its first
   * administrator, from a username and a password. See docs/standalone.md.
   */
  import { Alert, Button, Checkbox, FormField, Heading, Input } from "$lib/components";
  import { authStore } from "$lib/stores/auth.svelte";
  import { accessStore } from "$lib/stores/access.svelte";
  import { router } from "$lib/router/router.svelte";
  import { ApiError } from "$lib/api/types";
  import { validateNewLocalAccount } from "$lib/api/localAccount";
  import AuthTabs from "./AuthTabs.svelte";

  let username = $state("");
  let password = $state("");
  let confirm = $state("");
  let submitting = $state(false);
  let formError = $state<string | null>(null);
  // Another network of the shop is visible on the LAN: the user must say they mean a new one.
  let networkSeen = $state(false);
  let confirmNew = $state(false);

  async function onSubmit(e: SubmitEvent) {
    e.preventDefault();
    if (submitting) return;
    formError = validateNewLocalAccount(username, password, confirm);
    if (formError) return;
    submitting = true;
    try {
      await authStore.setupLocal(username.trim(), password, confirm, confirmNew);
      router.replace("dashboard");
    } catch (err) {
      networkSeen = err instanceof ApiError && err.code === "network_exists";
      formError = err instanceof ApiError ? err.message : "Erreur inattendue, veuillez réessayer.";
    } finally {
      submitting = false;
    }
  }
</script>

<!-- Renders only the form contents; see Register.svelte. -->
<div class="af">
  <AuthTabs active="register" />

  <header class="af__head">
    <Heading level={1} size="2xl">Créer le réseau local</Heading>
    <p class="af__subtitle">
      Créez l'administrateur du réseau de l'atelier. Aucune connexion internet n'est nécessaire ;
      vous pourrez passer en ligne plus tard depuis les réglages. Ce poste restera actif en
      arrière-plan et démarrera avec Windows pour que les autres puissent s'y connecter
      (réglable dans Réglages › Synchronisation).
    </p>
  </header>

  {#if accessStore.localState === "signin"}
    <Alert variant="info">
      Ce poste fait déjà partie d'un réseau local.
      <Button variant="link" size="sm" onclick={() => router.replace("login")}>Se connecter</Button>
    </Alert>
  {:else if accessStore.localState === "online"}
    <Alert variant="info">
      Cette installation est maintenant en ligne : repassez sur « En ligne ».
    </Alert>
  {:else if accessStore.localState === "tied"}
    <Alert variant="info">
      Ce poste est déjà lié à une entreprise en ligne : repassez sur « En ligne ».
    </Alert>
  {:else}
    <form class="af__form" onsubmit={onSubmit}>
      {#if formError}
        <Alert variant="danger">{formError}</Alert>
      {/if}

      <FormField
        label="Identifiant"
        htmlFor="localreg-username"
        required
        hint="3 à 32 caractères : lettres, chiffres, « . », « _ », « - »"
      >
        <Input
          id="localreg-username"
          name="username"
          autocomplete="username"
          required
          bind:value={username}
        />
      </FormField>

      <FormField
        label="Mot de passe"
        htmlFor="localreg-password"
        required
        hint="Minimum 8 caractères. Il n'y a pas de récupération : notez-le."
      >
        <Input
          id="localreg-password"
          type="password"
          name="password"
          autocomplete="new-password"
          required
          bind:value={password}
        />
      </FormField>

      <FormField label="Confirmer le mot de passe" htmlFor="localreg-confirm" required>
        <Input
          id="localreg-confirm"
          type="password"
          name="confirm_password"
          autocomplete="new-password"
          required
          bind:value={confirm}
        />
      </FormField>

      {#if networkSeen}
        <Checkbox
          bind:checked={confirmNew}
          label="Créer un nouveau réseau quand même"
          hint="À ne cocher que si l'atelier n'a pas déjà un réseau : sinon, connectez-vous pour le rejoindre."
        />
      {/if}

      <Button
        type="submit"
        variant="primary"
        size="lg"
        fullWidth
        loading={submitting}
        disabled={networkSeen && !confirmNew}
      >
        Créer le réseau local
      </Button>
    </form>
  {/if}
</div>

<style>
  .af {
    width: 360px;
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  .af__head {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .af__subtitle {
    font-size: var(--fs-sm);
    color: var(--color-text-muted);
  }
  .af__form {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
</style>
