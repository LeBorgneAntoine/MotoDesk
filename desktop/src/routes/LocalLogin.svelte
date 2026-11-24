<script lang="ts">
  /**
   * Sign in to the shop's local network (no cloud) with an account its administrator created.
   * A PC that belongs to the network checks the account it holds; a PC that has never joined
   * looks for a PC of the network and joins it with the same credentials, which is why it
   * needs one to be on. See docs/standalone.md.
   */
  import { Alert, Button, FormField, Heading, Input } from "$lib/components";
  import { authStore } from "$lib/stores/auth.svelte";
  import { accessStore } from "$lib/stores/access.svelte";
  import { router } from "$lib/router/router.svelte";
  import { ApiError } from "$lib/api/types";
  import AuthTabs from "./AuthTabs.svelte";
  import RestoreFirstRun from "./RestoreFirstRun.svelte";
  import RecoverAccess from "./RecoverAccess.svelte";

  let username = $state("");
  let password = $state("");
  let submitting = $state(false);
  let formError = $state<string | null>(null);
  let restoring = $state(false);
  let recovering = $state(false);

  // A PC with nothing set up joins the network with these credentials instead of checking them.
  const joining = $derived(accessStore.localState === "setup");

  async function onSubmit(e: SubmitEvent) {
    e.preventDefault();
    if (submitting) return;
    submitting = true;
    formError = null;
    try {
      await authStore.loginLocal(username, password);
      router.replace("dashboard");
    } catch (err) {
      formError = err instanceof ApiError ? err.message : "Erreur inattendue, veuillez réessayer.";
    } finally {
      submitting = false;
    }
  }
</script>

<!-- Renders only the form contents; see Login.svelte. -->
<div class="af">
  <AuthTabs active="login" />

  <header class="af__head">
    <Heading level={1} size="2xl">Réseau local</Heading>
    <p class="af__subtitle">Connectez-vous avec un compte du réseau de l'atelier.</p>
  </header>

  {#if accessStore.localState === "online"}
    <Alert variant="info">
      Cette installation est maintenant en ligne : repassez sur « En ligne » et connectez-vous avec
      l'adresse e-mail de mise en ligne.
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

      <FormField label="Identifiant" htmlFor="locallogin-username" required>
        <Input
          id="locallogin-username"
          name="username"
          autocomplete="username"
          required
          bind:value={username}
        />
      </FormField>

      <FormField label="Mot de passe" htmlFor="locallogin-password" required>
        <Input
          id="locallogin-password"
          type="password"
          name="password"
          autocomplete="current-password"
          required
          bind:value={password}
        />
      </FormField>

      {#if joining}
        <p class="af__subtitle">
          Ce poste n'a pas encore rejoint le réseau : il va chercher un poste allumé de l'atelier et
          s'y connecter avec ces identifiants. Un poste du réseau doit donc être allumé.
        </p>
      {/if}

      <Button type="submit" variant="primary" size="lg" fullWidth loading={submitting}>
        {joining ? "Rejoindre le réseau" : "Se connecter"}
      </Button>

      {#if !joining}
        <div class="af__alt">
          <Button variant="link" size="sm" type="button" onclick={() => (recovering = true)}>
            Mot de passe perdu ?
          </Button>
        </div>
      {/if}

      {#if joining}
        <div class="af__alt">
          Pas encore de réseau dans cet atelier ?
          <Button variant="link" size="sm" onclick={() => router.replace("register")}>
            Créer le réseau local
          </Button>
        </div>
        <div class="af__alt">
          <Button variant="link" size="sm" type="button" onclick={() => (restoring = true)}>
            Restaurer une sauvegarde
          </Button>
        </div>
      {/if}
    </form>
  {/if}
</div>

<RecoverAccess open={recovering} onClose={() => (recovering = false)} />

<RestoreFirstRun open={restoring} onClose={() => (restoring = false)} />

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
  .af__alt {
    font-size: var(--fs-sm);
    color: var(--color-text-muted);
    text-align: center;
  }
</style>
