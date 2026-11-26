//! Where the sync of this PC stands, the other PCs it sees, and the step that puts a local
//! network online (port of `routes/SyncSettings.svelte` and `routes/GoOnline.svelte`).
//! Administrators only.

use std::time::Duration;

use motodesk_client::local_account::{self, PromotePayload};
use motodesk_client::sync::{self, Diagnostics};
use motodesk_client::{ApiError, auth};
use motodesk_ui::gpui::prelude::FluentBuilder as _;
use motodesk_ui::gpui::*;
use motodesk_ui::*;

use super::background::BackgroundCard;
use crate::auth::{AuthStore, flows};
use crate::backend::ApiExt as _;
use crate::format::{self, humanize_error};

/// How often the figures are read again while the view is open.
const REFRESH: Duration = Duration::from_secs(5);

/// A moment for a tile's hint: its date, or a dash while it never happened.
fn moment(at: &Option<String>) -> String {
    at.as_deref().map(format::date_time).unwrap_or_else(|| "—".into())
}

/// How the cloud tile is coloured: unreachable, behind, or up to date.
fn cloud_status(diagnostics: &Diagnostics) -> Status {
    if !diagnostics.cloud_reachable {
        Status::Danger
    } else if diagnostics.pending_cloud_ops > 0 {
        Status::Warning
    } else {
        Status::Success
    }
}

/// Why the form that puts the installation online cannot be sent, or `None` when it can.
fn go_online_problem(email: &str, password: &str, confirm: &str) -> Option<String> {
    if !email.trim().contains('@') {
        return Some("Saisissez une adresse e-mail valide.".into());
    }
    // The cloud's rules for a password are those of a local account.
    local_account::validate_new_local_account("xxx", password, confirm)
}

/// The form of "Passer en ligne".
struct GoOnline {
    email: TextModel,
    password: TextModel,
    confirm: TextModel,
    company_name: TextModel,
    understood: bool,
    submitting: bool,
    error: Option<String>,
}

pub struct SyncView {
    diagnostics: Option<Diagnostics>,
    loading: bool,
    error: Option<String>,
    static_peers: TextModel,
    /// The list was edited: a refresh no longer overwrites it.
    static_dirty: bool,
    saving_peers: bool,
    syncing: bool,
    go_online: GoOnline,
    /// Staying in the network with the window closed: a card with a life of its own.
    background: Option<Entity<BackgroundCard>>,
    _subscriptions: Vec<Subscription>,
}

