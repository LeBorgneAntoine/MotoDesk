//! The Assistant (port of `desktop/src/routes/assistant/`): the conversations of this PC on the
//! left, the exchange in the middle, and on the right the canvas where the answer is laid out.
//!
//! With an OpenRouter key on this PC, a message goes to the model and its answer is written as
//! it arrives. Without one, the screen is a demonstration: the suggestions show what the canvas
//! can hold, with made-up data, and nothing is asked of anyone.

mod thread;

use chrono::Utc;
use motodesk_client::assistant::{self, Artifact, ArtifactBody, ChatEvent, ChatRequest, Conversation, ConversationSummary, Permission, Role};
use motodesk_client::ApiResult;
use motodesk_ui::gpui::prelude::FluentBuilder as _;
use motodesk_ui::gpui::*;
use motodesk_ui::*;

use self::thread::{SUGGESTIONS, Turn};
use crate::assistant::AssistantState;
use crate::backend::ApiExt as _;
use crate::format::humanize_error;
use crate::router::{self, Router};

/// The screens a link written by the assistant may open (`#/sales`).
const LINKABLE: [&str; 13] = [
    router::DASHBOARD,
    router::ASSISTANT,
    router::CLIENTS,
    router::ATELIER,
    router::PARC,
    router::STOCK,
    router::SALES,
    router::BILLING,
    router::SHARED,
    router::TEMPLATES,
    router::EMPLOYEES,
    router::DATA,
    router::SETTINGS,
];

/// A link of the assistant that is not a web page: only a screen of the app is followed.
fn follow_link(url: &str, cx: &mut App) {
    if let Some(screen) = url.strip_prefix("#/").and_then(|id| LINKABLE.into_iter().find(|known| *known == id)) {
        Router::global(cx).update(cx, |router, cx| router.go(screen, cx));
    }
}

pub struct AssistantPage {
    /// Whether this PC has a key; `None` until the service has said.
    configured: Option<bool>,
    /// The service could not say: the screen falls back to the demonstration, and says why.
    credentials_error: Option<String>,
    /// The conversations the service keeps, the latest first. Empty without a key.
    recorded: Vec<ConversationSummary>,
    active: Option<Conversation>,
    /// The answer being written, its two messages, and the listening that stops when dropped.
    turn: Option<Turn>,
    listening: Option<Task<()>>,
    stream_error: Option<String>,
    /// Bumped by every change of conversation, so that one opened late does not replace it.
    opens: u64,

    draft: TextModel,
    query: TextModel,
    log: ScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl AssistantPage {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let draft = TextModel::builder().placeholder("Demander à l'assistant…").composer(6).build(window, cx);
        let query = TextModel::builder().placeholder("Rechercher…").build(window, cx);
        let subscriptions = vec![
            draft.on_event(window, cx, |this: &mut Self, event, window, cx| match event {
                TextEvent::Submitted { with_modifier: false } => this.send_draft(window, cx),
                // The send button follows what is typed.
                TextEvent::Changed => cx.notify(),
                _ => {}
            }),
            query.on_event(window, cx, |_, event, _, cx| {
                if event == TextEvent::Changed {
                    cx.notify();
                }
            }),
            cx.observe(&AssistantState::global(cx), |_, _, cx| cx.notify()),
        ];
        let mut page = Self {
            configured: None,
            credentials_error: None,
            recorded: Vec::new(),
            active: None,
            turn: None,
            listening: None,
            stream_error: None,
            opens: 0,
            draft,
            query,
            log: ScrollHandle::new(),
            _subscriptions: subscriptions,
        };
        page.boot(cx);
        page.draft.focus(window, cx);
        page
    }

    fn boot(&mut self, cx: &mut Context<Self>) {
        cx.api(
            |client| async move {
                let status = assistant::credentials(&client).await?;
                let recorded = if status.configured { assistant::conversations(&client).await.unwrap_or_default() } else { Vec::new() };
                Ok((status.configured, recorded))
            },
            |this, answer: ApiResult<(bool, Vec<ConversationSummary>)>, cx| {
                match answer {
                    Ok((configured, recorded)) => {
                        this.configured = Some(configured);
                        this.recorded = recorded;
                    }
                    Err(error) => {
                        this.credentials_error = Some(humanize_error(&error, "Impossible de lire la configuration OpenRouter."));
                        this.configured = Some(false);
                    }
                }
                cx.notify();
            },
        );
    }

