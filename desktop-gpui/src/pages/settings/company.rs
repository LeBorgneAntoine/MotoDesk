//! The company's identity: its logo and its names.

use std::sync::Arc;

use motodesk_client::company::{self, Company};
use motodesk_ui::gpui::prelude::FluentBuilder as _;
use motodesk_ui::gpui::*;
use motodesk_ui::*;

use crate::auth::AuthStore;
use crate::backend::{ApiExt as _, PickExt as _};
use crate::format::humanize_error;
use crate::notices::NoticesState;

pub struct CompanyView {
    company: Option<Company>,
    logo: Option<Arc<Image>>,
    loading: bool,
    name: TextModel,
    legal_name: TextModel,
    saving: bool,
    uploading: bool,
    error: Option<String>,
    done: Option<&'static str>,
    _subscriptions: Vec<Subscription>,
}

impl CompanyView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let name = TextModel::builder().placeholder("Atelier Dupont").build(window, cx);
        let legal_name = TextModel::builder().placeholder("Atelier Dupont SARL").build(window, cx);
        let subscriptions = [&name, &legal_name]
            .into_iter()
            .map(|field| {
                field.on_event(window, cx, |this: &mut Self, event, window, cx| match event {
                    TextEvent::Submitted { .. } => this.save(window, cx),
                    // The save button follows what is typed.
                    _ => cx.notify(),
                })
            })
            .collect();
        let mut view = Self {
            company: None,
            logo: None,
            loading: true,
            name,
            legal_name,
            saving: false,
            uploading: false,
            error: None,
            done: None,
            _subscriptions: subscriptions,
        };
        view.load(window, cx);
        view
    }

    fn load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.api_in(
            window,
            |client| async move { (company::get(&client).await, company::logo(&client).await) },
            |this, (company, logo), window, cx| {
                this.loading = false;
                match company {
                    Ok(company) => this.show(company, window, cx),
                    Err(error) => this.error = Some(humanize_error(&error, "Impossible de lire la fiche de l'entreprise.")),
                }
                this.logo = logo.ok().flatten().and_then(picture_from_bytes);
                cx.notify();
            },
        );
    }

    fn show(&mut self, company: Company, window: &mut Window, cx: &mut Context<Self>) {
        self.name.set_text(company.name().to_string(), window, cx);
        self.legal_name.set_text(company.legal_name().to_string(), window, cx);
        self.company = Some(company);
    }

    /// Whether the names on screen differ from the ones stored.
    fn dirty(&self, cx: &App) -> bool {
        self.company.as_ref().is_some_and(|company| {
            self.name.text(cx).trim() != company.name() || self.legal_name.text(cx).trim() != company.legal_name()
        })
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving || !self.dirty(cx) || !AuthStore::global(cx).read(cx).is_admin() {
            return;
        }
        let (name, legal_name) = (self.name.text(cx).trim().to_string(), self.legal_name.text(cx).trim().to_string());
        self.done = None;
        if name.is_empty() {
            self.error = Some("Le nom de l'entreprise est obligatoire.".into());
            cx.notify();
            return;
        }
        self.error = None;
        self.saving = true;
        cx.notify();
        cx.api_in(
            window,
            move |client| async move { company::update(&client, &name, &legal_name).await },
            |this, result, window, cx| {
                this.saving = false;
                match result {
                    Ok(company) => {
                        this.show(company, window, cx);
                        this.done = Some("Informations enregistrées.");
                        // The company's name is one of the mentions the banner asks for.
                        NoticesState::global(cx).update(cx, |notices, cx| notices.recheck(cx));
                    }
                    Err(error) => this.error = Some(humanize_error(&error, "Échec de l'enregistrement.")),
                }
                cx.notify();
            },
        );
    }

    fn pick_logo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.uploading {
            return;
        }
        cx.pick_file(window, "Choisir un logo", company::MAX_LOGO_BYTES, |this, picked, window, cx| {
            let file = match picked {
                Ok(Some(file)) => file,
                // The dialog was closed.
                Ok(None) => return,
                Err(_) => {
                    this.error = Some("Impossible de lire ce fichier.".into());
                    cx.notify();
                    return;
                }
            };
            this.done = None;
            this.error = company::logo_problem(&file.name, file.size).map(str::to_string);
            let Some(content_type) = company::image_type(&file.name).filter(|_| this.error.is_none()) else {
                cx.notify();
                return;
            };
            this.uploading = true;
            cx.notify();
            let bytes = file.bytes.clone();
            cx.api_in(
                window,
                move |client| async move { company::upload_logo(&client, content_type, bytes).await },
                move |this, result, _, cx| {
                    this.uploading = false;
                    match result {
                        Ok(company) => {
                            this.company = Some(company);
                            this.logo = picture_from_bytes(file.bytes);
                            this.done = Some("Logo mis à jour.");
                        }
                        Err(error) => this.error = Some(humanize_error(&error, "Échec de l'envoi du logo.")),
                    }
                    cx.notify();
                },
            );
        });
    }
}

impl Render for CompanyView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.ui().clone();
        let (c, m) = (theme.colors, theme.m);
        if self.loading {
            return div().h(px(160.)).child(StateView::loading().compact().title("Chargement de la fiche entreprise…"));
        }
        let is_admin = AuthStore::global(cx).read(cx).is_admin();
        let has_logo = self.logo.is_some();

        let logo = SectionCard::new().title("Logo").icon(IconName::Image).child(
            row()
                .items_start()
                .gap(m.space_4)
                .child(Picture::new(self.logo.clone()))
                .child(
                    col()
                        .min_w_0()
                        .gap(m.space_3)
                        .child(div().fs(m.fs_sm).text_color(c.text_muted).child(
                            "PNG, JPG ou SVG. Format carré recommandé, 1 Mo maximum. Le logo est gardé sur ce poste pour rester visible hors ligne.",
                        ))
                        .when(is_admin, |this| {
                            this.child(
                                row().child(
                                    Button::new("company-logo")
                                        .icon(IconName::Upload)
                                        .label(if has_logo { "Remplacer le logo" } else { "Téléverser un logo" })
                                        .loading(self.uploading)
                                        .on_click(cx.listener(|this, _, window, cx| this.pick_logo(window, cx))),
                                ),
                            )
                        }),
                ),
        );

        let identity = SectionCard::new()
            .title("Identité")
            .icon(IconName::Building2)
            .child(
                row()
                    .items_start()
                    .gap(m.space_5)
                    .child(div().flex_1().min_w_0().child(
                        FormField::new("Nom de l'entreprise").required().child(Input::new(&self.name).disabled(!is_admin)),
                    ))
                    .child(div().flex_1().min_w_0().child(
                        FormField::new("Raison sociale")
                            .hint("Nom juridique complet (optionnel).")
                            .child(Input::new(&self.legal_name).disabled(!is_admin)),
                    )),
            );
        let identity = if is_admin {
            identity.footer(
                Button::new("company-save")
                    .primary()
                    .label("Enregistrer")
                    .loading(self.saving)
                    .disabled(!self.dirty(cx))
                    .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
            )
        } else {
            identity
        };

        col()
            .gap(m.space_4)
            .children(self.error.clone().map(|error| Alert::danger().child(error)))
            .children(self.done.map(|done| Alert::success().child(done)))
            .when(!is_admin, |this| {
                this.child(Alert::info().child("Seuls les administrateurs peuvent modifier l'identité de l'entreprise."))
            })
            .child(logo)
            .child(identity)
    }
}
