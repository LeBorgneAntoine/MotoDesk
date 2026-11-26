//! Menus: the one that opens on a right click, and the strip of menus in the title bar with
//! the keyboard shortcuts its entries advertise.

use std::rc::Rc;

use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button as KitButton, ButtonVariants as _};
use gpui_kit::component::menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::{App, InteractiveElement, IntoElement, Keystroke, ParentElement, RenderOnce, SharedString, Styled, Window, div};

use crate::components::icon::IconName;
use crate::style::{StyleExt as _, row};
use crate::theme::{FONT_MONO, Themed as _};

type Handler = Rc<dyn Fn(&mut Window, &mut App)>;

/// One line of a menu.
#[derive(Clone)]
pub enum MenuEntry {
    Action {
        label: SharedString,
        icon: Option<IconName>,
        disabled: bool,
        /// The keys shown at the end of the line, as the user reads them (« Ctrl+Maj+C »).
        shortcut: Option<SharedString>,
        /// Whether pressing those keys runs the entry from anywhere on the screen, or the hint
        /// only names keys the screen handles itself.
        bound: bool,
        handler: Handler,
    },
    Separator,
}

impl MenuEntry {
    pub fn new(label: impl Into<SharedString>, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        MenuEntry::Action { label: label.into(), icon: None, disabled: false, shortcut: None, bound: false, handler: Rc::new(handler) }
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

    /// The keys that run this entry, written as the user reads them: « Ctrl+N », « Ctrl+Maj+R »,
    /// « F5 ». They are shown at the end of the line, and [`MenuGroup::run_shortcut`] finds the
    /// entry by them.
    pub fn shortcut(mut self, keys: impl Into<SharedString>) -> Self {
        if let MenuEntry::Action { shortcut, bound, .. } = &mut self {
            *shortcut = Some(keys.into());
            *bound = true;
        }
        self
    }

    /// Keys shown at the end of the line that the screen handles by itself (an editor with its
    /// own keyboard): the menu only reminds the user of them.
    pub fn hint(mut self, keys: impl Into<SharedString>) -> Self {
        if let MenuEntry::Action { shortcut, bound, .. } = &mut self {
            *shortcut = Some(keys.into());
            *bound = false;
        }
        self
    }
}

pub(crate) fn fill(mut menu: PopupMenu, items: Vec<MenuEntry>) -> PopupMenu {
    for item in items {
        menu = match item {
            MenuEntry::Separator => menu.separator(),
            MenuEntry::Action { label, icon, disabled, shortcut, handler, .. } => {
                let mut entry = match shortcut {
                    // The keys go at the end of the line, apart from the label.
                    Some(keys) => PopupMenuItem::element(move |_, cx| {
                        let theme = cx.ui();
                        let (c, m) = (theme.colors, theme.m);
                        row()
                            .w_full()
                            .gap(m.space_5)
                            .justify_between()
                            .child(label.clone())
                            .child(div().font_family(FONT_MONO).fs(m.fs_xs).text_color(c.text_subtle).child(keys.clone()))
                    }),
                    None => PopupMenuItem::new(label),
                }
                .disabled(disabled);
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

// ---- the menus of the title bar ----------------------------------------------------------------

/// One menu of the title bar: « Fichier », « Affichage ».
#[derive(Clone)]
pub struct MenuGroup {
    pub label: SharedString,
    pub items: Vec<MenuEntry>,
}

impl MenuGroup {
    pub fn new(label: impl Into<SharedString>, items: Vec<MenuEntry>) -> Self {
        Self { label: label.into(), items }
    }

    /// Run the entry of `groups` that advertises the keys `pressed` (see [`pressed_shortcut`]).
    /// Returns whether one did. A disabled entry does not run.
    pub fn run_shortcut(groups: &[MenuGroup], pressed: &str, window: &mut Window, cx: &mut App) -> bool {
        let found = groups.iter().flat_map(|group| &group.items).find_map(|item| match item {
            MenuEntry::Action { shortcut: Some(keys), bound: true, disabled: false, handler, .. } if normalize_shortcut(keys) == pressed => {
                Some(handler.clone())
            }
            _ => None,
        });
        match found {
            Some(handler) => {
                handler(window, cx);
                true
            }
            None => false,
        }
    }
}

/// The menus of the screen, in the title bar: each opens under its name.
#[derive(IntoElement)]
pub struct MenuStrip {
    groups: Vec<MenuGroup>,
}

impl MenuStrip {
    pub fn new(groups: Vec<MenuGroup>) -> Self {
        Self { groups }
    }
}

impl RenderOnce for MenuStrip {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let m = cx.ui().m;
        row().h_full().gap(m.space_1).children(self.groups.into_iter().enumerate().map(|(index, group)| {
            let items = group.items;
            KitButton::new(("menu-strip", index)).ghost().small().label(group.label).dropdown_menu(move |menu, _, _| fill(menu, items.clone()))
        }))
    }
}

/// Whether the keyboard is in a text field: copy, paste and undo are then the field's.
pub fn typing_in_a_field(window: &Window) -> bool {
    window.context_stack().iter().any(|context| context.contains("Input"))
}

/// A shortcut as it is written in a menu (« Ctrl+Maj+C ») in the form keys are compared in:
/// `ctrl+shift+c`.
pub fn normalize_shortcut(written: &str) -> String {
    let lowered: String = written.to_lowercase().chars().filter(|c| !c.is_whitespace()).collect();
    lowered.replace("contrôle", "ctrl").replace("control", "ctrl").replace("maj", "shift").replace("cmd", "ctrl")
}

/// On an AZERTY keyboard the digit row types these until Shift is held. A shortcut names the
/// key, so with Ctrl held they read as the digit printed on it.
fn digit_under(key: &str) -> Option<&'static str> {
    Some(match key {
        "&" => "1",
        "é" => "2",
        "\"" => "3",
        "'" => "4",
        "(" => "5",
        "-" => "6",
        "è" => "7",
        "_" => "8",
        "ç" => "9",
        "à" => "0",
        _ => return None,
    })
}

/// The comparable form of keys held and a key pressed.
fn shortcut_of(control: bool, shift: bool, alt: bool, key: &str) -> String {
    let key = key.to_lowercase();
    let key = if control || alt { digit_under(&key).map(str::to_string).unwrap_or(key) } else { key };
    let mut parts = Vec::new();
    if control {
        parts.push("ctrl");
    }
    if shift {
        parts.push("shift");
    }
    if alt {
        parts.push("alt");
    }
    parts.push(&key);
    parts.join("+")
}

/// The keys a text field owns: copy, paste, undo and friends are never menu shortcuts there.
fn belongs_to_text_field(pressed: &str, in_field: bool) -> bool {
    in_field && pressed.strip_prefix("ctrl+").is_some_and(|key| ["c", "x", "v", "a", "z", "y"].contains(&key))
}

/// What a keystroke is as a menu shortcut, or `None` when it cannot be one here: a plain
/// letter, or keys that belong to the text field the keyboard is in.
pub fn pressed_shortcut(keystroke: &Keystroke, in_field: bool) -> Option<String> {
    let modifiers = keystroke.modifiers;
    let control = modifiers.control || modifiers.platform;
    let function_key = keystroke.key.len() > 1 && keystroke.key.starts_with('f') && keystroke.key[1..].chars().all(|c| c.is_ascii_digit());
    // Without Ctrl or Alt only the function keys are shortcuts: everything else is typing.
    if !(control || modifiers.alt || function_key) {
        return None;
    }
    let pressed = shortcut_of(control, modifiers.shift, modifiers.alt, &keystroke.key);
    (!belongs_to_text_field(&pressed, in_field)).then_some(pressed)
}

#[cfg(test)]
mod tests {
    use gpui_kit::{Keystroke, Modifiers};

    use super::{belongs_to_text_field, normalize_shortcut, pressed_shortcut, shortcut_of};

    fn keystroke(control: bool, shift: bool, key: &str) -> Keystroke {
        Keystroke { modifiers: Modifiers { control, shift, ..Default::default() }, key: key.into(), key_char: None }
    }

    #[test]
    fn a_shortcut_reads_the_same_written_and_pressed() {
        assert_eq!(normalize_shortcut("Ctrl+N"), "ctrl+n");
        assert_eq!(normalize_shortcut("Ctrl + Maj + C"), "ctrl+shift+c");
        assert_eq!(normalize_shortcut("F5"), "f5");
        assert_eq!(shortcut_of(true, false, false, "N"), "ctrl+n");
        assert_eq!(shortcut_of(true, true, false, "c"), "ctrl+shift+c");
        assert_eq!(pressed_shortcut(&keystroke(true, true, "r"), false).as_deref(), Some("ctrl+shift+r"));
        assert_eq!(pressed_shortcut(&keystroke(false, false, "f5"), true).as_deref(), Some("f5"));
        assert_eq!(pressed_shortcut(&keystroke(false, false, "f2"), false).as_deref(), Some("f2"));
    }

    #[test]
    fn typing_is_not_a_shortcut() {
        // A plain letter, with or without Shift, is text; so is « f » alone.
        assert_eq!(pressed_shortcut(&keystroke(false, false, "n"), false), None);
        assert_eq!(pressed_shortcut(&keystroke(false, true, "n"), false), None);
        assert_eq!(pressed_shortcut(&keystroke(false, false, "f"), false), None);
        assert_eq!(pressed_shortcut(&keystroke(false, false, "enter"), false), None);
        // In a field, copy, paste and undo are the field's; the others still reach the menus.
        assert!(belongs_to_text_field("ctrl+c", true) && belongs_to_text_field("ctrl+z", true));
        assert!(!belongs_to_text_field("ctrl+c", false) && !belongs_to_text_field("ctrl+n", true) && !belongs_to_text_field("f5", true));
        assert_eq!(pressed_shortcut(&keystroke(true, false, "v"), true), None);
        assert_eq!(pressed_shortcut(&keystroke(true, false, "n"), true).as_deref(), Some("ctrl+n"));
    }

    #[test]
    fn the_digit_row_of_a_french_keyboard_is_read_as_digits() {
        assert_eq!(shortcut_of(true, false, false, "&"), "ctrl+1");
        assert_eq!(shortcut_of(true, false, false, "é"), "ctrl+2");
        assert_eq!(shortcut_of(true, false, false, "\""), "ctrl+3");
        assert_eq!(shortcut_of(true, false, false, "1"), "ctrl+1");
        // Without Ctrl it is what it types.
        assert_eq!(shortcut_of(false, false, false, "&"), "&");
    }
}
