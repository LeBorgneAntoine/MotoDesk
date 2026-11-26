//! The team of an online company: who has an access, with which role, and whether it is
//! switched on.

use std::rc::Rc;

use motodesk_client::employees::{self, CreateEmployee, Employee, Role, UpdateEmployee, initials};
use motodesk_ui::gpui::prelude::FluentBuilder as _;
use motodesk_ui::gpui::*;
use motodesk_ui::*;

use crate::auth::AuthStore;
use crate::backend::ApiExt as _;
use crate::format::{self, humanize_error};

const EMPLOYEE: &str = "employee";
const NEW: &str = "new";
const ALL: &str = "all";

fn is_active(employee: &Employee) -> bool {
    employee.is_active.unwrap_or(true)
}

/// The fields of the form, as text: what "has anything changed?" compares.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct Snapshot {
    full_name: String,
    email: String,
    username: String,
    password: String,
    role: String,
    is_active: bool,
}

struct EmployeeForm {
    /// The employee being edited; `None` for a new one.
    editing: Option<Employee>,
    full_name: TextModel,
    email: TextModel,
    username: TextModel,
    password: TextModel,
    role: SelectModel,
    is_active: bool,
    /// A first attempt to send was made: from then on the fields say what is wrong.
    touched: bool,
    started_as: Snapshot,
    _subscriptions: Vec<Subscription>,
}

/// `a@b.c`, loosely: something, an at sign, something with a dot.
fn looks_like_an_email(text: &str) -> bool {
    let text = text.trim();
    let Some((local, domain)) = text.split_once('@') else { return false };
    !local.is_empty()
        && !text.contains(char::is_whitespace)
        && !domain.contains('@')
        && domain.split_once('.').is_some_and(|(host, tld)| !host.is_empty() && !tld.is_empty())
}