    fn live(&self) -> bool {
        self.configured == Some(true)
    }

    fn streaming(&self) -> bool {
        self.listening.is_some()
    }

    /// The list on the left: what the service keeps, or the demonstrations of this run.
    fn rail(&self, cx: &App) -> Vec<ConversationSummary> {
        if self.live() { self.recorded.clone() } else { AssistantState::global(cx).read(cx).demo.iter().map(Conversation::summary).collect() }
    }

    /// Stop listening to an answer. The service finishes writing it and keeps it: it is there
    /// when its conversation is opened again.
    fn stop_listening(&mut self) {
        self.listening = None;
        self.turn = None;
        self.stream_error = None;
    }

    /// A demonstration conversation changed: the copy that outlives this screen follows.
    fn keep_demo(&self, cx: &mut Context<Self>) {
        if let (false, Some(conversation)) = (self.live(), self.active.as_ref()) {
            AssistantState::global(cx).update(cx, |state, cx| state.keep_demo(conversation, cx));
        }
    }

    // ---- conversations -------------------------------------------------------------------------

    fn start_new(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.stop_listening();
        self.opens += 1;
        self.active = None;
        self.draft.clear(window, cx);
        self.draft.focus(window, cx);
        cx.notify();
    }

    fn open(&mut self, id: String, cx: &mut Context<Self>) {
        if self.active.as_ref().is_some_and(|open| open.id == id) {
            return;
        }
        self.stop_listening();
        self.opens += 1;
        if !self.live() {
            self.active = AssistantState::global(cx).read(cx).demo.iter().find(|kept| kept.id == id).cloned();
            self.log.scroll_to_bottom();
            cx.notify();
            return;
        }
        let asked = self.opens;
        cx.api(
            move |client| async move { assistant::conversation(&client, &id).await },
            move |this, answer, cx| {
                if this.opens != asked {
                    return;
                }
                // One that cannot be read (deleted from another window) leaves the screen empty.
                this.active = answer.ok();
                this.log.scroll_to_bottom();
                cx.notify();
            },
        );
    }

    fn ask_delete(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let page = cx.entity();
        Confirm::new("Supprimer la conversation", "Cette conversation et ses canevas seront retirés de cet appareil.")
            .confirm_label("Supprimer")
            .danger()
            .open(window, cx, move |window, cx| {
                let id = id.clone();
                page.update(cx, |this, cx| this.delete(id, window, cx));
            });
    }

    fn delete(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let was_open = self.active.as_ref().is_some_and(|open| open.id == id);
        if was_open {
            self.stop_listening();
            self.opens += 1;
            self.active = None;
        }
        if !self.live() {
            AssistantState::global(cx).update(cx, |state, cx| {
                state.demo.retain(|kept| kept.id != id);
                cx.notify();
            });
            cx.notify();
            return;
        }
        cx.api_in(
            window,
            {
                let id = id.clone();
                move |client| async move { assistant::delete_conversation(&client, &id).await }
            },
            move |this, answer, window, cx| {
                match answer {
                    // Already gone is what was asked for.
                    Ok(()) => this.recorded.retain(|row| row.id != id),
                    Err(error) if error.is_not_found() => this.recorded.retain(|row| row.id != id),
                    Err(error) => toast::error(humanize_error(&error, "La conversation n'a pas pu être supprimée."), window, cx),
                }
                cx.notify();
            },
        );
        cx.notify();
    }

    // ---- sending -------------------------------------------------------------------------------

