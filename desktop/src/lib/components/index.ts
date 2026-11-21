/**
 * Motodesk UI kit — the single source of controls for the desktop app.
 *
 * Screens import from `$lib/components` only. If something is missing, add a
 * generic component here rather than hand-rolling markup in a route.
 * See .cursor/rules/ui-kit.mdc and .cursor/rules/ui-patterns.mdc.
 */

/* ---- Form controls ------------------------------------------------------ */
export { default as Logo } from "./Logo.svelte";
export { default as Button } from "./Button.svelte";
export { default as IconButton } from "./IconButton.svelte";
export { default as Input } from "./Input.svelte";
export { default as Textarea } from "./Textarea.svelte";
export { default as Select } from "./Select.svelte";
export { default as Combobox } from "./Combobox.svelte";
export { default as Checkbox } from "./Checkbox.svelte";
export { default as Switch } from "./Switch.svelte";
export { default as ColorField } from "./ColorField.svelte";
export { default as FormField } from "./FormField.svelte";
export { default as SearchField } from "./SearchField.svelte";
export { default as FilePicker } from "./FilePicker.svelte";
export { default as Composer } from "./Composer.svelte";
export { default as Prose } from "./Prose.svelte";

/* ---- Content & data ----------------------------------------------------- */
export { default as Heading } from "./Heading.svelte";
export { default as Icon } from "./Icon.svelte";
export { default as Tag } from "./Tag.svelte";
export { default as Alert } from "./Alert.svelte";
export { default as Spinner } from "./Spinner.svelte";
export { default as StateView } from "./StateView.svelte";
export { default as Card } from "./Card.svelte";
export { default as SectionCard } from "./SectionCard.svelte";
export { default as InfoList } from "./InfoList.svelte";
export { default as KpiTile } from "./KpiTile.svelte";
export { default as DataTable } from "./DataTable.svelte";
export { default as Pagination } from "./Pagination.svelte";
export { default as ListRow } from "./ListRow.svelte";
export { default as TileButton } from "./TileButton.svelte";
export { default as Divider } from "./Divider.svelte";
export { default as PosKey } from "./PosKey.svelte";
export { default as PosProductTile } from "./PosProductTile.svelte";
export { default as PosPayBar } from "./PosPayBar.svelte";
export { default as PosCartLine } from "./PosCartLine.svelte";
export { default as PosQuickItem } from "./PosQuickItem.svelte";
export { default as PosSearchHit } from "./PosSearchHit.svelte";
export { default as PosChrome } from "./PosChrome.svelte";

/* ---- Layout & navigation ------------------------------------------------ */
export { default as PageToolbar } from "./PageToolbar.svelte";
export { default as FilterRail } from "./FilterRail.svelte";
export { default as Toolbar } from "./Toolbar.svelte";
export { default as ToolbarButton } from "./ToolbarButton.svelte";
export { default as Tabs } from "./Tabs.svelte";
export { default as Stepper } from "./Stepper.svelte";
export { default as TitleBar } from "./TitleBar.svelte";
export { default as Sidebar } from "./Sidebar.svelte";
export { default as StatusBar } from "./StatusBar.svelte";
export { default as StatusBarButton } from "./StatusBarButton.svelte";

/* ---- Overlays ----------------------------------------------------------- */
export { default as Modal } from "./Modal.svelte";
export { default as ConfirmDialog } from "./ConfirmDialog.svelte";
export { default as ToastHost } from "./ToastHost.svelte";
export { default as ContextMenuHost } from "./ContextMenuHost.svelte";
export { default as Panel } from "./Panel.svelte";
export { default as PanelSection } from "./PanelSection.svelte";
export { default as PanelHost } from "./PanelHost.svelte";

/* ---- Types -------------------------------------------------------------- */
export type { TabItem } from "./Tabs.svelte";
export type { DataTableAlign, DataTableColumn } from "./DataTable.svelte";
export type { SelectOption } from "./Select.svelte";
export type { ComboboxOption } from "./Combobox.svelte";
export type { IconName } from "./Icon.svelte";
export type { SidebarItem } from "./Sidebar.svelte";
export type { InfoItem } from "./InfoList.svelte";
export type { StepItem } from "./Stepper.svelte";
export type { FilePickerApi } from "./FilePicker.svelte";

export {
  useTitleBarMenus,
  titleBarMenus,
  isDivider,
  type TitleBarMenu,
  type TitleBarMenuItem,
  type TitleBarMenuAction,
  type TitleBarMenuDivider,
} from "$lib/menus/titlebar-menus.svelte";

export {
  contextMenu,
  contextMenuAction,
  isContextDivider,
  type ContextMenuItem,
  type ContextMenuAction,
  type ContextMenuDivider,
} from "$lib/contextMenu/contextMenu.svelte";