impl EmployeeForm {
    fn new(editing: Option<Employee>, window: &mut Window, cx: &mut Context<CloudTeam>) -> Self {
        let e = editing.as_ref();
        let text = |placeholder: &'static str, value: &str, window: &mut Window, cx: &mut Context<CloudTeam>| {
            TextModel::builder().placeholder(placeholder).value(value.to_string()).build(window, cx)
        };
        let role = e.map(Role::of).unwrap_or_default();
        let mut form = Self {
            full_name: text("Jean Dupont", e.map(|e| e.full_name.as_str()).unwrap_or_default(), window, cx),
            email: text("jean@atelier.fr", e.map(|e| e.email.as_str()).unwrap_or_default(), window, cx),
            username: text("jdupont", e.and_then(|e| e.username.as_deref()).unwrap_or_default(), window, cx),
            password: TextModel::builder().password().build(window, cx),
            role: SelectModel::new(
                vec![SelectOption::new("employee", "Employé"), SelectOption::new("admin", "Administrateur")],
                Some(role.value()),
                window,
                cx,
            ),
            is_active: e.map(is_active).unwrap_or(true),
            touched: false,
            started_as: Snapshot::default(),
            editing,
            _subscriptions: Vec::new(),
        };
        form.started_as = form.snapshot(cx);
        let mut subscriptions: Vec<Subscription> = [&form.full_name, &form.email, &form.username, &form.password]
            .into_iter()
            .map(|field| {
                field.on_event(window, cx, |page, event, window, cx| match event {
                    TextEvent::Changed => page.form_changed(cx),
                    TextEvent::Submitted { .. } => page.submit(window, cx),
                    _ => {}
                })
            })
            .collect();
        subscriptions.push(form.role.on_change(window, cx, |page, _, _, cx| page.form_changed(cx)));
        form._subscriptions = subscriptions;
        form
    }

    fn snapshot(&self, cx: &App) -> Snapshot {
        Snapshot {
            full_name: self.full_name.text(cx),
            email: self.email.text(cx),
            username: self.username.text(cx),
            password: self.password.text(cx),
            role: self.role.value(cx).unwrap_or_default(),
            is_active: self.is_active,
        }
    }

    fn is_dirty(&self, cx: &App) -> bool {
        self.snapshot(cx) != self.started_as
    }

    /// (name missing, email invalid, password too short), once a send was attempted.
    fn problems(&self, cx: &App) -> (bool, bool, bool) {
        if !self.touched {
            return (false, false, false);
        }
        let now = self.snapshot(cx);
        let password = now.password.chars().count();
        (
            now.full_name.trim().is_empty(),
            !looks_like_an_email(&now.email),
            (self.editing.is_none() || password > 0) && password < 8,
        )
    }

    fn render(&self, error: Option<SharedString>, cx: &mut Context<CloudTeam>) -> Div {
        let m = cx.ui().m;
        let editing = self.editing.is_some();
        let (name_missing, email_invalid, password_short) = self.problems(cx);
        let page = cx.entity();
        let pair = |a: FormField, b: FormField| {
            row().gap(m.space_3).items_start().child(div().flex_1().min_w_0().child(a)).child(div().flex_1().min_w_0().child(b))
        };
        let email = FormField::new("Email").required().error(email_invalid.then_some("Email invalide."));
        let password = FormField::new(if editing { "Nouveau mot de passe" } else { "Mot de passe" })
            .error(password_short.then_some("Au moins 8 caractères."))
            .hint(if editing { "Laisser vide pour conserver l'actuel." } else { "Au moins 8 caractères." });

        col()
            .gap(m.space_4)
            .children(error.map(|e| Alert::danger().child(e)))
            .child(
                FormField::new("Nom complet")
                    .required()
                    .error(name_missing.then_some("Le nom est obligatoire."))
                    .child(Input::new(&self.full_name).invalid(name_missing)),
            )
            .child(pair(
                if editing { email.hint("Sert à la connexion.") } else { email }.child(Input::new(&self.email).invalid(email_invalid)),
                FormField::new("Identifiant").hint("Optionnel — sert aussi à la connexion.").child(Input::new(&self.username)),
            ))
            .child(pair(
                if editing { password } else { password.required() }
                    .child(Input::new(&self.password).invalid(password_short).reveal_toggle()),
                FormField::new("Rôle")
                    .hint("Les administrateurs gèrent l'équipe et les paramètres.")
                    .child(Select::new(&self.role)),
            ))
            .when(editing, |this| {
                this.child(
                    Checkbox::new("employee-active", self.is_active)
                        .label("Compte actif")
                        .hint("Décochez pour bloquer la connexion sans supprimer le compte.")
                        .on_change(move |on, _, cx| {
                            page.update(cx, |page, cx| {
                                if let Some(form) = page.form.as_mut() {
                                    form.is_active = on;
                                }
                                page.form_changed(cx);
                            })
                        }),
                )
            })
    }
}

pub struct CloudTeam {
    /// Everyone but the signed-in administrator.
    team: Vec<Employee>,
    /// The members the filters let through: what the table shows.
    shown: Rc<Vec<Employee>>,
    loading: bool,
    load_error: Option<SharedString>,
    search: TextModel,
    role_filter: SelectModel,
    status_filter: SelectModel,

    panels: Entity<PanelStack>,
    form: Option<EmployeeForm>,
    saving: bool,
    save_error: Option<SharedString>,
    _subscriptions: Vec<Subscription>,
}

