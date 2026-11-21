<script lang="ts">
  /**
   * Employee form body. The host panel owns the action bar and calls `submit()`.
   */
  import { untrack } from "svelte";
  import { Alert, Checkbox, FormField, Input, Select, type SelectOption } from "$lib/components";
  import type { Employee } from "$lib/api/types";

  export interface EmployeeFormData {
    email: string;
    username: string;
    password: string;
    full_name: string;
    role: "admin" | "employee";
    is_active: boolean;
  }

  interface Props {
    initial: Employee | null;
    submitting?: boolean;
    errorMessage?: string | null;
    onSubmit: (data: EmployeeFormData) => void;
    onDirtyChange?: (dirty: boolean) => void;
  }

  let {
    initial,
    submitting = false,
    errorMessage = null,
    onSubmit,
    onDirtyChange,
  }: Props = $props();

  const isEdit = untrack(() => initial !== null);

  const start = untrack(() => ({
    fullName: initial?.full_name ?? "",
    email: initial?.email ?? "",
    username: initial?.username ?? "",
    role: (initial?.role === "admin" ? "admin" : "employee") as "admin" | "employee",
    isActive: initial?.is_active ?? true,
  }));

  let fullName = $state(start.fullName);
  let email = $state(start.email);
  let username = $state(start.username);
  let password = $state("");
  let role = $state<"admin" | "employee">(start.role);
  let isActive = $state(start.isActive);

  let touched = $state(false);

  const roleOptions: SelectOption<"admin" | "employee">[] = [
    { value: "employee", label: "Employé" },
    { value: "admin", label: "Administrateur" },
  ];

  const emailInvalid = $derived(touched && !/^[^@\s]+@[^@\s]+\.[^@\s]+$/.test(email.trim()));
  const nameInvalid = $derived(touched && fullName.trim().length === 0);
  const passwordInvalid = $derived(
    touched && (!isEdit || password.length > 0) && password.length < 8,
  );

  const dirty = $derived(
    fullName !== start.fullName ||
      email !== start.email ||
      username !== start.username ||
      password.length > 0 ||
      role !== start.role ||
      isActive !== start.isActive,
  );

  $effect(() => {
    onDirtyChange?.(dirty);
  });

  export function submit(): void {
    if (submitting) return;
    touched = true;
    if (nameInvalid || emailInvalid || passwordInvalid) return;

    onSubmit({
      email: email.trim(),
      username: username.trim(),
      password,
      full_name: fullName.trim(),
      role,
      is_active: isActive,
    });
  }

  function onFormSubmit(e: SubmitEvent): void {
    e.preventDefault();
    submit();
  }
</script>

<form class="ef" onsubmit={onFormSubmit} novalidate>
  {#if errorMessage}
    <Alert variant="danger">{errorMessage}</Alert>
  {/if}

  <FormField label="Nom complet" required error={nameInvalid ? "Le nom est obligatoire." : null}>
    <Input bind:value={fullName} placeholder="Jean Dupont" invalid={nameInvalid} />
  </FormField>

  <div class="ef__grid">
    <FormField
      label="Email"
      required
      error={emailInvalid ? "Email invalide." : null}
      hint={isEdit ? "Sert à la connexion." : undefined}
    >
      <Input
        bind:value={email}
        type="email"
        placeholder="jean@atelier.fr"
        autocomplete="off"
        invalid={emailInvalid}
      />
    </FormField>
    <FormField label="Identifiant" hint="Optionnel — sert aussi à la connexion.">
      <Input bind:value={username} placeholder="jdupont" autocomplete="off" />
    </FormField>
  </div>

  <div class="ef__grid">
    <FormField
      label={isEdit ? "Nouveau mot de passe" : "Mot de passe"}
      required={!isEdit}
      error={passwordInvalid ? "Au moins 8 caractères." : null}
      hint={isEdit ? "Laisser vide pour conserver l'actuel." : "Au moins 8 caractères."}
    >
      <Input
        bind:value={password}
        type="password"
        autocomplete="new-password"
        invalid={passwordInvalid}
      />
    </FormField>
    <FormField label="Rôle" hint="Les administrateurs gèrent l'équipe et les paramètres.">
      <Select bind:value={role} options={roleOptions} />
    </FormField>
  </div>

  {#if isEdit}
    <Checkbox
      bind:checked={isActive}
      label="Compte actif"
      hint="Décochez pour bloquer la connexion sans supprimer le compte."
    />
  {/if}
</form>

<style>
  .ef {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    min-width: 0;
  }
  .ef__grid {
    display: grid;
    gap: var(--space-3);
    grid-template-columns: 1fr 1fr;
  }
</style>
