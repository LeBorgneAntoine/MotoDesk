<script lang="ts">
  import { Alert, Button, FormField, Heading, Input } from "$lib/components";
  import { authStore } from "$lib/stores/auth.svelte";
  import { router } from "$lib/router/router.svelte";
  import { ApiError } from "$lib/api/types";
  import AuthTabs from "./AuthTabs.svelte";

  let companyName = $state("");
  let fullName = $state("");
  let email = $state("");
  let password = $state("");
  let confirm = $state("");
  let inviteCode = $state("");
  let submitting = $state(false);
  let formError = $state<string | null>(null);

  async function onSubmit(e: SubmitEvent) {
    e.preventDefault();
    if (submitting) return;
    if (password !== confirm) {
      formError = "Les mots de passe ne correspondent pas.";
      return;
    }
    if (password.length < 8) {
      formError = "Le mot de passe doit comporter au moins 8 caractères.";
      return;
    }

    submitting = true;
    formError = null;
    try {
      await authStore.register(
        email,
        password,
        fullName,
        companyName.trim() || undefined,
        inviteCode.trim() || undefined,
      );
      router.replace("dashboard");
    } catch (err) {
      if (err instanceof ApiError) {
        const detail = err.details && err.details.length ? ` — ${err.details.join("; ")}` : "";
        formError = `${err.message}${detail}`;
      } else {
        formError = "Erreur réseau, veuillez réessayer.";
      }
    } finally {
      submitting = false;
    }
  }
</script>

<!--
  Renders only the form contents. See Login.svelte for the rationale.
-->
<div class="af">
  <AuthTabs active="register" />

  <header class="af__head">
    <Heading level={1} size="2xl">Créer un compte</Heading>
    <p class="af__subtitle">Configurez votre espace Motodesk.</p>
  </header>

  <form class="af__form" onsubmit={onSubmit}>
    {#if formError}
      <Alert variant="danger">{formError}</Alert>
    {/if}

    <FormField
      label="Nom de l'entreprise"
      htmlFor="reg-company"
      required={!inviteCode.trim()}
      hint={inviteCode.trim()
        ? "Inutile si vous rejoignez une entreprise avec une invitation."
        : "Vous serez l'administrateur de cette entreprise"}
    >
      <Input id="reg-company" name="company_name" bind:value={companyName} />
    </FormField>

    <FormField
      label="Code d'invitation"
      htmlFor="reg-invite"
      hint="Pour rejoindre une entreprise existante. Laissez vide pour créer la vôtre (premier compte seulement)."
    >
      <Input id="reg-invite" name="invite_code" bind:value={inviteCode} />
    </FormField>

    <FormField label="Nom complet" htmlFor="reg-name" required>
      <Input id="reg-name" name="full_name" required bind:value={fullName} />
    </FormField>

    <FormField label="Email" htmlFor="reg-email" required>
      <Input
        id="reg-email"
        type="email"
        name="email"
        autocomplete="username"
        required
        bind:value={email}
      />
    </FormField>

    <FormField label="Mot de passe" htmlFor="reg-password" required hint="Minimum 8 caractères">
      <Input
        id="reg-password"
        type="password"
        name="password"
        autocomplete="new-password"
        required
        bind:value={password}
      />
    </FormField>

    <FormField label="Confirmer" htmlFor="reg-confirm" required>
      <Input
        id="reg-confirm"
        type="password"
        name="confirm_password"
        autocomplete="new-password"
        required
        bind:value={confirm}
      />
    </FormField>

    <Button type="submit" variant="primary" size="lg" fullWidth loading={submitting}>
      Créer mon compte
    </Button>
  </form>
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
