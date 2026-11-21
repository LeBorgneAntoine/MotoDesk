<script lang="ts">
  import {
    Button,
    ColorField,
    FormField,
    Heading,
    Icon,
    IconButton,
    Input,
    Select,
    StateView,
    ToolbarButton,
    type SelectOption,
  } from "$lib/components";
  import type {
    Element,
    FontWeight,
    ImageEl,
    RectEl,
    TableEl,
    TableColumn,
    TextAlign,
    TextEl,
  } from "$lib/templates/schema";
  import { isImage, isRect, isTable, isText } from "$lib/templates/schema";
  import { fieldsForList, findListByPath, listsForKind } from "$lib/templates/variables";
  import type { TemplateKind } from "$lib/api/templates";
  import RichTextEditor, { type RichTextEditorApi } from "./RichTextEditor.svelte";

  interface Props {
    element: Element | null;
    kind: TemplateKind;
    onChange: (el: Element) => void;
    onDelete: () => void;
    /** Reports the panel's rich-text editor API up so the variables panel
     *  can insert tokens at the caret. */
    onContentEditorReady?: (api: RichTextEditorApi | null) => void;
    onContentEditorFocus?: (focused: boolean) => void;
  }

  let {
    element,
    kind,
    onChange,
    onDelete,
    onContentEditorReady,
    onContentEditorFocus,
  }: Props = $props();

  const alignOptions: SelectOption<TextAlign>[] = [
    { value: "left",   label: "Gauche" },
    { value: "center", label: "Centré" },
    { value: "right",  label: "Droite" },
  ];
  const weightOptions: SelectOption<FontWeight>[] = [
    { value: "normal", label: "Normal" },
    { value: "bold",   label: "Gras"   },
  ];
  const fitOptions: SelectOption<"contain" | "fill">[] = [
    { value: "contain", label: "Contenir (préserve le ratio)" },
    { value: "fill",    label: "Étirer (remplir la zone)"     },
  ];
  const formatOptions: SelectOption<"text" | "qty" | "price" | "percent">[] = [
    { value: "text",    label: "Texte"      },
    { value: "qty",     label: "Quantité"   },
    { value: "price",   label: "Prix"       },
    { value: "percent", label: "Pourcentage"},
  ];

  const listBindingOptions: SelectOption<string>[] = $derived(
    listsForKind(kind).map((l) => ({ value: l.path, label: l.label })),
  );

  /** Schema attribute options for the current list binding, plus a "custom" escape. */
  function attrOptionsFor(listPath: string): SelectOption<string>[] {
    const fields = fieldsForList(listPath);
    return [
      ...fields.map((f) => ({ value: f.binding, label: `${f.label} · ${f.binding}` })),
      { value: "__custom__", label: "Personnalisé…" },
    ];
  }

  function setNumberField<E extends Element, K extends keyof E>(
    el: E,
    field: K,
    val: string,
  ) {
    const n = Number(val);
    if (!Number.isFinite(n)) return;
    onChange({ ...el, [field]: n } as E);
  }

  // ---- Table column helpers --------------------------------------------

  function setColumn(el: TableEl, i: number, patch: Partial<TableColumn>) {
    const columns = el.columns.map((c, idx) => (idx === i ? { ...c, ...patch } : c));
    onChange({ ...el, columns });
  }

  /** When user picks a known field, auto-fill header + format + align from the list schema. */
  function pickField(el: TableEl, i: number, fieldKey: string) {
    if (fieldKey === "__custom__") {
      setColumn(el, i, { binding: "" });
      return;
    }
    const field = fieldsForList(el.listBinding).find((f) => f.binding === fieldKey);
    if (!field) return;
    const align: TableColumn["align"] =
      field.format === "qty" || field.format === "price" || field.format === "percent"
        ? "right"
        : "left";
    setColumn(el, i, {
      binding: field.binding,
      header: el.columns[i].header && el.columns[i].header !== ""
        ? el.columns[i].header
        : field.label,
      format: field.format,
      align,
    });
  }

  function addColumn(el: TableEl) {
    const fields = fieldsForList(el.listBinding);
    const usedBindings = new Set(el.columns.map((c) => c.binding));
    const next = fields.find((f) => !usedBindings.has(f.binding)) ?? fields[0];
    const col: TableColumn = next
      ? {
          header: next.label,
          binding: next.binding,
          widthPct: Math.max(8, Math.floor(100 / (el.columns.length + 1))),
          format: next.format,
          align: next.format === "qty" || next.format === "price" || next.format === "percent"
            ? "right" : "left",
        }
      : {
          header: "Colonne",
          binding: "",
          widthPct: 15,
          format: "text",
          align: "left",
        };
    onChange({ ...el, columns: [...el.columns, col] });
  }

  function removeColumn(el: TableEl, i: number) {
    if (el.columns.length <= 1) return;
    const columns = el.columns.filter((_, idx) => idx !== i);
    onChange({ ...el, columns });
  }

  /** Swap a column with its neighbour. */
  function moveColumn(el: TableEl, from: number, to: number) {
    if (to < 0 || to >= el.columns.length || from === to) return;
    const columns = el.columns.slice();
    const [moved] = columns.splice(from, 1);
    columns.splice(to, 0, moved);
    onChange({ ...el, columns });
  }

  /** Switching the list source: clear bindings whose binding no longer exists in the new schema. */
  function changeListBinding(el: TableEl, newPath: string) {
    const fields = fieldsForList(newPath);
    const valid = new Set(fields.map((f) => f.binding));
    const columns = el.columns.map((c) =>
      valid.has(c.binding) ? c : { ...c, binding: "" },
    );
    onChange({ ...el, listBinding: newPath, columns });
  }

  /** Apply ALL fields from the current list as columns (equal widths, formatted). */
  function fillAllColumns(el: TableEl) {
    const fields = fieldsForList(el.listBinding);
    if (fields.length === 0) return;
    const widthPct = Math.max(5, Math.floor(100 / fields.length));
    const columns: TableColumn[] = fields.map((f) => ({
      header: f.label,
      binding: f.binding,
      widthPct,
      format: f.format,
      align: f.format === "qty" || f.format === "price" || f.format === "percent"
        ? "right" : "left",
    }));
    onChange({ ...el, columns });
  }

  // ---- Drag-reorder for columns ---------------------------------------

  let dragFrom = $state<number | null>(null);
  let dragOver = $state<number | null>(null);

  function onColDragStart(i: number, e: DragEvent) {
    dragFrom = i;
    e.dataTransfer?.setData("application/x-mg-col", String(i));
    if (e.dataTransfer) e.dataTransfer.effectAllowed = "move";
  }
  function onColDragOver(i: number, e: DragEvent) {
    if (dragFrom == null) return;
    e.preventDefault();
    dragOver = i;
    if (e.dataTransfer) e.dataTransfer.dropEffect = "move";
  }
  function onColDrop(el: TableEl, i: number, e: DragEvent) {
    e.preventDefault();
    if (dragFrom == null || dragFrom === i) {
      dragFrom = null;
      dragOver = null;
      return;
    }
    moveColumn(el, dragFrom, i);
    dragFrom = null;
    dragOver = null;
  }
  function onColDragEnd() {
    dragFrom = null;
    dragOver = null;
  }