impl SyncView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let static_peers = TextModel::builder().placeholder("192.168.1.42:17300").multiline(4).build(window, cx);
        let password = |window: &mut Window, cx: &mut Context<Self>| TextModel::builder().password().build(window, cx);
        let go_online = GoOnline {
            email: TextModel::builder().placeholder("vous@atelier.fr").build(window, cx),
            password: password(window, cx),
            confirm: password(window, cx),
            company_name: TextModel::new(window, cx),
            understood: false,
            submitting: false,
            error: None,
        };
        let subscriptions = vec![static_peers.on_event(window, cx, |this: &mut Self, event, _, cx| {
            if matches!(event, TextEvent::Changed) {
                this.static_dirty = true;
                cx.notify();
            }
        })];
        // Read now, then every few seconds for as long as the view is open.
        cx.spawn_in(window, async move |this, cx| {
            loop {
                if this.update_in(cx, |this, window, cx| this.refresh(window, cx)).is_err() {
                    break;
                }
                cx.background_executor().timer(REFRESH).await;
            }
        })
        .detach();
        Self {
            diagnostics: None,
            loading: true,
            error: None,
            static_peers,
            static_dirty: false,
            saving_peers: false,
            syncing: false,
            go_online,
            background: crate::background::SUPPORTED.then(|| cx.new(BackgroundCard::new)),
            _subscriptions: subscriptions,
        }
    }

    fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.api_in(
            window,
            |client| async move { (sync::diagnostics(&client).await, sync::static_peers(&client).await.unwrap_or_default()) },
            |this, (diagnostics, peers), window, cx| {
                this.loading = false;
                match diagnostics {
                    Ok(diagnostics) => {
                        this.diagnostics = Some(diagnostics);
                        this.error = None;
                        let listed = peers.join("\n");
                        if !this.static_dirty && this.static_peers.text(cx) != listed {
                            this.static_peers.set_text(listed, window, cx);
                            // Filling the field is not an edit.
                            this.static_dirty = false;
                        }
                    }
                    // Figures already on screen stay there: one missed reading is not news.
                    Err(error) if this.diagnostics.is_none() => {
                        this.error = Some(humanize_error(&error, "Diagnostics indisponibles."));
                    }
                    Err(_) => {}
                }
                cx.notify();
            },
        );
    }

    fn save_static_peers(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving_peers {
            return;
        }
        let peers = sync::parse_static_peers(&self.static_peers.text(cx));
        self.saving_peers = true;
        cx.notify();
        cx.api_in(
            window,
            move |client| async move { sync::set_static_peers(&client, &peers).await },
            |this, result, window, cx| {
                this.saving_peers = false;
                match result {
                    Ok(peers) => {
                        this.static_peers.set_text(peers.join("\n"), window, cx);
                        this.static_dirty = false;
                        toast::success("Liste des adresses enregistrée.", window, cx);
                    }
                    Err(error) => toast::error(humanize_error(&error, "Échec de l'enregistrement."), window, cx),
                }
                cx.notify();
            },
        );
    }

    /// Sync now. With `import`, everything is fetched from the cloud again: slow, and only
    /// worth it when the local database is empty or damaged.
    fn sync_now(&mut self, import: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.syncing {
            return;
        }
        self.syncing = true;
        cx.notify();
        cx.api_in(
            window,
            move |client| async move {
                // The service syncs with the cloud session it was last given: give it this one
                // (a local network's session is the service's own, and is left alone).
                if let Some(session) = client.tokens() {
                    auth::sync_session_to_local(&client, &session).await;
                }
                if import {
                    sync::force_import_from_cloud(&client).await.map(|result| result.message())
                } else {
                    sync::sync_now(&client).await.map(|result| {
                        if result.cloud_sync_ok { "Synchronisé avec le cloud".to_string() } else { "Local OK · cloud inaccessible".to_string() }
                    })
                }
            },
            |this, result: Result<String, ApiError>, window, cx| {
                this.syncing = false;
                match result {
                    Ok(message) => toast::success(message, window, cx),
                    Err(error) => toast::error(humanize_error(&error, "Échec de la synchronisation."), window, cx),
                }
                this.refresh(window, cx);
                cx.notify();
            },
        );
    }

    fn retry_refused(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.api_in(
            window,
            |client| async move {
                sync::retry_refused(&client).await?;
                sync::sync_now(&client).await
            },
            |this, result, window, cx| {
                if let Err(error) = result {
                    toast::error(humanize_error(&error, "Impossible de relancer l'envoi."), window, cx);
                }
                this.refresh(window, cx);
            },
        );
    }

    fn submit_go_online(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let form = &mut self.go_online;
        if form.submitting || !form.understood {
            return;
        }
        let (email, password, confirm) = (form.email.text(cx).trim().to_string(), form.password.text(cx), form.confirm.text(cx));
        form.error = go_online_problem(&email, &password, &confirm);
        if form.error.is_some() {
            cx.notify();
            return;
        }
        let payload = PromotePayload { email, password, confirm, full_name: None, company_name: form.company_name.trimmed(cx) };
        form.submitting = true;
        cx.notify();
        cx.api_in(
            window,
            move |client| async move { flows::go_online(client, payload).await },
            |this, result, window, cx| {
                this.go_online.submitting = false;
                match result {
                    Ok(session) => {
                        toast::success("Installation en ligne : vos données sont envoyées au cloud.", window, cx);
                        // From here on the session is the cloud's.
                        AuthStore::global(cx).update(cx, |store, cx| store.adopt(session, cx));
                        this.refresh(window, cx);
                    }
                    Err(error) => this.go_online.error = Some(humanize_error(&error, "Erreur inattendue, veuillez réessayer.")),
                }
                cx.notify();
            },
        );
    }

    // ---- drawing -------------------------------------------------------------------------------

    fn text(text: impl Into<SharedString>, cx: &App) -> Div {
        let theme = cx.ui();
        div().fs(theme.m.fs_sm).text_color(theme.colors.text_muted).child(text.into())
    }

    fn go_online_card(&self, cloud_url: &str, cx: &mut Context<Self>) -> SectionCard {
        let m = cx.ui().m;
        let form = &self.go_online;
        let view = cx.entity();
        let field = |label: &'static str, model: &TextModel| div().max_w(px(420.)).child(FormField::new(label).required().child(Input::new(model)));
        SectionCard::new()
            .title("Passer en ligne")
            .subtitle("Le réseau local de l'atelier fonctionne sans cloud.")
            .icon(IconName::Upload)
            .child(
                col()
                    .gap(m.space_3)
                    .child(Self::text(
                        "L'entreprise est créée sur le cloud avec ses données actuelles (clients, stock, ventes, journal fiscal), puis envoyée par la synchronisation. Vous vous connecterez ensuite avec l'adresse e-mail et le mot de passe ci-dessous, depuis cet ordinateur ou un autre.",
                        cx,
                    ))
                    .when(!cloud_url.is_empty(), |this| this.child(Self::text(format!("Serveur cloud : {cloud_url}"), cx)))
                    .children(form.error.clone().map(|error| Alert::danger().child(error)))
                    .child(field("Adresse e-mail du compte en ligne", &form.email))
                    .child(div().max_w(px(420.)).child(
                        FormField::new("Mot de passe du compte en ligne")
                            .required()
                            .hint("Minimum 8 caractères")
                            .child(Input::new(&form.password).reveal_toggle()),
                    ))
                    .child(field("Confirmer le mot de passe", &form.confirm))
                    .child(div().max_w(px(420.)).child(
                        FormField::new("Nom de l'entreprise sur le cloud")
                            .hint("Laissez vide pour reprendre le nom saisi dans Paramètres › Entreprise.")
                            .child(Input::new(&form.company_name)),
                    ))
                    .child(
                        Checkbox::new("go-understood", form.understood)
                            .label("Je comprends que cette étape est définitive : l'installation ne revient pas en mode local.")
                            .on_change(move |on, _, cx| {
                                view.update(cx, |this, cx| {
                                    this.go_online.understood = on;
                                    cx.notify();
                                })
                            }),
                    )
                    .child(
                        row().child(
                            Button::new("go-online")
                                .primary()
                                .label("Passer en ligne")
                                .loading(form.submitting)
                                .disabled(!form.understood)
                                .on_click(cx.listener(|this, _, window, cx| this.submit_go_online(window, cx))),
                        ),
                    ),
            )
    }

    fn state_card(&self, diagnostics: &Diagnostics, cx: &mut Context<Self>) -> SectionCard {
        let m = cx.ui().m;
        let refused = (diagnostics.refused_ops > 0).then(|| {
            let reason = diagnostics.refused_latest.as_deref().map(|why| format!(" (dernier motif : {why})")).unwrap_or_default();
            Alert::warning().title("Modifications refusées par le cloud").child(
                col()
                    .gap(m.space_2)
                    .child(format!(
                        "{} modification(s) n'ont pas été acceptées par le cloud{reason}. Elles attendent puis sont renvoyées d'elles-mêmes ; les autres modifications partent normalement.",
                        diagnostics.refused_ops
                    ))
                    .child(row().child(Button::new("sync-retry").link().small().label("Réessayer maintenant").on_click(cx.listener(
                        |this, _, window, cx| this.retry_refused(window, cx),
                    )))),
            )
        });
        let misconfigured = diagnostics.cloud_url_misconfigured.then(|| {
            Alert::danger().title("Cloud mal configuré").child(format!(
                "Cette version cherche le cloud sur cet ordinateur ({}) : rien ne sera synchronisé. Elle doit être refabriquée avec l'adresse du serveur cloud (variable MOTODESK_CLOUD_URL).",
                diagnostics.cloud_url
            ))
        });
        let push_error = diagnostics.last_cloud_push_error.clone().map(|error| {
            let again = diagnostics
                .cloud_backoff_until
                .as_deref()
                .map(|until| format!(" Nouvelle tentative après {}.", format::date_time(until)))
                .unwrap_or_default();
            Alert::warning().title("Dernière erreur de poussée").child(format!("{error}{again}"))
        });
        let cloud_address = if diagnostics.cloud_url.is_empty() { "—".to_string() } else { diagnostics.cloud_url.clone() };
        let tiles = vec![
            KpiTile::new("Cloud", if diagnostics.cloud_reachable { "En ligne" } else { "Hors ligne" })
                .hint(cloud_address)
                .status(cloud_status(diagnostics))
                .icon(IconName::Database),
            KpiTile::new("Opérations en attente", format::integer(diagnostics.pending_cloud_ops))
                .hint(format!("Dernière poussée : {}", moment(&diagnostics.last_cloud_push_at)))
                .status(if diagnostics.pending_cloud_ops > 0 { Status::Warning } else { Status::Neutral })
                .icon(IconName::Upload),
            KpiTile::new("Postes détectés", format::integer(diagnostics.lan_peers.len() as i64))
                .hint(format!("Dernière diffusion : {}", moment(&diagnostics.last_lan_push_at)))
                .icon(IconName::Layers),
            KpiTile::new("Identifiant de ce poste", diagnostics.short_device_id()).hint(diagnostics.device_id.clone()).icon(IconName::IdCard),
        ];
        SectionCard::new()
            .title("État de synchronisation")
            .icon(IconName::RefreshCw)
            .action(
                Button::new("sync-refresh")
                    .ghost()
                    .small()
                    .icon(IconName::RefreshCw)
                    .label("Actualiser")
                    .disabled(self.syncing)
                    .on_click(cx.listener(|this, _, window, cx| this.refresh(window, cx))),
            )
            .action(
                Button::new("sync-now")
                    .primary()
                    .small()
                    .label("Synchroniser")
                    .tooltip("Maj + clic : tout réimporter depuis le cloud")
                    .loading(self.syncing)
                    .on_click(cx.listener(|this, _, window, cx| {
                        let import = window.modifiers().shift;
                        this.sync_now(import, window, cx);
                    })),
            )
            .child(
                col()
                    .gap(m.space_3)
                    .children(refused)
                    .children(misconfigured)
                    .child(row().items_stretch().gap(m.space_3).children(tiles))
                    .children(push_error),
            )
    }

    fn peers_card(diagnostics: &Diagnostics) -> SectionCard {
        let card = SectionCard::new().title("Postes détectés sur le réseau").icon(IconName::UsersRound).flush();
        if diagnostics.lan_peers.is_empty() {
            return card.child(div().h(px(140.)).child(StateView::empty().compact().icon(IconName::UsersRound).title("Aucun poste détecté")));
        }
        let peers = diagnostics.lan_peers.clone();
        card.child(
            DataTable::new(
                "sync-peers",
                vec![Column::new("Appareil").flex(2.).mono(), Column::new("Adresse").flex(3.), Column::new("Port").fixed(px(90.)).right()],
                peers.len(),
                move |ix, column, _, _| {
                    let Some(peer) = peers.get(ix) else { return div().into_any_element() };
                    match column {
                        0 => sync::short_id(&peer.device_id).into_any_element(),
                        1 => peer.host.clone().into_any_element(),
                        _ => peer.port.to_string().into_any_element(),
                    }
                },
            )
            .compact()
            .auto_height(),
        )
    }
}

