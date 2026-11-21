<script lang="ts">
  /**
   * Employee panel — create or edit a team member. Employees have a small,
   * single-block record, so the panel opens straight into the form.
   */
  import { Button, Icon, IconButton, InfoList, Panel, PanelSection, type InfoItem } from "$lib/components";
  import type { Employee } from "$lib/api/types";
  import { formatDateTime } from "$lib/format";
  import EmployeeForm, { type EmployeeFormData } from "./EmployeeForm.svelte";

  interface Props {
    layerId: string;
    mode: "create" | "edit";
    employee: Employee | null;
    saving?: boolean;
    saveError?: string | null;
    dirty?: boolean;
    onClose: () => void;
    onSubmit: (data: EmployeeFormData) => void;
    onDeactivate: () => void;
    onDelete: () => void;
    onDirtyChange: (dirty: boolean) => void;
  }

  let {
    layerId,
    mode,
    employee,
    saving = false,
    saveError = null,
    dirty = false,
    onClose,
    onSubmit,
    onDeactivate,
    onDelete,
    onDirtyChange,
  }: Props = $props();

  let form: EmployeeForm | null = $state(null);

  const meta = $derived<InfoItem[]>(
    employee
      ? [
          { label: "Rôle", value: employee.role === "admin" ? "Administrateur" : "Employé" },
          { label: "Statut", value: employee.is_active ? "Actif" : "Inactif" },
          { label: "Créé le", value: formatDateTime(employee.created_at) },
          { label: "Identifiant", value: employee.username, mono: true, placeholder: "—" },
        ]
      : [],
  );
</script>

<Panel
  {layerId}
  title={mode === "create" ? "Nouvel employé" : (employee?.full_name ?? "Employé")}
  subtitle={mode === "create" ? "Créez un accès pour un membre de l'équipe." : employee?.email}
  icon={mode === "create" ? "user-plus" : "circle-user"}
  badge={employee && !employee.is_active ? "Inactif" : undefined}
  {dirty}
  {onClose}
>
  {#snippet actions()}
    {#if mode === "edit" && employee}
      {#if employee.is_active}
        <IconButton title="Désactiver le compte" variant="danger" onclick={onDeactivate}>
          <Icon name="log-out" size={14} />
        </IconButton>
      {/if}
      <IconButton title="Supprimer définitivement" variant="danger" onclick={onDelete}>
        <Icon name="trash" size={14} />
      </IconButton>
    {/if}
  {/snippet}

  <PanelSection title="Compte" icon="circle-user">
    <EmployeeForm
      bind:this={form}
      initial={mode === "edit" ? employee : null}
      submitting={saving}
      errorMessage={saveError}
      {onSubmit}
      {onDirtyChange}
    />
  </PanelSection>

  {#if mode === "edit" && employee}
    <PanelSection title="Informations" icon="info">
      <InfoList items={meta} columns={2} dense />
    </PanelSection>
  {/if}

  {#snippet footer()}
    <Button variant="ghost" onclick={onClose} disabled={saving}>Annuler</Button>
    <Button variant="primary" loading={saving} onclick={() => form?.submit()}>
      {mode === "create" ? "Créer l'employé" : "Enregistrer"}
    </Button>
  {/snippet}
</Panel>
