//! Paramètres: a rail of categories and, for most of them, a grid of fields whose changes
//! wait in a bar at the bottom until they are saved together.
//!
//! Five categories draw themselves: the company's identity, the backups, the sync, the till
//! and the assistant.

mod assistant;
mod background;
mod company;
mod registry;
mod sync;
mod system;
mod till;

use std::collections::BTreeMap;

use motodesk_client::fiscal::{self, ProfileCheck};
use motodesk_client::settings::{self, AppSetting, SettingsBulk};
use motodesk_ui::gpui::prelude::FluentBuilder as _;
use motodesk_ui::gpui::*;
use motodesk_ui::*;
use serde_json::Value;

use crate::appearance;
use crate::auth::AuthStore;
use crate::backend::ApiExt as _;
use crate::format::humanize_error;
use crate::router::Router;
use registry::{CATEGORIES, Category, Def, Kind, category_by_id, settings_for};

/// What the bar at the bottom says about the changes that wait.
fn waiting_label(changes: usize, has_problem: bool) -> String {
    match (has_problem, changes) {
        (true, _) => "Corrigez les champs signalés avant d'enregistrer.".into(),
        (false, 0) => "Aucune modification".into(),
        (false, 1) => "1 modification non enregistrée".into(),
        (false, n) => format!("{n} modifications non enregistrées"),
    }
}

/// How many mandatory mentions are missing, in words.
fn missing_lead(count: usize) -> String {
    let what = if count == 1 { "une mention obligatoire".to_string() } else { format!("{count} mentions obligatoires") };
    format!("Il manque {what} sur vos factures. Tant que le profil est incomplet, aucun document ne peut être validé.")
}

enum Control {
    /// A line, a few lines, a number or a colour: all of them typed.
    Text(TextModel),
    Choice(SelectModel),
    Toggle,
}

struct Field {
    def: &'static Def,
    control: Control,
    _subscription: Option<Subscription>,
}

pub struct SettingsPage {
    active: &'static Category,
    /// What the service holds: only the settings that were given a value.
    stored: BTreeMap<String, Value>,
    /// Changes made on screen and not saved yet. They survive a change of category.
    staged: BTreeMap<String, Value>,
    /// Why what is typed in a field cannot be stored, by key.
    problems: BTreeMap<&'static str, String>,
    loading: bool,
    load_error: Option<String>,
    save_error: Option<String>,
    saving: bool,
    /// The mandatory mentions of an invoice, checked again after every save.
    profile: Option<ProfileCheck>,
    /// The fields of the active category.
    fields: Vec<Field>,
    /// The view of a category that draws itself.
    special: Option<AnyView>,
}