impl Render for SyncView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.ui().clone();
        let (c, m) = (theme.colors, theme.m);
        let is_local = AuthStore::global(cx).read(cx).is_local();
        let cloud_url = self.diagnostics.as_ref().map(|d| d.cloud_url.clone()).unwrap_or_default();
        let page = col().gap(m.space_4).when(is_local, |this| this.child(self.go_online_card(&cloud_url, cx)));

        if self.loading {
            return page.child(div().h(px(160.)).child(StateView::loading().compact().title("Lecture des diagnostics…")));
        }
        let Some(diagnostics) = self.diagnostics.clone() else {
            return page.child(
                div().h(px(220.)).child(
                    StateView::error()
                        .compact()
                        .title("Diagnostics indisponibles")
                        .body(self.error.clone().unwrap_or_default())
                        .action("Réessayer", cx.listener(|this, _, window, cx| this.refresh(window, cx))),
                ),
            );
        };

        let mesh = SectionCard::new().title("Synchronisation locale automatique").icon(IconName::Layers).child(
            col()
                .gap(m.space_3)
                .child(Self::text(
                    "Plus besoin de code de jumelage : les appareils connectés au même compte d'entreprise se découvrent et se synchronisent automatiquement sur le réseau local.",
                    cx,
                ))
                .child(
                    row()
                        .gap(m.space_3)
                        .child(if diagnostics.mesh_secret_set {
                            Tag::new("Réseau activé").status(Status::Success).icon(IconName::CircleCheck).medium()
                        } else {
                            Tag::new("En attente").status(Status::Muted).icon(IconName::CircleAlert).medium()
                        })
                        .child(div().min_w_0().fs(m.fs_sm).text_color(c.text).child(if diagnostics.mesh_secret_set {
                            "Découverte automatique des appareils de votre société."
                        } else {
                            "En attente de la synchronisation initiale avec le cloud pour activer le réseau."
                        })),
                ),
        );

        let static_peers = SectionCard::new()
            .title("Adresses saisies à la main")
            .subtitle("Quand le réseau empêche les postes de se trouver seuls")
            .icon(IconName::List)
            .footer(
                Button::new("sync-peers-save")
                    .primary()
                    .label("Enregistrer la liste")
                    .loading(self.saving_peers)
                    .disabled(!self.static_dirty)
                    .on_click(cx.listener(|this, _, window, cx| this.save_static_peers(window, cx))),
            )
            .child(
                col()
                    .gap(m.space_2)
                    .child(Self::text("Une par ligne, au format hôte:port.", cx))
                    .child(Textarea::new(&self.static_peers).mono()),
            );

        page.child(self.state_card(&diagnostics, cx))
            .child(mesh)
            .child(Self::peers_card(&diagnostics))
            .child(static_peers)
            .when_some(self.background.clone(), |this, card| this.child(card))
    }
}

