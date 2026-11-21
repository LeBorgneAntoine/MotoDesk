<script lang="ts">
  import { Alert, Button, FormField, Heading, Input } from "$lib/components";
  import { authStore } from "$lib/stores/auth.svelte";
  import { router } from "$lib/router/router.svelte";
  import { ApiError } from "$lib/api/types";
  import { toast } from "$lib/stores/toast.svelte";
  import { cloudGet } from "$lib/api/cloudClient";
  import { onMount } from "svelte";
  import AuthTabs from "./AuthTabs.svelte";

  let identifier = $state("");
  let password = $state("");
  let submitting = $state(false);
  let formError = $state<string | null>(null);
  let canRegister = $state(true);

  onMount(() => {
    void cloudGet<{ allow_public: boolean; first_run: boolean }>("/api/auth/registration-options")
      .then((o) => {
        canRegister = o.allow_public || o.first_run;
      })
      .catch(() => {
        canRegister = true;
      });
  });

  async function onSubmit(e: SubmitEvent) {
    e.preventDefault();
    if (submitting) return;
    submitting = true;
    formError = null;
    try {
      await authStore.login(identifier, password);
      const msg = authStore.bootstrapMessage();
      if (msg) toast.success(msg);
      router.replace("dashboard");
    } catch (err) {
      if (err instanceof ApiError) {
        formError =
          err.code === "cloud_required"
            ? "Le serveur cloud n'est pas accessible. Vérifiez votre connexion internet et l'adresse du serveur (Réglages › Synchronisation)."
            : err.code === "network_error"
              ? err.message
              : err.message;
      } else {
        formError = "Erreur réseau, veuillez réessayer.";
      }
    } finally {
      submitting = false;
    }
  }
</script>

<!--
  Renders only the form contents. The outer card / brand panel are owned
  by App.svelte so the card can morph between login and register without
  this component participating in layout.
-->
<div class="af">
  <AuthTabs active="login" />

  <header class="af__head">
    <Heading level={1} size="2xl">Connexion</Heading>
    <p class="af__subtitle">Accédez à votre espace de travail.</p>
  </header>

  <form class="af__form" onsubmit={onSubmit}>
    {#if formError}
      <Alert variant="danger">{formError}</Alert>
    {/if}

    <FormField label="Email ou identifiant" htmlFor="login-identifier" required>
      <Input
        id="login-identifier"
        type="text"
        name="identifier"
        autocomplete="username"
        required
        bind:value={identifier}
      />
    </FormField>

    <FormField label="Mot de passe" htmlFor="login-password" required>
      <Input
        id="login-password"
        type="password"
        name="password"
        autocomplete="current-password"
        required
        bind:value={password}
      />
    </FormField>

    <Button type="submit" variant="primary" size="lg" fullWidth loading={submitting}>
      Se connecter
    </Button>

    {#if !canRegister}
      <p class="af__subtitle">
        Les inscriptions sont fermées. Demandez une invitation à un administrateur.
      </p>
    {/if}
  </form>
</div>

<style>
  .af {
    width: 320px;
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