impl SettingsPage {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        // `cat=legal` opens a category directly.
        let asked = Router::global(cx).read(cx).param("cat").map(str::to_string);
        let active = asked.as_deref().and_then(category_by_id).unwrap_or(&CATEGORIES[0]);
        let mut page = Self {
            active,
            stored: BTreeMap::new(),
            staged: BTreeMap::new(),
            problems: BTreeMap::new(),
            loading: true,
            load_error: None,
            save_error: None,
            saving: false,
            profile: None,
            fields: Vec::new(),
            special: None,
        };
        page.open(active, window, cx);
        page.load(window, cx);
        page.refresh_profile(cx);
        page
    }

    fn is_admin(cx: &App) -> bool {
        AuthStore::global(cx).read(cx).is_admin()
    }

    // ---- values --------------------------------------------------------------------------------

    /// The value that applies now: the change that waits, else what is stored, else the default.
    fn value(&self, def: &Def) -> Value {
        self.staged.get(def.key).or_else(|| self.stored.get(def.key)).cloned().unwrap_or_else(|| def.default_value())
    }

    /// Keep a change for the next save. A value put back to what is stored waits for nothing.
    fn stage(&mut self, def: &'static Def, value: Value, cx: &mut Context<Self>) {
        let base = self.stored.get(def.key).cloned().unwrap_or_else(|| def.default_value());
        if registry::same(&value, &base) {
            self.staged.remove(def.key);
        } else {
            self.staged.insert(def.key.to_string(), value);
        }
        cx.notify();
    }

    /// Something was typed in the field of `def`.
    fn typed(&mut self, def: &'static Def, cx: &mut Context<Self>) {
        let Some(Control::Text(model)) = self.fields.iter().find(|field| field.def.key == def.key).map(|field| &field.control)
        else {
            return;
        };
        match def.read(&model.text(cx)) {
            Ok(value) => {
                self.problems.remove(def.key);
                self.stage(def, value, cx);
            }
            Err(problem) => {
                self.problems.insert(def.key, problem);
                self.staged.remove(def.key);
                cx.notify();
            }
        }
    }

    /// Give the app the look the stored settings ask for.
    fn apply_look(&self, cx: &mut Context<Self>) {
        let stored: Vec<AppSetting> = self
            .stored
            .iter()
            .map(|(key, value)| AppSetting { key: key.clone(), value: value.clone(), updated_at: String::new() })
            .collect();
        appearance::apply(&stored, cx);
    }

    // ---- the fields ----------------------------------------------------------------------------

    fn open(&mut self, category: &'static Category, window: &mut Window, cx: &mut Context<Self>) {
        self.active = category;
        // What was typed wrong in a field that is no longer shown is gone with it.
        self.problems.clear();
        self.fields.clear();
        self.special = None;
        if category.special {
            self.special = match category.id {
                "company" => Some(cx.new(|cx| company::CompanyView::new(window, cx)).into()),
                "system" if Self::is_admin(cx) => Some(cx.new(|cx| system::SystemView::new(window, cx)).into()),
                "sync" if Self::is_admin(cx) => Some(cx.new(|cx| sync::SyncView::new(window, cx)).into()),
                "caisse" if Self::is_admin(cx) => Some(cx.new(|cx| till::TillSettings::new(window, cx)).into()),
                "assistant" => Some(cx.new(|cx| assistant::AssistantSettings::new(window, cx)).into()),
                _ => None,
            };
        } else {
            self.fields = settings_for(category.id).map(|def| self.field(def, window, cx)).collect();
        }
    }

    fn field(&self, def: &'static Def, window: &mut Window, cx: &mut Context<Self>) -> Field {
        let value = self.value(def);
        match def.kind {
            Kind::Toggle => Field { def, control: Control::Toggle, _subscription: None },
            Kind::Choice(options) => {
                let options = options.iter().map(|(value, label)| SelectOption::new(*value, *label)).collect();
                let model = SelectModel::new(options, value.as_str(), window, cx);
                let subscription = model.on_change(window, cx, move |this: &mut Self, chosen, _, cx| {
                    if let Some(chosen) = chosen {
                        this.stage(def, Value::from(chosen), cx);
                    }
                });
                Field { def, control: Control::Choice(model), _subscription: Some(subscription) }
            }
            kind => {
                let builder = TextModel::builder().placeholder(def.placeholder).value(def.shown(&value));
                let model = if kind == Kind::Area { builder.multiline(3) } else { builder }.build(window, cx);
                let subscription = model.on_event(window, cx, move |this: &mut Self, event, _, cx| {
                    if matches!(event, TextEvent::Changed) {
                        this.typed(def, cx);
                    }
                });
                Field { def, control: Control::Text(model), _subscription: Some(subscription) }
            }
        }
    }

    /// Put in every field the value that applies now (after a load, a save, a cancel, a reset).
    fn sync_fields(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for field in &self.fields {
            let value = self.value(field.def);
            match &field.control {
                Control::Text(model) => {
                    let shown = field.def.shown(&value);
                    // Left alone when already right: the caret stays where it is.
                    if model.text(cx) != shown {
                        model.set_text(shown, window, cx);
                    }
                }
                Control::Choice(model) => {
                    if let Some(chosen) = value.as_str() {
                        model.set_value(chosen, window, cx);
                    }
                }
                Control::Toggle => {}
            }
        }
    }

    // ---- reading and writing -------------------------------------------------------------------

    fn load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.loading = true;
        cx.api_in(
            window,
            |client| async move { settings::list(&client).await },
            |this, result, window, cx| {
                this.loading = false;
                match result {
                    Ok(rows) => {
                        this.stored = rows.into_iter().map(|row| (row.key, row.value)).collect();
                        this.load_error = None;
                        this.sync_fields(window, cx);
                    }
                    Err(error) => this.load_error = Some(humanize_error(&error, "Impossible de charger les paramètres.")),
                }
                cx.notify();
            },
        );
    }

    fn refresh_profile(&mut self, cx: &mut Context<Self>) {
        cx.api(
            |client| async move { fiscal::check_profile(&client).await },
            |this, result, cx| {
                // An aid, not a gate: validating a document enforces the same list.
                this.profile = result.ok();
                cx.notify();
            },
        );
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving || self.staged.is_empty() || !self.problems.is_empty() {
            return;
        }
        let entries: SettingsBulk = self.staged.clone();
        self.saving = true;
        self.save_error = None;
        cx.notify();
        cx.api_in(
            window,
            move |client| async move { settings::set_bulk(&client, &entries).await },
            |this, result, window, cx| {
                this.saving = false;
                match result {
                    Ok(rows) => {
                        let look_changed = rows.iter().any(|row| row.key.starts_with("system."));
                        for row in rows {
                            this.stored.insert(row.key, row.value);
                        }
                        this.staged.clear();
                        this.sync_fields(window, cx);
                        if look_changed {
                            this.apply_look(cx);
                        }
                        this.refresh_profile(cx);
                        toast::success("Paramètres enregistrés.", window, cx);
                    }
                    Err(error) => this.save_error = Some(humanize_error(&error, "Erreur d'enregistrement.")),
                }
                cx.notify();
            },
        );
    }

    fn discard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.staged.clear();
        self.problems.clear();
        self.save_error = None;
        self.sync_fields(window, cx);
        cx.notify();
    }

    fn ask_reset(&mut self, def: &'static Def, window: &mut Window, cx: &mut Context<Self>) {
        let page = cx.entity();
        Confirm::new("Réinitialiser ce paramètre ?", "La valeur par défaut sera rétablie.").confirm_label("Réinitialiser").open(
            window,
            cx,
            move |window, cx| page.update(cx, |this, cx| this.reset(def, window, cx)),
        );
    }

    fn reset(&mut self, def: &'static Def, window: &mut Window, cx: &mut Context<Self>) {
        cx.api_in(
            window,
            move |client| async move { settings::reset(&client, def.key).await },
            move |this, result, window, cx| {
                match result {
                    Ok(()) => {
                        this.stored.remove(def.key);
                        this.staged.remove(def.key);
                        this.problems.remove(def.key);
                        this.sync_fields(window, cx);
                        if def.key.starts_with("system.") {
                            this.apply_look(cx);
                        }
                        this.refresh_profile(cx);
                    }
                    Err(error) => this.save_error = Some(humanize_error(&error, "Erreur de réinitialisation.")),
                }
                cx.notify();
            },
        );
    }

    fn select(&mut self, category: &'static Category, window: &mut Window, cx: &mut Context<Self>) {
        if self.active.id == category.id {
            return;
        }
        self.save_error = None;
        self.open(category, window, cx);
        Router::global(cx).update(cx, |router, _| router.set_param("cat", Some(category.id.to_string())));
        cx.notify();
    }

    // ---- drawing -------------------------------------------------------------------------------

    fn rail(&self, cx: &mut Context<Self>) -> FilterRail {
        let is_admin = Self::is_admin(cx);
        let entries = CATEGORIES.iter().map(|category| {
            let entry = ListRow::new(category.id, category.label)
                .icon(category.icon)
                .selected(self.active.id == category.id)
                .on_click(cx.listener(move |this, _, window, cx| this.select(category, window, cx)));
            if category.admin_only && !is_admin { entry.trailing(Icon::new(IconName::Lock).sm()) } else { entry }
        });
        FilterRail::new("settings-rail", "Paramètres").children(entries.collect::<Vec<_>>()).footer("Configuration de l'atelier")
    }

    /// The state of the mandatory mentions, on the two categories that are printed on invoices.
    fn mentions(&self, cx: &App) -> Option<Alert> {
        let invoicing = match self.active.id {
            "invoicing" => true,
            "legal" => false,
            _ => return None,
        };
        let profile = self.profile.as_ref()?;
        let here: Vec<_> = profile.missing.iter().filter(|mention| mention.key.starts_with("invoicing.") == invoicing).collect();
        if here.is_empty() {
            return profile.complete.then(|| {
                Alert::success().child("Toutes les mentions obligatoires sont renseignées. Les factures peuvent être validées.")
            });
        }
        let m = cx.ui().m;
        let lines = here.iter().map(|mention| {
            row()
                .items_start()
                .gap(m.space_1)
                .fs(m.fs_sm)
                .child(div().flex_none().font_weight(FontWeight::SEMIBOLD).child(mention.label.clone()))
                .child(div().min_w_0().child(format!("— {}", mention.reason)))
        });
        Some(Alert::warning().title(missing_lead(here.len())).child(col().gap(px(2.)).children(lines)))
    }

    fn cell(&self, field: &Field, locked: bool, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.ui().clone();
        let (c, m) = (theme.colors, theme.m);
        let def = field.def;
        let value = self.value(def);
        let staged = self.staged.contains_key(def.key);
        let problem = self.problems.get(def.key).cloned();
        let resettable = self.stored.contains_key(def.key) && !staged && problem.is_none() && !locked;
        let id = |part: &str| SharedString::from(format!("{part}-{}", def.key));

        let head = row()
            .justify_between()
            .gap(m.space_2)
            .min_h(m.control_h_sm)
            .child(
                row()
                    .min_w_0()
                    .gap(m.space_2)
                    .child(div().fs(m.fs_sm).font_weight(FontWeight::MEDIUM).text_color(c.text_strong).child(def.label))
                    .when(staged, |this| {
                        this.child(div().flex_none().fs(m.fs_xs).font_weight(FontWeight::MEDIUM).text_color(c.accent).child("MODIFIÉ"))
                    }),
            )
            .when(resettable, |this| {
                this.child(
                    Button::new(id("reset"))
                        .link()
                        .small()
                        .label("Réinitialiser")
                        .on_click(cx.listener(move |this, _, window, cx| this.ask_reset(def, window, cx))),
                )
            });

        let control = match &field.control {
            Control::Toggle => {
                let on = value.as_bool().unwrap_or(false);
                let page = cx.entity();
                Switch::new(id("switch"), on)
                    .label(if on { "Activé" } else { "Désactivé" })
                    .disabled(locked)
                    .on_change(move |on, _, cx| page.update(cx, |this, cx| this.stage(def, Value::from(on), cx)))
                    .into_any_element()
            }
            Control::Choice(model) => Select::new(model).disabled(locked).into_any_element(),
            Control::Text(model) => match def.kind {
                Kind::Area => Textarea::new(model).disabled(locked).into_any_element(),
                Kind::Colour(presets) => {
                    let page = cx.entity();
                    ColorField::new(id("colour"), model)
                        .presets(presets)
                        .disabled(locked)
                        .on_pick(move |_, _, cx| page.update(cx, |this, cx| this.typed(def, cx)))
                        .into_any_element()
                }
                _ => Input::new(model).invalid(problem.is_some()).disabled(locked).into_any_element(),
            },
        };
        let control = if def.unit.is_empty() {
            control
        } else {
            row()
                .gap(m.space_2)
                .child(div().flex_1().min_w_0().child(control))
                .child(div().flex_none().fs(m.fs_sm).text_color(c.text_muted).child(def.unit))
                .into_any_element()
        };

        col()
            .min_w_0()
            .gap(px(6.))
            .child(head)
            .when(!def.description.is_empty(), |this| this.child(div().fs(m.fs_xs).text_color(c.text_muted).child(def.description)))
            .child(control)
            .when_some(problem, |this, problem| this.child(div().fs(m.fs_xs).text_color(c.danger).child(problem)))
            .into_any_element()
    }

    /// The fields, two to a line; a wide one takes a line of its own.
    fn grid(&self, locked: bool, cx: &mut Context<Self>) -> Div {
        let m = cx.ui().m;
        let line = |left: AnyElement, right: Option<AnyElement>| {
            row()
                .items_start()
                .gap(m.space_5)
                .child(div().flex_1().min_w_0().child(left))
                .child(div().flex_1().min_w_0().children(right))
        };
        let mut lines = Vec::new();
        let mut waiting: Option<AnyElement> = None;
        for field in &self.fields {
            let cell = self.cell(field, locked, cx);
            if field.def.wide {
                lines.extend(waiting.take().map(|left| line(left, None)));
                lines.push(row().child(div().flex_1().min_w_0().child(cell)));
            } else if let Some(left) = waiting.take() {
                lines.push(line(left, Some(cell)));
            } else {
                waiting = Some(cell);
            }
        }
        lines.extend(waiting.map(|left| line(left, None)));
        col().gap(m.space_4).when(locked, |this| this.opacity(0.75)).children(lines)
    }

    fn body(&self, locked: bool, cx: &mut Context<Self>) -> Div {
        let m = cx.ui().m;
        let is_admin = Self::is_admin(cx);
        let mut body = col().px(m.space_5).py(m.space_4).gap(m.space_4);
        if self.loading {
            body = body.child(div().h(px(120.)).child(StateView::loading().compact().title("Chargement des paramètres…")));
        }
        if let Some(error) = self.save_error.clone().or_else(|| self.load_error.clone()) {
            body = body.child(Alert::danger().child(error));
        }
        if self.active.special {
            return match (self.active.id, self.special.clone()) {
                (_, Some(view)) => body.child(view),
                ("system", None) if !is_admin => body.child(Alert::info().child("Les sauvegardes sont réservées aux administrateurs.")),
                ("sync", None) if !is_admin => {
                    body.child(Alert::info().child("La synchronisation est gérée par les administrateurs."))
                }
                ("caisse", None) => body.child(Alert::info().child("Le rôle de caisse est réservé aux administrateurs.")),
                _ => body,
            };
        }
        if locked {
            body = body.child(Alert::info().child(
                "Ces paramètres sont réservés aux administrateurs. Vous pouvez les consulter, mais pas les modifier.",
            ));
        }
        body.children(self.mentions(cx)).child(self.grid(locked, cx))
    }
}