    fn send_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(prompt) = self.draft.trimmed(cx) else { return };
        if self.streaming() {
            return;
        }
        self.draft.clear(window, cx);
        if self.live() { self.send_live(prompt, cx) } else { self.send_demo(&prompt, thread::STUB_REPLY, None, cx) }
    }

    fn suggest(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(suggestion) = SUGGESTIONS.get(index) else { return };
        if self.streaming() {
            return;
        }
        self.draft.clear(window, cx);
        if self.live() {
            self.send_live(suggestion.prompt.to_string(), cx);
            return;
        }
        let (reply, artifact) = AssistantState::global(cx).update(cx, |state, _| thread::demo_turn(suggestion.id, &mut state.ids));
        self.send_demo(suggestion.prompt, reply, Some(artifact), cx);
    }

    fn send_demo(&mut self, prompt: &str, reply: &str, shown: Option<Artifact>, cx: &mut Context<Self>) {
        let now = thread::now();
        AssistantState::global(cx).update(cx, |state, _| {
            let conversation = self.active.get_or_insert_with(|| thread::empty(state.ids.next("conversation"), &now));
            thread::demo_exchange(conversation, prompt, reply, shown, &mut state.ids, &now);
        });
        self.keep_demo(cx);
        self.log.scroll_to_bottom();
        cx.notify();
    }

    fn send_live(&mut self, prompt: String, cx: &mut Context<Self>) {
        let state = AssistantState::global(cx);
        let request = ChatRequest { conversation_id: thread::recorded_id(self.active.as_ref()), message: prompt.clone(), model: state.read(cx).prefs.model() };
        let turn = state.update(cx, |state, _| thread::begin(&mut self.active, &prompt, &mut state.ids, &thread::now()));
        self.turn = Some(turn);
        self.stream_error = None;
        self.log.scroll_to_bottom();
        self.listening = Some(match dev::scripted_answer() {
            Some(text) => dev::play(text, request, cx),
            None => cx.api_stream(
                move |client, report| async move { assistant::stream_chat(&client, &request, report).await },
                Self::heard,
                Self::answered,
            ),
        });
        cx.notify();
    }

    /// One event of the answer being written.
    fn heard(&mut self, event: ChatEvent, cx: &mut Context<Self>) {
        let (Some(conversation), Some(turn)) = (self.active.as_mut(), self.turn.as_ref()) else { return };
        thread::apply(conversation, &event, turn, &thread::now());
        match &event {
            // Recorded, or renamed: the conversation takes its place at the top of the list.
            ChatEvent::Start { .. } | ChatEvent::Done { .. } => thread::list_first(&mut self.recorded, conversation),
            ChatEvent::Error { message } => self.stream_error = Some(message.clone()),
            ChatEvent::Delta { .. } => {}
        }
        self.log.scroll_to_bottom();
        cx.notify();
    }

    /// The answer is over, or could not start.
    fn answered(&mut self, result: ApiResult<()>, cx: &mut Context<Self>) {
        self.listening = None;
        self.turn = None;
        if let Err(error) = result {
            let reason = humanize_error(&error, "Impossible de joindre l'assistant.");
            if let Some(conversation) = self.active.as_mut() {
                thread::fail(conversation, &reason);
            }
            self.stream_error = Some(reason);
        }
        self.log.scroll_to_bottom();
        cx.notify();
    }

    // ---- the canvas ----------------------------------------------------------------------------

    fn show_artifact(&mut self, id: String, cx: &mut Context<Self>) {
        if let Some(conversation) = self.active.as_mut() {
            conversation.active_artifact_id = Some(id);
        }
        self.keep_demo(cx);
        cx.notify();
    }

    fn answer_permission(&mut self, artifact_id: String, approved: bool, cx: &mut Context<Self>) {
        let Some(conversation) = self.active.as_mut() else { return };
        AssistantState::global(cx).update(cx, |state, _| thread::resolve_permission(conversation, &artifact_id, approved, &mut state.ids, &thread::now()));
        self.keep_demo(cx);
        self.log.scroll_to_bottom();
        cx.notify();
    }

    fn list_artifacts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let page = cx.entity();
        Modal::new("Documents et canevas").width(px(480.)).open(
            window,
            cx,
            move |_, cx| {
                let this = page.read(cx);
                let artifacts = this.active.as_ref().map(|open| open.artifacts.as_slice()).unwrap_or_default();
                if artifacts.is_empty() {
                    return element(div().h(px(160.)).child(
                        StateView::empty().compact().icon(IconName::Layers).title("Aucun artefact").body("Les tableaux, documents et demandes d'autorisation de cette conversation apparaîtront ici."),
                    ));
                }
                let shown = this.active.as_ref().and_then(Conversation::active_artifact).map(|artifact| artifact.id.clone());
                let rows = artifacts.iter().enumerate().map(|(index, artifact)| {
                    let (page, id) = (page.clone(), artifact.id.clone());
                    ListRow::new(("assistant-artifact", index), artifact.title.clone())
                        .icon(artifact_icon(&artifact.body))
                        .meta(thread::artifact_meta(artifact))
                        .selected(shown.as_deref() == Some(artifact.id.as_str()))
                        .on_click(move |_, window, cx| {
                            page.update(cx, |this, cx| this.show_artifact(id.clone(), cx));
                            close_modal(window, cx);
                        })
                });
                element(col().children(rows.collect::<Vec<_>>()))
            },
            |_, _| vec![element(Button::new("assistant-artifacts-close").label("Fermer").on_click(|_, window, cx| close_modal(window, cx)))],
        );
    }

    fn toggle_rail(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let state = AssistantState::global(cx);
        let mut prefs = state.read(cx).prefs.clone();
        prefs.rail_collapsed = !prefs.rail_collapsed;
        // The list folds either way; only remembering it for the next run can fail.
        if let Err(error) = state.update(cx, |state, cx| state.save(prefs, cx)) {
            toast::error(format!("Ce choix n'a pas pu être enregistré : {error}"), window, cx);
        }
    }

    // ---- drawing -------------------------------------------------------------------------------

    fn banner(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let m = cx.ui().m;
        let reason = self
            .credentials_error
            .clone()
            .unwrap_or_else(|| "Ajoutez une clé API pour parler au modèle. Les suggestions restent une démonstration du canevas.".to_string());
        div().flex_none().child(
            Alert::info().flush().title("OpenRouter non configuré").child(
                col().gap(m.space_1).items_start().child(reason).child(Button::new("assistant-open-settings").link().small().label("Ouvrir les réglages").on_click(
                    |_, _, cx| Router::global(cx).update(cx, |router, cx| router.go_with(router::SETTINGS, [("cat", "assistant")], cx)),
                )),
            ),
        )
    }

    fn history(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.ui().clone();
        let (c, m) = (theme.colors, theme.m);
        let new = IconButton::new("assistant-new", IconName::Plus)
            .title("Nouvelle conversation")
            .on_click(cx.listener(|this, _, window, cx| this.start_new(window, cx)));
        let fold = |title: &'static str| {
            IconButton::new("assistant-fold", IconName::PanelLeft).title(title).on_click(cx.listener(|this, _, window, cx| this.toggle_rail(window, cx)))
        };
        if AssistantState::global(cx).read(cx).prefs.rail_collapsed {
            return col()
                .flex_none()
                .w(m.sidebar_w_collapsed)
                .items_center()
                .gap(m.space_1)
                .py(m.space_2)
                .bg(c.bg_sidebar)
                .border_r_1()
                .border_color(c.border)
                .child(fold("Afficher l'historique"))
                .child(new)
                .into_any_element();
        }

        let all = self.rail(cx);
        let query = self.query.text(cx);
        let rows = thread::matching(&all, &query);
        let open = self.active.as_ref().map(|open| open.id.as_str());
        let now = Utc::now();
        let list: AnyElement = if rows.is_empty() {
            let (title, body) = if query.trim().is_empty() {
                ("Aucune conversation", "Écrivez un message ou choisissez une suggestion.")
            } else {
                ("Aucun résultat", "Aucun titre ne correspond.")
            };
            div().h(px(180.)).child(StateView::empty().compact().icon(IconName::Sparkles).title(title).body(body)).into_any_element()
        } else {
            col()
                .children(rows.into_iter().enumerate().map(|(index, row)| {
                    let (to_open, to_delete) = (row.id.clone(), row.id.clone());
                    ListRow::new(("assistant-conversation", index), row.title.clone())
                        .dense()
                        .icon(IconName::Sparkles)
                        .meta(thread::relative(&row.updated_at, now))
                        .selected(open == Some(row.id.as_str()))
                        .trailing(
                            IconButton::new(("assistant-delete", index), IconName::Trash).title("Supprimer").size(IconButtonSize::Xs).danger().on_click(cx.listener(
                                move |this, _, window, cx| {
                                    // The row under the button would open the conversation being deleted.
                                    cx.stop_propagation();
                                    this.ask_delete(to_delete.clone(), window, cx)
                                },
                            )),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| this.open(to_open.clone(), cx)))
                }))
                .into_any_element()
        };
        FilterRail::new("assistant-history", "Conversations")
            .action(new)
            .action(fold("Masquer l'historique"))
            .child(div().p(m.space_2).child(Input::new(&self.query).icon(IconName::Search).clearable()))
            .child(list)
            .into_any_element()
    }

    fn composer(&self, cx: &mut Context<Self>) -> Composer {
        let page = cx.entity();
        Composer::new("assistant-composer", &self.draft)
            .loading(self.streaming())
            .on_send(move |window, cx| page.update(cx, |this, cx| this.send_draft(window, cx)))
    }

    /// The exchange: an invitation while nothing was said, then the messages and the field.
    fn chat(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.ui().clone();
        let (c, m) = (theme.colors, theme.m);
        let pane = col().flex_1().min_w(px(280.)).min_h_0().bg(c.bg_app).border_r_1().border_color(c.border);
        let messages = self.active.as_ref().map(|open| open.messages.as_slice()).unwrap_or_default();
        if messages.is_empty() {
            let lead = if self.live() {
                "Posez une question : le modèle répond ici, le canevas à droite."
            } else {
                "Recherche, documents, stock, atelier — l'assistant agira ici. Les suggestions ci-dessous montrent le canevas tant qu'aucune clé n'est configurée."
            };
            let live = self.live();
            let suggestions: Vec<Div> = SUGGESTIONS.iter().enumerate().map(|(index, suggestion)| {
                // With a key the tile asks the model its question; without, it says what it demonstrates.
                div().w(relative(0.5)).child(
                    TileButton::new(("assistant-suggestion", index), suggestion.label)
                        .icon(IconName::Sparkles)
                        .description(if live { suggestion.prompt } else { suggestion.description })
                        .divided(index % 2 == 1)
                        .on_click(cx.listener(move |this, _, window, cx| this.suggest(index, window, cx))),
                )
            })
            .collect();
            return pane.justify_center().items_center().child(
                col()
                    .w_full()
                    .max_w(px(560.))
                    .items_center()
                    .gap(m.space_4)
                    .p(m.space_6)
                    .child(Heading::new("Comment puis-je aider ?").size(HeadingSize::Xl2))
                    .child(div().fs(m.fs_sm).text_color(c.text_muted).text_center().child(lead))
                    .child(self.composer(cx))
                    .child(row().w_full().flex_wrap().bg(c.bg_surface).border_1().border_color(c.border).children(suggestions)),
            );
        }

        let streaming = self.streaming();
        let last = messages.len() - 1;
        let bubbles = messages.iter().enumerate().map(|(index, message)| {
            let mine = message.role == Role::User;
            let who = row()
                .gap(m.space_1)
                .items_center()
                .when(mine, |this| this.flex_row_reverse())
                .fs_tight(m.fs_xs)
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(c.text_muted)
                .child(Icon::new(if mine { IconName::User } else { IconName::Bot }).color(c.text_muted))
                .child(if mine { "VOUS" } else { "ASSISTANT" });
            let body = div().px(m.space_3).py(m.space_2).fs(m.fs_md).text_color(c.text);
            let body = if mine {
                body.bg(c.accent_soft).child(message.content.clone())
            } else if message.content.is_empty() && streaming && index == last {
                body.bg(c.bg_surface).border_1().border_color(c.border_subtle).child(Spinner::new())
            } else {
                body.bg(c.bg_surface).border_1().border_color(c.border_subtle).child(
                    Prose::new(ElementId::Name(format!("assistant-message-{}", message.id).into()), message.content.clone()).compact().on_link(|url, _, cx| follow_link(url, cx)),
                )
            };
            col().max_w(px(672.)).gap(m.space_1).when(mine, |this| this.self_end().items_end()).child(who).child(body)
        });
        pane.child(
            div()
                .id("assistant-log")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(&self.log)
                .child(col().p(m.space_4).gap(m.space_3).children(bubbles.collect::<Vec<_>>())),
        )
        .when_some(self.stream_error.clone(), |this, error| this.child(div().flex_none().px(m.space_4).py(m.space_2).fs(m.fs_sm).text_color(c.danger).child(error)))
        .child(div().flex_none().px(m.space_4).py(m.space_3).bg(c.bg_surface).border_t_1().border_color(c.border).child(self.composer(cx)))
    }

    /// What the assistant laid out: a document, a table, indicators, a request for permission.
    fn canvas(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.ui().clone();
        let (c, m) = (theme.colors, theme.m);
        let artifact = self.active.as_ref().and_then(Conversation::active_artifact);
        let head = row()
            .flex_none()
            .h(m.toolbar_h)
            .px(m.space_3)
            .gap(m.space_2)
            .items_center()
            .bg(c.bg_toolbar)
            .border_b_1()
            .border_color(c.border)
            .child(div().flex_1().min_w_0().one_line().child(Heading::new(artifact.map_or("Canevas".to_string(), |shown| shown.title.clone())).size(HeadingSize::Sm).tone(Tone::Muted)))
            .child(
                IconButton::new("assistant-artifacts", IconName::Layers)
                    .title("Documents et canevas de la conversation")
                    .on_click(cx.listener(|this, _, window, cx| this.list_artifacts(window, cx))),
            );
        let page = |content: AnyElement| div().w_full().max_w(px(760.)).mx_auto().p(m.space_6).child(content).into_any_element();
        let body: AnyElement = match artifact.map(|shown| (shown.id.clone(), &shown.body)) {
            None => StateView::empty()
                .icon(IconName::LayoutTemplate)
                .title("Canevas vide")
                .body("L'assistant affichera ici documents, tableaux, graphiques et demandes d'autorisation.")
                .into_any_element(),
            Some((id, ArtifactBody::Markdown(text))) => page(
                Prose::new(ElementId::Name(format!("assistant-canvas-{id}").into()), text.clone()).on_link(|url, _, cx| follow_link(url, cx)).into_any_element(),
            ),
            Some((_, ArtifactBody::Table { columns, rows })) => {
                let cells = rows.clone();
                page(
                    DataTable::new("assistant-canvas-table", columns.iter().map(|label| Column::new(label.clone()).flex(1.)).collect(), rows.len(), move |row, column, _, _| {
                        div().one_line().child(cells.get(row).and_then(|cells| cells.get(column)).cloned().unwrap_or_default()).into_any_element()
                    })
                    .compact()
                    .auto_height()
                    .into_any_element(),
                )
            }
            Some((_, ArtifactBody::Kpis(items))) => page(
                row()
                    .items_stretch()
                    .gap(m.space_3)
                    .children(items.iter().map(|item| {
                        let tile = KpiTile::new(item.label.clone(), item.value.clone());
                        div().flex_1().min_w_0().child(match &item.hint {
                            Some(hint) => tile.hint(hint.clone()),
                            None => tile,
                        })
                    }))
                    .into_any_element(),
            ),
            Some((id, ArtifactBody::Permission { tool, summary, status })) => {
                let (tone, answered) = match status {
                    Permission::Pending => (Alert::warning(), None),
                    Permission::Approved => (Alert::success(), Some(Status::Success)),
                    Permission::Denied => (Alert::danger(), Some(Status::Danger)),
                };
                let answer = |approved: bool| {
                    let id = id.clone();
                    cx.listener(move |this: &mut Self, _: &ClickEvent, _: &mut Window, cx| this.answer_permission(id.clone(), approved, cx))
                };
                page(
                    col()
                        .max_w(px(520.))
                        .gap(m.space_4)
                        .child(tone.title("Autorisation requise").child(summary.clone()))
                        .child(InfoList::new(vec![InfoItem::new("Outil", Some(tool.clone())).mono(), InfoItem::new("État", Some(status.label()))]))
                        .child(match answered {
                            None => row()
                                .gap(m.space_2)
                                .justify_end()
                                .child(Button::new("assistant-permission-deny").label("Refuser").on_click(answer(false)))
                                .child(Button::new("assistant-permission-allow").primary().label("Autoriser").on_click(answer(true))),
                            Some(outcome) => row().child(Tag::new(status.label()).status(outcome).medium()),
                        })
                        .into_any_element(),
                )
            }
            Some((_, ArtifactBody::Question { prompt })) => page(Alert::info().title("Question").child(prompt.clone()).into_any_element()),
        };
        col()
            .flex_1()
            .min_w(px(320.))
            .min_h_0()
            .bg(c.bg_surface)
            .child(head)
            .child(div().id("assistant-canvas").flex_1().min_h_0().overflow_y_scroll().child(body))
    }
}

