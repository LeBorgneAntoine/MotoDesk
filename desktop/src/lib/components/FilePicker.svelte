<script lang="ts" module>
  export interface FilePickerApi {
    /** Opens the native file dialog. */
    open: () => void;
    /** Clears the selection so picking the same file again still fires. */
    reset: () => void;
  }
</script>

<script lang="ts">
  /**
   * FilePicker — the only place a raw `<input type="file">` lives. Screens
   * either let it render its own trigger button, or grab the handle through
   * `onReady` and open the dialog from their own affordance (a drop zone, a
   * menu item).
   */
  import Button from "./Button.svelte";
  import Icon, { type IconName } from "./Icon.svelte";

  interface Props {
    accept?: string;
    multiple?: boolean;
    /** Renders a trigger button with this label. Omit for handle-only use. */
    label?: string;
    icon?: IconName;
    variant?: "primary" | "secondary" | "ghost";
    disabled?: boolean;
    loading?: boolean;
    onFiles: (files: File[]) => void;
    onReady?: (api: FilePickerApi) => void;
  }

  let {
    accept,
    multiple = false,
    label,
    icon = "upload",
    variant = "secondary",
    disabled = false,
    loading = false,
    onFiles,
    onReady,
  }: Props = $props();

  let input: HTMLInputElement | null = $state(null);

  const api: FilePickerApi = {
    open: () => input?.click(),
    reset: () => {
      if (input) input.value = "";
    },
  };

  $effect(() => {
    if (input) onReady?.(api);
  });

  function onChange(e: Event): void {
    const el = e.currentTarget as HTMLInputElement;
    const files = Array.from(el.files ?? []);
    if (files.length > 0) onFiles(files);
    // Reset so re-picking the same path still triggers a change event.
    el.value = "";
  }
</script>

<input
  bind:this={input}
  class="fpick__input"
  type="file"
  {accept}
  {multiple}
  {disabled}
  tabindex="-1"
  aria-hidden="true"
  onchange={onChange}
/>

{#if label}
  <Button {variant} {disabled} {loading} onclick={() => api.open()}>
    {#if icon}<Icon name={icon} size={14} />{/if}
    {label}
  </Button>
{/if}

<style>
  .fpick__input {
    position: absolute;
    width: 1px;
    height: 1px;
    opacity: 0;
    pointer-events: none;
  }
</style>