impl Render for SettingsPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.ui().clone();
        let (c, m) = (theme.colors, theme.m);
        let locked = self.active.admin_only && !Self::is_admin(cx);
        let has_problem = !self.problems.is_empty();
        let waiting = !self.active.special && !locked && (!self.staged.is_empty() || has_problem);

        let bar = waiting.then(|| {
            row()
                .flex_none()
                .justify_between()
                .gap(m.space_3)
                .px(m.space_5)
                .py(m.space_2)
                .bg(c.bg_surface)
                .border_t_1()
                .border_color(c.border)
                .child(div().fs(m.fs_sm).child(waiting_label(self.staged.len(), has_problem)))
                .child(
                    row()
                        .gap(m.space_2)
                        .child(
                            Button::new("settings-discard")
                                .ghost()
                                .label("Annuler")
                                .disabled(self.saving)
                                .on_click(cx.listener(|this, _, window, cx| this.discard(window, cx))),
                        )
                        .child(
                            Button::new("settings-save")
                                .primary()
                                .label("Enregistrer")
                                .loading(self.saving)
                                .disabled(self.staged.is_empty() || has_problem)
                                .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
                        ),
                )
        });

        row()
            .size_full()
            .items_stretch()
            .bg(c.bg_app)
            .child(self.rail(cx))
            .child(
                col()
                    .flex_1()
                    .min_w_0()
                    .child(PageToolbar::new(self.active.label).icon(self.active.icon).subtitle(self.active.description))
                    // One scroll position per category: a category opens at its top.
                    .child(
                        div()
                            .id(SharedString::from(format!("settings-body-{}", self.active.id)))
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .child(self.body(locked, cx)),
                    )
                    .children(bar),
            )
    }
}

#[cfg(test)]
mod tests {
    // Not `super::*`: it would bring in GPUI's own `test` attribute in place of Rust's.
    use super::{missing_lead, waiting_label};

    #[test]
    fn the_bar_counts_what_waits_and_says_when_a_field_is_wrong() {
        assert_eq!(waiting_label(0, false), "Aucune modification");
        assert_eq!(waiting_label(1, false), "1 modification non enregistrée");
        assert_eq!(waiting_label(3, false), "3 modifications non enregistrées");
        assert!(waiting_label(3, true).starts_with("Corrigez"));
    }

    #[test]
    fn the_missing_mentions_are_counted_in_words() {
        assert!(missing_lead(1).starts_with("Il manque une mention obligatoire sur"));
        assert!(missing_lead(4).starts_with("Il manque 4 mentions obligatoires sur"));
    }
}