impl CloudTeam {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = TextModel::builder().placeholder("Rechercher un membre…").build(window, cx);
        let role_filter = SelectModel::new(
            vec![
                SelectOption::new(ALL, "Tous les rôles"),
                SelectOption::new("admin", "Administrateurs"),
                SelectOption::new("employee", "Employés"),
            ],
            Some(ALL),
            window,
            cx,
        );
        let status_filter = SelectModel::new(
            vec![
                SelectOption::new(ALL, "Tous les statuts"),
                SelectOption::new("active", "Actifs"),
                SelectOption::new("inactive", "Inactifs"),
            ],
            Some(ALL),
            window,
            cx,
        );
        let panels = cx.new(PanelStack::new);
        let subscriptions = vec![
            search.on_event(window, cx, |this, event, _, cx| {
                if event == TextEvent::Changed {
                    this.apply_filters(cx);
                }
            }),
            role_filter.on_change(window, cx, |this, _, _, cx| this.apply_filters(cx)),
            status_filter.on_change(window, cx, |this, _, _, cx| this.apply_filters(cx)),
            cx.observe(&panels, |this, panels, cx| {
                if !panels.read(cx).is_open() {
                    this.form = None;
                    this.save_error = None;
                }
                cx.notify();
            }),
        ];
        let mut page = Self {
            team: Vec::new(),
            shown: Rc::new(Vec::new()),
            loading: true,
            load_error: None,
            search,
            role_filter,
            status_filter,
            panels,
            form: None,
            saving: false,
            save_error: None,
            _subscriptions: subscriptions,
        };
        page.load(cx);
        page
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        self.load_error = None;
        cx.notify();
        cx.api(
            |client| async move { employees::list(&client).await },
            |this, result, cx| {
                this.loading = false;
                match result {
                    Ok(everyone) => {
                        let me = AuthStore::global(cx).read(cx).user.as_ref().map(|user| user.id.clone());
                        this.team = everyone.into_iter().filter(|e| Some(&e.id) != me.as_ref()).collect();
                    }
                    Err(error) => {
                        this.team = Vec::new();
                        this.load_error =
                            Some(humanize_error(&error, "Impossible de charger la liste des employés.").into());
                    }
                }
                this.apply_filters(cx);
            },
        );
    }

    fn filters(&self, cx: &App) -> (String, String, String) {
        (
            self.search.text(cx).trim().to_lowercase(),
            self.role_filter.value(cx).unwrap_or_else(|| ALL.into()),
            self.status_filter.value(cx).unwrap_or_else(|| ALL.into()),
        )
    }

    fn has_filters(&self, cx: &App) -> bool {
        let (query, role, status) = self.filters(cx);
        !query.is_empty() || role != ALL || status != ALL
    }

    fn apply_filters(&mut self, cx: &mut Context<Self>) {
        let (query, role, status) = self.filters(cx);
        self.shown = Rc::new(
            self.team
                .iter()
                .filter(|e| match role.as_str() {
                    "admin" => Role::of(e) == Role::Admin,
                    "employee" => Role::of(e) == Role::Employee,
                    _ => true,
                })
                .filter(|e| match status.as_str() {
                    "active" => is_active(e),
                    "inactive" => !is_active(e),
                    _ => true,
                })
                .filter(|e| {
                    query.is_empty()
                        || e.full_name.to_lowercase().contains(&query)
                        || e.email.to_lowercase().contains(&query)
                        || e.username.as_deref().unwrap_or_default().to_lowercase().contains(&query)
                })
                .cloned()
                .collect(),
        );
        cx.notify();
    }

    fn reset_filters(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search.clear(window, cx);
        self.role_filter.set_value(ALL, window, cx);
        self.status_filter.set_value(ALL, window, cx);
        self.apply_filters(cx);
    }

    // ---- the panel --------------------------------------------------------------------------

    fn update_panels(&self, cx: &mut Context<Self>, change: impl FnOnce(&mut PanelStack)) {
        self.panels.update(cx, |stack, cx| {
            change(stack);
            cx.notify();
        });
    }

    fn open_create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let form = EmployeeForm::new(None, window, cx);
        form.full_name.focus(window, cx);
        self.form = Some(form);
        self.save_error = None;
        let layer = PanelLayer::new(EMPLOYEE, NEW, "Nouvel employé")
            .subtitle("Créez un accès pour un membre de l'équipe.")
            .icon(IconName::UserPlus);
        self.update_panels(cx, |stack| {
            stack.close_all();
            stack.open(layer);
        });
    }

    fn open_edit(&mut self, employee: Employee, window: &mut Window, cx: &mut Context<Self>) {
        let layer = PanelLayer::new(EMPLOYEE, employee.id.clone(), employee.full_name.clone())
            .subtitle(employee.email.clone())
            .icon(IconName::CircleUser);
        self.form = Some(EmployeeForm::new(Some(employee), window, cx));
        self.save_error = None;
        self.update_panels(cx, |stack| {
            stack.close_all();
            stack.open(layer);
        });
    }

    fn form_changed(&mut self, cx: &mut Context<Self>) {
        let Some(form) = self.form.as_ref() else { return };
        let dirty = form.is_dirty(cx);
        if let Some(id) = self.panels.read(cx).top().map(|layer| layer.id.clone()) {
            self.update_panels(cx, |stack| stack.set_dirty(&id, dirty));
        }
        cx.notify();
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving {
            return;
        }
        let Some(form) = self.form.as_mut() else { return };
        form.touched = true;
        let (name_missing, email_invalid, password_short) = form.problems(cx);
        if name_missing || email_invalid || password_short {
            return cx.notify();
        }
        let now = form.snapshot(cx);
        let role = Role::from_value(&now.role);
        let username = Some(now.username.trim().to_string()).filter(|u| !u.is_empty());
        let editing = form.editing.as_ref().map(|e| e.id.clone());
        self.saving = true;
        self.save_error = None;
        cx.notify();
        cx.api_in(
            window,
            move |client| async move {
                match editing {
                    Some(id) => {
                        let patch = UpdateEmployee {
                            email: Some(now.email.trim().to_string()),
                            full_name: Some(now.full_name.trim().to_string()),
                            username,
                            role: Some(role),
                            is_active: Some(now.is_active),
                            password: Some(now.password).filter(|p| !p.is_empty()),
                        };
                        employees::update(&client, &id, &patch).await.map(|_| false)
                    }
                    None => {
                        let new = CreateEmployee {
                            email: now.email.trim().to_string(),
                            username,
                            password: now.password,
                            full_name: now.full_name.trim().to_string(),
                            role,
                        };
                        employees::create(&client, &new).await.map(|_| true)
                    }
                }
            },
            |this, result, window, cx| {
                this.saving = false;
                match result {
                    Ok(created) => {
                        toast::success(if created { "Employé créé." } else { "Employé enregistré." }, window, cx);
                        this.update_panels(cx, |stack| stack.close_all());
                        this.load(cx);
                    }
                    Err(error) => {
                        this.save_error = Some(humanize_error(&error, "Impossible d'enregistrer l'employé.").into());
                        cx.notify();
                    }
                }
            },
        );
    }

    /// Ask, then switch the account off (`delete == false`) or delete it for good.
    fn ask_remove(&mut self, employee: &Employee, delete: bool, window: &mut Window, cx: &mut Context<Self>) {
        let (page, id, name) = (cx.entity(), employee.id.clone(), employee.full_name.clone());
        let (title, message, label) = if delete {
            (
                "Supprimer cet employé ?",
                format!("{name} sera supprimé définitivement. Cette action est irréversible."),
                "Supprimer",
            )
        } else {
            (
                "Désactiver cet employé ?",
                format!("{name} ne pourra plus se connecter. Vous pourrez le réactiver plus tard."),
                "Désactiver",
            )
        };
        Confirm::new(title, message).confirm_label(label).danger().open(window, cx, move |window, cx| {
            let id = id.clone();
            page.update(cx, |_, cx| {
                cx.api_in(
                    window,
                    move |client| async move {
                        if delete { employees::delete(&client, &id).await } else { employees::deactivate(&client, &id).await }
                    },
                    move |this, result, window, cx| match result {
                        Ok(()) => {
                            toast::success(if delete { "Employé supprimé." } else { "Compte désactivé." }, window, cx);
                            this.update_panels(cx, |stack| stack.close_all());
                            this.load(cx);
                        }
                        Err(error) => toast::error(
                            humanize_error(
                                &error,
                                if delete { "La suppression a échoué." } else { "La désactivation a échoué." },
                            ),
                            window,
                            cx,
                        ),
                    },
                )
            })
        });
    }

    fn invite(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.api_in(
            window,
            |client| async move { employees::create_invite(&client).await },
            |_, result, window, cx| match result {
                Ok(invite) => {
                    cx.write_to_clipboard(ClipboardItem::new_string(invite.token));
                    toast::success("Invitation copiée. Valable 7 jours — à coller à l'inscription.", window, cx);
                }
                Err(error) => toast::error(humanize_error(&error, "Impossible de créer l'invitation."), window, cx),
            },
        );
    }

    // ---- drawing ----------------------------------------------------------------------------

    fn body(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.ui().clone();
        let (c, m) = (theme.colors, theme.m);
        if let Some(error) = self.load_error.clone() {
            return StateView::error()
                .title("Équipe indisponible")
                .body(error)
                .action("Réessayer", cx.listener(|this, _, _, cx| this.load(cx)))
                .into_any_element();
        }
        if self.loading && self.team.is_empty() {
            return StateView::loading().title("Chargement de l'équipe…").into_any_element();
        }
        if self.team.is_empty() {
            return StateView::empty()
                .icon(IconName::UsersRound)
                .title("Aucun employé pour le moment")
                .body("Créez un accès pour chaque membre de votre atelier : les administrateurs gèrent l'équipe et les paramètres, les employés ont un accès limité.")
                .action("Ajouter un employé", cx.listener(|this, _, window, cx| this.open_create(window, cx)))
                .into_any_element();
        }

        let total = self.team.len();
        let admins = self.team.iter().filter(|e| Role::of(e) == Role::Admin).count();
        let active = self.team.iter().filter(|e| is_active(e)).count();
        let kpis = row()
            .flex_none()
            .gap(m.space_3)
            .p(m.space_4)
            .child(KpiTile::new(if total > 1 { "Membres" } else { "Membre" }, total.to_string()).icon(IconName::UsersRound))
            .child(KpiTile::new("Administrateurs", admins.to_string()).icon(IconName::Shield).status(Status::Accent))
            .child(KpiTile::new("Actifs", active.to_string()).icon(IconName::CircleCheck).status(Status::Success))
            .child(KpiTile::new("Inactifs", (total - active).to_string()).icon(IconName::LogOut));

        let list: AnyElement = if self.shown.is_empty() {
            StateView::empty()
                .compact()
                .icon(IconName::Search)
                .title("Aucun résultat")
                .body("Aucun employé ne correspond à votre recherche.")
                .into_any_element()
        } else {
            let (rows, for_click, for_muted) = (self.shown.clone(), self.shown.clone(), self.shown.clone());
            let (page, page_click) = (cx.entity(), cx.entity());
            let open = self.panels.read(cx).key_of(EMPLOYEE);
            let selected = open.and_then(|id| rows.iter().position(|e| e.id.as_str() == id.as_ref()));
            DataTable::new(
                "team-table",
                vec![
                    Column::new("Employé").flex(2.),
                    Column::new("Contact").flex(2.),
                    Column::new("Rôle").fixed(px(190.)),
                    Column::new("").fixed(px(120.)).right(),
                ],
                rows.len(),
                move |ix, column, _, _| {
                    let employee = &rows[ix];
                    match column {
                        0 => row()
                            .gap(m.space_3)
                            .min_w_0()
                            .child(
                                row()
                                    .flex_none()
                                    .justify_center()
                                    .size(px(26.))
                                    .bg(c.accent_soft)
                                    .text_color(c.accent_active)
                                    .text_size(m.fs_micro)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(initials(&employee.full_name)),
                            )
                            .child(div().one_line().font_weight(FontWeight::MEDIUM).child(employee.full_name.clone()))
                            .into_any_element(),
                        1 => col()
                            .min_w_0()
                            .gap(px(2.))
                            .text_color(c.text_muted)
                            .child(
                                row().gap(m.space_1).child(Icon::new(IconName::Mail).sm()).child(
                                    div().one_line().child(employee.email.clone()),
                                ),
                            )
                            .when_some(employee.username.clone().filter(|u| !u.is_empty()), |this, username| {
                                this.child(
                                    row().gap(m.space_1).child(Icon::new(IconName::CircleUser).sm()).child(
                                        div().one_line().child(username),
                                    ),
                                )
                            })
                            .into_any_element(),
                        2 => row()
                            .gap(m.space_1)
                            .child(if Role::of(employee) == Role::Admin {
                                Tag::new("Admin").status(Status::Accent).icon(IconName::Shield)
                            } else {
                                Tag::new("Employé").status(Status::Muted)
                            })
                            .when(!is_active(employee), |this| this.child(Tag::new("Inactif").status(Status::Muted)))
                            .into_any_element(),
                        _ => {
                            let (edit, off, delete) = (page.clone(), page.clone(), page.clone());
                            let (a, b, d) = (employee.clone(), employee.clone(), employee.clone());
                            // These clicks are for the buttons, not for the row under them.
                            row()
                                .gap(m.space_1)
                                .child(IconButton::new(("team-edit", ix), IconName::Pencil).title("Modifier").on_click(
                                    move |_, window, cx| {
                                        cx.stop_propagation();
                                        let employee = a.clone();
                                        edit.update(cx, |this, cx| this.open_edit(employee, window, cx))
                                    },
                                ))
                                .when(is_active(employee), |this| {
                                    this.child(
                                        IconButton::new(("team-off", ix), IconName::LogOut).title("Désactiver").danger().on_click(
                                            move |_, window, cx| {
                                                cx.stop_propagation();
                                                off.update(cx, |this, cx| this.ask_remove(&b, false, window, cx))
                                            },
                                        ),
                                    )
                                })
                                .child(
                                    IconButton::new(("team-delete", ix), IconName::Trash)
                                        .title("Supprimer définitivement")
                                        .danger()
                                        .on_click(move |_, window, cx| {
                                            cx.stop_propagation();
                                            delete.update(cx, |this, cx| this.ask_remove(&d, true, window, cx))
                                        }),
                                )
                                .into_any_element()
                        }
                    }
                },
            )
            .row_height(px(52.))
            .selected(selected)
            .muted_rows(move |row| for_muted.get(row).is_some_and(|e| !is_active(e)))
            .on_row_click(move |row, window, cx| {
                let Some(employee) = for_click.get(*row).cloned() else { return };
                page_click.update(cx, |this, cx| this.open_edit(employee, window, cx));
            })
            .into_any_element()
        };

        col()
            .size_full()
            .min_h_0()
            .child(kpis)
            .child(div().flex_1().min_h_0().bg(c.bg_surface).hairline_top(c.border).child(list))
            .into_any_element()
    }

    fn panel(&self, layer: &PanelLayer, cx: &mut Context<Self>) -> Panel {
        let mut panel = Panel::new(&self.panels, layer.id.clone());
        let Some(form) = self.form.as_ref() else { return panel };
        let stack = self.panels.clone();
        let creating = form.editing.is_none();

        if let Some(employee) = form.editing.as_ref() {
            if !is_active(employee) {
                panel = panel.badge("Inactif", Status::Muted);
            }
            let (to_switch_off, to_delete) = (employee.clone(), employee.clone());
            if is_active(employee) {
                panel = panel.action(
                    IconButton::new("employee-off", IconName::LogOut)
                        .title("Désactiver le compte")
                        .size(IconButtonSize::Md)
                        .danger()
                        .on_click(cx.listener(move |this, _, window, cx| this.ask_remove(&to_switch_off, false, window, cx))),
                );
            }
            panel = panel.action(
                IconButton::new("employee-delete", IconName::Trash)
                    .title("Supprimer définitivement")
                    .size(IconButtonSize::Md)
                    .danger()
                    .on_click(cx.listener(move |this, _, window, cx| this.ask_remove(&to_delete, true, window, cx))),
            );
        }

        let panel = panel.child(
            PanelSection::new("Compte").icon(IconName::CircleUser).child(form.render(self.save_error.clone(), cx)),
        );
        let panel = match form.editing.as_ref() {
            Some(employee) => panel.child(
                PanelSection::new("Informations").icon(IconName::Info).child(
                    InfoList::new(vec![
                        InfoItem::new("Créé le", Some(format::date(&employee.created_at))),
                        InfoItem::new("Statut", Some(if is_active(employee) { "Actif" } else { "Inactif" })),
                    ])
                    .dense(),
                ),
            ),
            None => panel,
        };
        panel
            .footer(Button::new("employee-cancel").ghost().label("Annuler").disabled(self.saving).on_click(
                move |_, window, cx| request_close(&stack, window, cx),
            ))
            .footer(
                Button::new("employee-save")
                    .primary()
                    .loading(self.saving)
                    .label(if creating { "Créer l'employé" } else { "Enregistrer" })
                    .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))),
            )
    }
}