#[cfg(test)]
mod tests {
    // Not `super::*`: it would bring in GPUI's own `test` attribute in place of Rust's.
    use motodesk_client::sync::Diagnostics;
    use motodesk_ui::Status;

    use super::{cloud_status, go_online_problem, moment};

    #[test]
    fn the_cloud_tile_is_red_out_of_reach_amber_behind_and_green_up_to_date() {
        let mut diagnostics = Diagnostics::default();
        assert_eq!(cloud_status(&diagnostics), Status::Danger);
        diagnostics.cloud_reachable = true;
        assert_eq!(cloud_status(&diagnostics), Status::Success);
        diagnostics.pending_cloud_ops = 3;
        assert_eq!(cloud_status(&diagnostics), Status::Warning);
    }

    #[test]
    fn going_online_needs_an_address_and_a_password_typed_twice() {
        assert_eq!(go_online_problem("camille", "mot de passe", "mot de passe").as_deref(), Some("Saisissez une adresse e-mail valide."));
        assert!(go_online_problem("camille@atelier.fr", "court", "court").is_some());
        assert!(go_online_problem("camille@atelier.fr", "mot de passe", "autre chose").is_some());
        assert_eq!(go_online_problem(" camille@atelier.fr ", "mot de passe", "mot de passe"), None);
    }

    #[test]
    fn a_moment_that_never_came_is_a_dash() {
        assert_eq!(moment(&None), "—");
        assert_ne!(moment(&Some("2026-11-20T08:30:00Z".into())), "—");
    }
}
