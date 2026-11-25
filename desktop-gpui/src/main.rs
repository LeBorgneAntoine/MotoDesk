// Build check: a frameless window with the Kit's title bar, a text field and a button.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{ActiveTheme as _, Theme, TitleBar};
use gpui_kit::*;

struct Hello {
    name: Entity<InputState>,
}

impl Render for Hello {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let typed = self.name.read(cx).value();
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(cx.theme().background)
            .child(TitleBar::new().child("Motodesk"))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .p_6()
                    .child("Essai : saisir « élève, Noël, garçon »")
                    .child(Input::new(&self.name))
                    .child(Button::new("ok").primary().label("Valider"))
                    .child(format!("Saisi : {typed}")),
            )
    }
}

fn main() {
    gpui_kit::application().run(|cx| {
        gpui_kit::init(cx);
        let theme = Theme::global_mut(cx);
        theme.radius = px(0.);
        theme.radius_lg = px(0.);

        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(1280.), px(800.)),
                cx,
            ))),
            window_min_size: Some(size(px(960.), px(600.))),
            ..TitleBar::window_options()
        };
        gpui_kit::open_window(options, cx, |window, cx| {
            cx.new(|cx| Hello {
                name: cx.new(|cx| InputState::new(window, cx).placeholder("Nom du client")),
            })
        })
        .expect("failed to open window");
        cx.activate(true);
    });
}
