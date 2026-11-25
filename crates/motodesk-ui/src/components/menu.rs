//! The menu that opens on a right click.

use std::rc::Rc;

use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenu, PopupMenuItem};
use gpui_kit::{App, InteractiveElement, IntoElement, ParentElement, SharedString, Styled, Window};

use crate::components::icon::IconName;

type Handler = Rc<dyn Fn(&mut Window, &mut App)>;

/// One line of a menu.
#[derive(Clone)]
pub enum MenuEntry {
    Action { label: SharedString, icon: Option<IconName>, disabled: bool, handler: Handler },
    Separator,
}

impl MenuEntry {
    pub fn new(label: impl Into<SharedString>, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        MenuEntry::Action { label: label.into(), icon: None, disabled: false, handler: Rc::new(handler) }
    }

    pub fn separator() -> Self {
        MenuEntry::Separator
    }

    pub fn icon(mut self, name: IconName) -> Self {
        if let MenuEntry::Action { icon, .. } = &mut self {
            *icon = Some(name);
        }
        self
    }

    pub fn disabled(mut self, is_disabled: bool) -> Self {
        if let MenuEntry::Action { disabled, .. } = &mut self {
            *disabled = is_disabled;
        }
        self
    }
}

pub(crate) fn fill(mut menu: PopupMenu, items: Vec<MenuEntry>) -> PopupMenu {
    for item in items {
        menu = match item {
            MenuEntry::Separator => menu.separator(),
            MenuEntry::Action { label, icon, disabled, handler } => {
                let mut entry = PopupMenuItem::new(label).disabled(disabled);
                if let Some(icon) = icon {
                    entry = entry.icon(icon);
                }
                menu.item(entry.on_click(move |_, window, cx| handler(window, cx)))
            }
        };
    }
    menu
}

/// Give `element` a right-click menu. `items` is asked each time the menu opens, so it can
/// reflect the current state.
pub fn with_context_menu<E>(element: E, items: impl Fn(&mut Window, &mut App) -> Vec<MenuEntry> + 'static) -> impl IntoElement
where
    E: InteractiveElement + ParentElement + Styled + IntoElement + 'static,
{
    element.context_menu(move |menu, window, cx| fill(menu, items(window, cx)))
}