fn artifact_icon(body: &ArtifactBody) -> IconName {
    match body {
        ArtifactBody::Markdown(_) => IconName::FileText,
        ArtifactBody::Table { .. } => IconName::Table,
        ArtifactBody::Kpis(_) => IconName::Gauge,
        ArtifactBody::Permission { .. } => IconName::Shield,
        ArtifactBody::Question { .. } => IconName::Info,
    }
}

impl Render for AssistantPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = cx.ui().colors;
        if self.configured.is_none() {
            return col().size_full().bg(c.bg_app).child(StateView::loading());
        }
        col()
            .size_full()
            .bg(c.bg_app)
            .when(!self.live(), |this| this.child(self.banner(cx)))
            .child(row().flex_1().min_h_0().items_stretch().child(self.history(cx)).child(self.chat(cx)).child(self.canvas(cx)))
    }
}

/// Development only: an answer read from a file instead of asked of a model, to watch the
/// screen write one without a key or a network. `MOTODESK_DEV_ASSISTANT_REPLY=<markdown file>`.
/// Nothing is recorded by the service; the variable does not exist in a published version.
mod dev {
    use super::*;

    #[cfg(not(debug_assertions))]
    pub fn scripted_answer() -> Option<String> {
        None
    }

    #[cfg(debug_assertions)]
    pub fn scripted_answer() -> Option<String> {
        std::fs::read_to_string(std::env::var_os("MOTODESK_DEV_ASSISTANT_REPLY")?).ok()
    }

    /// The events the service would send for `text`, a few characters at a time.
    pub fn play(text: String, request: ChatRequest, cx: &mut Context<AssistantPage>) -> Task<()> {
        let conversation_id = request.conversation_id.unwrap_or_else(|| "dev-conversation".to_string());
        let mut events = vec![ChatEvent::Start { conversation_id, user_message_id: "dev-user".into(), assistant_message_id: "dev-assistant".into() }];
        let letters: Vec<char> = text.chars().collect();
        events.extend(letters.chunks(9).map(|piece| ChatEvent::Delta { text: piece.iter().collect() }));
        // An empty title keeps the one on screen, as the service's does for a conversation under way.
        events.push(ChatEvent::Done { title: String::new() });
        cx.spawn(async move |page, cx| {
            for event in events {
                cx.background_executor().timer(std::time::Duration::from_millis(45)).await;
                if page.update(cx, |page, cx| page.heard(event, cx)).is_err() {
                    return;
                }
            }
            let _ = page.update(cx, |page, cx| page.answered(Ok(()), cx));
        })
    }
}