</script>

<div class="props">
  <Heading level={4} size="xs" caps>Propriétés</Heading>

  {#if !element}
    <StateView
      kind="empty"
      compact
      icon="circle-dot"
      title="Aucune sélection"
      body="Sélectionnez un élément sur la page pour modifier ses propriétés."
    />
  {:else}
    <div class="props__body">
      <!-- Position / size grid -->
      <div class="props__section">
        <div class="props__section-title">Position et taille (mm)</div>
        <div class="props__grid4">
          <FormField label="X">
            <Input
              type="number"
              value={String(element.x)}
              step="1"
              oninput={(e) => setNumberField(element!, "x", (e.target as HTMLInputElement).value)}
            />
          </FormField>
          <FormField label="Y">
            <Input
              type="number"
              value={String(element.y)}
              step="1"
              oninput={(e) => setNumberField(element!, "y", (e.target as HTMLInputElement).value)}
            />
          </FormField>
          <FormField label="L">
            <Input
              type="number"
              value={String(element.w)}
              step="1"
              min="1"
              oninput={(e) => setNumberField(element!, "w", (e.target as HTMLInputElement).value)}
            />
          </FormField>
          <FormField label="H">
            <Input
              type="number"
              value={String(element.h)}
              step="1"
              min="1"
              oninput={(e) => setNumberField(element!, "h", (e.target as HTMLInputElement).value)}
            />
          </FormField>
        </div>
      </div>

      <!-- Text-specific -->
      {#if isText(element)}
        {@const t = element as TextEl}
        <div class="props__section">
          <div class="props__section-title">Contenu</div>
          <FormField label="Texte" hint="Cliquez une variable à gauche pour l'insérer au curseur.">
            <RichTextEditor
              value={t.content}
              rows={3}
              placeholder="Saisissez votre texte ou insérez une variable."
              onChange={(v) => onChange({ ...t, content: v })}
              onReady={(api) => onContentEditorReady?.(api)}
              onFocus={() => onContentEditorFocus?.(true)}
              onBlur={() => onContentEditorFocus?.(false)}
            />
          </FormField>

          <div class="props__grid2">
            <FormField label="Taille (pt)">
              <Input
                type="number"
                value={String(t.fontSize)}
                step="0.5"
                min="6"
                max="72"
                oninput={(e) => setNumberField(t, "fontSize", (e.target as HTMLInputElement).value)}
              />
            </FormField>
            <FormField label="Graisse">
              <Select
                value={t.fontWeight}
                options={weightOptions}
                onchange={(e) => onChange({ ...t, fontWeight: (e.target as HTMLSelectElement).value as FontWeight })}
              />
            </FormField>
          </div>

          <div class="props__grid2">
            <FormField label="Alignement">
              <Select
                value={t.align}
                options={alignOptions}
                onchange={(e) => onChange({ ...t, align: (e.target as HTMLSelectElement).value as TextAlign })}
              />
            </FormField>
            <FormField label="Couleur">
              <ColorField
                value={t.color}
                ariaLabel="Couleur du texte"
                onchange={(color) => onChange({ ...t, color })}
              />
            </FormField>
          </div>
        </div>
      {/if}

      <!-- Image-specific -->
      {#if isImage(element)}
        {@const im = element as ImageEl}
        <div class="props__section">
          <div class="props__section-title">Image</div>
          <FormField label="Source" hint="URL ou variable (ex: variable du logo de l'atelier).">
            <Input
              value={im.src}
              placeholder="https://… ou variable"
              oninput={(e) => onChange({ ...im, src: (e.target as HTMLInputElement).value })}
            />
          </FormField>
          <FormField label="Ajustement">
            <Select
              value={im.fit}
              options={fitOptions}
              onchange={(e) => onChange({ ...im, fit: (e.target as HTMLSelectElement).value as "contain" | "fill" })}
            />
          </FormField>
        </div>
      {/if}

      <!-- Table-specific -->
      {#if isTable(element)}
        {@const tb = element as TableEl}
        {@const listDef = findListByPath(tb.listBinding)}
        {@const attrOptions = attrOptionsFor(tb.listBinding)}
        <div class="props__section">
          <div class="props__section-title">Tableau</div>

          <FormField
            label="Source de la liste"
            hint={listDef
              ? `${listDef.fields.length} attributs disponibles.`
              : "Aucun schéma reconnu pour cette source."}
          >
            <Select
              value={tb.listBinding}
              options={listBindingOptions}
              onchange={(e) => changeListBinding(tb, (e.target as HTMLSelectElement).value)}
            />
          </FormField>

          <div class="props__grid2">
            <FormField label="Hauteur ligne (mm)">
              <Input
                type="number"
                value={String(tb.rowHeight)}
                step="0.5"
                min="3"
                oninput={(e) => setNumberField(tb, "rowHeight", (e.target as HTMLInputElement).value)}
              />
            </FormField>
            <FormField label="Hauteur en-tête (mm)">
              <Input
                type="number"
                value={String(tb.headerHeight)}
                step="0.5"
                min="3"
                oninput={(e) => setNumberField(tb, "headerHeight", (e.target as HTMLInputElement).value)}
              />
            </FormField>
          </div>

          <FormField label="Taille texte (pt)">
            <Input
              type="number"
              value={String(tb.fontSize)}
              step="0.5"
              min="6"
              max="20"
              oninput={(e) => setNumberField(tb, "fontSize", (e.target as HTMLInputElement).value)}
            />
          </FormField>

          <div class="props__cols-head">
            <span class="props__section-title">Colonnes</span>
            <div class="props__cols-actions">
              <ToolbarButton
                icon="zap"
                label="Auto"
                title="Reconstruire à partir du schéma"
                onclick={() => fillAllColumns(tb)}
              />
              <ToolbarButton icon="plus" label="Ajouter" onclick={() => addColumn(tb)} />
            </div>
          </div>

          <div class="props__cols">
            {#each tb.columns as col, i (i)}
              {@const fieldKey = attrOptions.some((o) => o.value === col.binding) ? col.binding : "__custom__"}
              <div
                class="propscol"
                class:propscol--over={dragOver === i && dragFrom !== i}
                role="group"
                aria-label="Colonne {i + 1}"
                ondragover={(e) => onColDragOver(i, e)}
                ondrop={(e) => onColDrop(tb, i, e)}
              >
                <div class="propscol__row propscol__row--head">
                  <IconButton
                    size="xs"
                    title="Réordonner la colonne — glisser pour déplacer"
                    draggable
                    ondragstart={(e) => onColDragStart(i, e)}
                    ondragend={onColDragEnd}
                  >
                    <Icon name="grip-vertical" size={12} />
                  </IconButton>
                  <Select
                    value={fieldKey}
                    options={attrOptions}
                    onchange={(e) => pickField(tb, i, (e.target as HTMLSelectElement).value)}
                  />
                  <IconButton
                    title="Supprimer la colonne"
                    size="sm"
                    variant="danger"
                    disabled={tb.columns.length <= 1}
                    onclick={() => removeColumn(tb, i)}
                  >
                    <Icon name="trash" size={12} />
                  </IconButton>
                </div>

                {#if fieldKey === "__custom__"}
                  <div class="propscol__row">
                    <Input
                      value={col.binding}
                      placeholder="champ (ex: label)"
                      oninput={(e) => setColumn(tb, i, { binding: (e.target as HTMLInputElement).value })}
                    />
                  </div>
                {/if}

                <div class="propscol__row">
                  <Input
                    value={col.header}
                    placeholder="En-tête de colonne"
                    oninput={(e) => setColumn(tb, i, { header: (e.target as HTMLInputElement).value })}
                  />
                  <Input
                    type="number"
                    value={String(col.widthPct)}
                    placeholder="%"
                    min="1"
                    max="100"
                    step="1"
                    oninput={(e) => {
                      const n = Number((e.target as HTMLInputElement).value);
                      if (Number.isFinite(n)) setColumn(tb, i, { widthPct: n });
                    }}
                  />
                </div>
                <div class="propscol__row">
                  <Select
                    value={col.format ?? "text"}
                    options={formatOptions}
                    onchange={(e) => setColumn(tb, i, { format: (e.target as HTMLSelectElement).value as "text" | "qty" | "price" | "percent" })}
                  />
                  <Select
                    value={col.align ?? "left"}
                    options={alignOptions}
                    onchange={(e) => setColumn(tb, i, { align: (e.target as HTMLSelectElement).value as TextAlign })}
                  />
                </div>
              </div>
            {/each}
          </div>
        </div>
      {/if}

      <!-- Rect-specific -->
      {#if isRect(element)}
        {@const r = element as RectEl}
        <div class="props__section">
          <div class="props__section-title">Cadre</div>
          <div class="props__grid2">
            <FormField label="Bordure">
              <ColorField
                value={r.stroke}
                ariaLabel="Couleur de la bordure"
                onchange={(stroke) => onChange({ ...r, stroke })}
              />
            </FormField>
            <FormField label="Épaisseur (mm)">
              <Input
                type="number"
                value={String(r.strokeWidth)}
                step="0.1"
                min="0"
                oninput={(e) => setNumberField(r, "strokeWidth", (e.target as HTMLInputElement).value)}
              />
            </FormField>
          </div>
          <FormField label="Remplissage">
            <Input
              value={r.fill}
              placeholder="transparent ou #ffffff"
              oninput={(e) => onChange({ ...r, fill: (e.target as HTMLInputElement).value })}
            />
          </FormField>
        </div>
      {/if}

      <div class="props__danger">
        <Button variant="danger" fullWidth onclick={onDelete}>
          <Icon name="trash" size={13} /> Supprimer l'élément
        </Button>
      </div>
    </div>
  {/if}
</div>

<style>
  .props {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding: var(--space-3);
    height: 100%;
    min-height: 0;
    overflow-y: auto;
  }
  .props__body {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .props__section {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .props__section-title {
    font-size: var(--fs-xs);
    font-weight: var(--fw-semibold);
    color: var(--color-text);
    text-transform: uppercase;
    letter-spacing: var(--tracking-caps);
    line-height: 1;
  }
  .props__grid4 {
    display: grid;
    grid-template-columns: 1fr 1fr 1fr 1fr;
    gap: var(--space-2);
  }
  .props__grid2 {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--space-2);
  }

  .props__cols-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-top: var(--space-2);
  }
  .props__cols-actions {
    display: inline-flex;
    gap: var(--space-1);
  }
  .props__cols {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .propscol {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    padding: var(--space-2);
    background: var(--color-bg-app);
    border: var(--border-w) solid var(--color-border-subtle);
    transition: border-color 80ms ease, background 80ms ease;
  }
  .propscol--over {
    border-color: var(--color-accent);
    background: var(--color-bg-hover);
  }
  .propscol__row {
    display: grid;
    grid-template-columns: 1fr auto;
    align-items: center;
    gap: var(--space-1);
  }
  .propscol__row > :global(*:first-child) {
    min-width: 0;
  }
  .propscol__row--head {
    grid-template-columns: auto 1fr auto;
  }
  .props__danger {
    margin-top: var(--space-3);
    padding-top: var(--space-3);
    border-top: var(--border-w) solid var(--color-border-subtle);
  }
</style>