impl Render for CloudTeam {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = cx.ui().colors;
        let total = self.team.len();
        let has_filters = self.has_filters(cx);
        let count = if self.loading && self.team.is_empty() {
            "Chargement…".to_string()
        } else if has_filters {
            format!("{} sur {total}", self.shown.len())
        } else {
            format!("{total} membre{}", if total > 1 { "s" } else { "" })
        };
        let layers: Vec<PanelLayer> = self.panels.read(cx).layers().to_vec();
        let panels: Vec<Panel> = layers.iter().map(|layer| self.panel(layer, cx)).collect();

        let mut toolbar = PageToolbar::new("Employés")
            .icon(IconName::UsersRound)
            .subtitle("Gérez les accès de votre équipe et leurs rôles.")
            .meta(count)
            .trailing(div().w(px(150.)).child(Select::new(&self.role_filter)))
            .trailing(div().w(px(150.)).child(Select::new(&self.status_filter)));
        if has_filters {
            toolbar = toolbar.trailing(
                IconButton::new("team-reset", IconName::X)
                    .title("Réinitialiser les filtres")
                    .size(IconButtonSize::Md)
                    .on_click(cx.listener(|this, _, window, cx| this.reset_filters(window, cx))),
            );
        }
        let toolbar = toolbar
            .trailing(SearchField::new(&self.search).width(px(200.)))
            .trailing(
                IconButton::new("team-refresh", IconName::RefreshCw)
                    .title("Actualiser")
                    .size(IconButtonSize::Md)
                    .disabled(self.loading)
                    .on_click(cx.listener(|this, _, _, cx| this.load(cx))),
            )
            .trailing(Button::new("team-invite").label("Inviter").on_click(cx.listener(|this, _, window, cx| this.invite(window, cx))))
            .trailing(Button::new("team-new").primary().icon(IconName::UserPlus).label("Nouvel employé").on_click(
                cx.listener(|this, _, window, cx| this.open_create(window, cx)),
            ));

        col()
            .relative()
            .size_full()
            .bg(c.bg_app)
            .on_key_down(PanelHost::key_handler(&self.panels))
            .child(toolbar)
            .child(div().flex_1().min_h_0().child(self.body(cx)))
            .child(PanelHost::new(&self.panels).children(panels))
    }
}

#[cfg(test)]
mod tests {
    // Not `super::*`: it would bring in GPUI's own `test` attribute in place of Rust's.
    use super::looks_like_an_email;

    #[test]
    fn an_email_needs_a_name_an_at_sign_and_a_dotted_domain() {
        assert!(looks_like_an_email("jean@atelier.fr"));
        assert!(looks_like_an_email("  léa.martin@garage-dupont.example.com "));
        for wrong in ["", "jean", "jean@", "@atelier.fr", "jean@atelier", "jean dupont@atelier.fr", "a@b@c.fr", "jean@.fr"] {
            assert!(!looks_like_an_email(wrong), "{wrong}");
        }
    }
}
