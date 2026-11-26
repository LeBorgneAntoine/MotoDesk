//! The controls. Same names as the Svelte kit (`desktop/src/lib/components`).

mod button;
mod combobox;
mod content;
mod controls;
mod data;
mod icon;
mod layout;
mod menu;
mod overlay;
mod panel;
mod table;
mod text_field;
mod tooltip;
mod wizard;

pub use button::{Button, ButtonSize, ButtonVariant};
pub use combobox::{ComboModel, ComboOption, Combobox};
pub use content::{
    Alert, Divider, Heading, HeadingSize, Picture, Preformatted, Spinner, StateKind, StateView, Status, Tag, Tone,
    picture_from_bytes,
};
pub use controls::{
    Checkbox, ColorField, FormField, IconButton, IconButtonSize, SearchField, Select, SelectModel, SelectOption, Switch,
    TabItem, Tabs,
};
pub use data::{
    Card, CardPadding, CardVariant, InfoItem, InfoList, KpiTile, ListRow, Pagination, SectionCard, TileButton, Trend,
};
pub use icon::{Icon, IconName, IconSize};
pub use layout::{FilterRail, PageToolbar, Sidebar, SidebarItem, StatusBar, StatusBarButton, TitleBar};
pub use menu::{MenuEntry, with_context_menu};
pub use overlay::{Confirm, Modal, close_modal, element, toast};
pub use panel::{MAX_DEPTH, Panel, PanelHost, PanelLayer, PanelSection, PanelStack, PanelWidth, request_close};
pub use table::{Align, Column, DataTable};
pub use text_field::{Input, TextEvent, TextModel, TextModelBuilder, Textarea};
pub use tooltip::tooltip;
pub use wizard::{DropZone, Stepper};
