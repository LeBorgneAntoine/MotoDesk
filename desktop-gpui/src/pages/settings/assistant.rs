//! The assistant of this PC: its OpenRouter key and the model it talks to.
//!
//! Neither goes to the shop's settings, which travel to every PC: the key is kept by the
//! service beside the database (and in the system's keychain), the model in a file of this PC.
//! Only an administrator sets, tests or erases the key; anyone chooses the model.

use motodesk_client::assistant::{self, CURATED_MODELS, CUSTOM_MODEL, Credentials};
use motodesk_ui::gpui::prelude::FluentBuilder as _;
use motodesk_ui::gpui::*;
use motodesk_ui::*;

use crate::assistant::AssistantState;
use crate::auth::AuthStore;
use crate::backend::ApiExt as _;
use crate::format::humanize_error;

/// What the key is busy with: one thing at a time.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Busy {
    Saving,
    Testing,
    Erasing,
}

pub struct AssistantSettings {
    status: Option<Credentials>,
    error: Option<String>,
    busy: Option<Busy>,
    key: TextModel,
    model: SelectModel,
    slug: TextModel,
    _subscriptions: Vec<Subscription>,
}

/// What the list and the field show for a stored choice: a model of the list, or « another »
/// with its identifier.
fn shown_choice(default_model: &str, custom_slug: &str) -> (String, String) {
    if !custom_slug.trim().is_empty() {
        return (CUSTOM_MODEL.to_string(), custom_slug.trim().to_string());
    }
    if CURATED_MODELS.iter().any(|(id, _)| *id == default_model) {
        (default_model.to_string(), String::new())
    } else {
        (CUSTOM_MODEL.to_string(), default_model.to_string())
    }
}

/// What to keep for what the list and the field show: the model, and the identifier typed by
/// hand when « another » is chosen. `Err` says why nothing is kept; `Ok(None)` that there is
/// nothing to keep yet (« another », and nothing typed).
fn choice_to_keep(selected: &str, typed: &str) -> Result<Option<(String, String)>, &'static str> {
    if selected != CUSTOM_MODEL {
        // A model of the list: an identifier typed earlier no longer applies.
        return Ok(Some((selected.to_string(), String::new())));
    }
    let typed = typed.trim();
    if typed.is_empty() {
        return Ok(None);
    }
    if !assistant::is_valid_model_slug(typed) {
        return Err("Le slug du modèle doit ressembler à fournisseur/modèle.");
    }
    Ok(Some((typed.to_string(), typed.to_string())))
}

impl AssistantSettings {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let prefs = AssistantState::global(cx).read(cx).prefs.clone();
        let (selected, typed) = shown_choice(&prefs.default_model, &prefs.custom_slug);
        let key = TextModel::builder().placeholder("sk-or-…").password().build(window, cx);
        let slug = TextModel::builder().placeholder("fournisseur/modèle").value(typed).build(window, cx);
        let options = CURATED_MODELS
            .iter()
            .map(|(id, label)| SelectOption::new(*id, *label))
            .chain([SelectOption::new(CUSTOM_MODEL, "Autre (saisie libre)")])
            .collect();
        let model = SelectModel::new(options, Some(selected.as_str()), window, cx);

        let subscriptions = vec![
            key.on_event(window, cx, |this: &mut Self, event, window, cx| match event {
                TextEvent::Submitted { .. } => this.save_key(window, cx),
                // The button follows what is typed.
                TextEvent::Changed => cx.notify(),
                _ => {}
            }),
            model.on_change(window, cx, |this: &mut Self, _, window, cx| this.keep_model(true, window, cx)),
            // An identifier is kept as soon as it reads as one, quietly: a click elsewhere does
            // not always take the keyboard out of the field, and what was typed must not be
            // lost for that. Enter says so.
            slug.on_event(window, cx, |this: &mut Self, event, window, cx| match event {
                TextEvent::Submitted { .. } => this.keep_model(true, window, cx),
                TextEvent::Changed | TextEvent::Blurred => this.keep_model(false, window, cx),
                _ => {}
            }),
        ];
        let mut view = Self { status: None, error: None, busy: None, key, model, slug, _subscriptions: subscriptions };
        view.refresh(cx);
        view
    }

    fn is_admin(cx: &App) -> bool {
        AuthStore::global(cx).read(cx).is_admin()
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        cx.api(
            |client| async move { assistant::credentials(&client).await },
            |this, answer, cx| {
                match answer {
                    Ok(status) => this.status = Some(status),
                    Err(error) => this.error = Some(humanize_error(&error, "Impossible de lire la clé.")),
                }
                cx.notify();
            },
        );
    }

    fn save_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(key) = self.key.trimmed(cx) else { return };
        if self.busy.is_some() {
            return;
        }
        self.busy = Some(Busy::Saving);
        self.error = None;
        cx.notify();
        cx.api_in(
            window,
            move |client| async move { assistant::store_key(&client, &key).await },
            |this, answer, window, cx| {
                this.busy = None;
                match answer {
                    Ok(status) if status.configured => {
                        this.status = Some(status);
                        // The key is not shown again once it is kept.
                        this.key.clear(window, cx);
                        toast::success("Clé OpenRouter enregistrée sur cet appareil.", window, cx);
                    }
                    Ok(status) => {
                        this.status = Some(status);
                        this.error = Some("La clé n'a pas pu être enregistrée sur cet appareil.".to_string());
                    }
                    Err(error) => this.error = Some(humanize_error(&error, "Enregistrement impossible.")),
                }
                cx.notify();
            },
        );
    }

    fn erase_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy.is_some() {
            return;
        }
        self.busy = Some(Busy::Erasing);
        self.error = None;
        cx.notify();
        cx.api_in(
            window,
            |client| async move { assistant::forget_key(&client).await },
            |this, answer, window, cx| {
                this.busy = None;
                match answer {
                    Ok(()) => {
                        this.status = Some(Credentials::default());
                        toast::success("Clé OpenRouter effacée.", window, cx);
                    }
                    Err(error) => this.error = Some(humanize_error(&error, "Suppression impossible.")),
                }
                cx.notify();
            },
        );
    }

    /// Ask OpenRouter, with the stored key, whether it answers.
    fn test(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy.is_some() {
            return;
        }
        self.busy = Some(Busy::Testing);
        self.error = None;
        cx.notify();
        cx.api_in(
            window,
            |client| async move { assistant::test_connection(&client).await },
            |this, answer, window, cx| {
                this.busy = None;
                match answer {
                    Ok(reached) if reached.model_count > 0 => {
                        toast::success(format!("Connexion OpenRouter réussie · {} modèle(s) visibles.", reached.model_count), window, cx)
                    }
                    Ok(_) => toast::success("Connexion OpenRouter réussie.", window, cx),
                    Err(error) => this.error = Some(humanize_error(&error, "Test de connexion impossible.")),
                }
                cx.notify();
            },
        );
    }

    /// Keep the model shown. `announce` when the user asked for it (a choice in the list,
    /// Enter): the outcome is then said, a refusal included.
    fn keep_model(&mut self, announce: bool, window: &mut Window, cx: &mut Context<Self>) {
        let selected = self.model.value(cx).unwrap_or_default();
        let (model, custom) = match choice_to_keep(&selected, &self.slug.text(cx)) {
            Ok(Some(choice)) => choice,
            Ok(None) => {
                cx.notify();
                return;
            }
            Err(why) => {
                // While typing, the field itself says what is expected.
                if announce {
                    self.error = Some(why.to_string());
                }
                cx.notify();
                return;
            }
        };
        self.error = None;
        let state = AssistantState::global(cx);
        let mut prefs = state.read(cx).prefs.clone();
        let unchanged = (prefs.default_model.as_str(), prefs.custom_slug.as_str()) == (model.as_str(), custom.as_str());
        prefs.default_model = model;
        prefs.custom_slug = custom;
        let kept = if unchanged { Ok(()) } else { state.update(cx, |state, cx| state.save(prefs, cx)) };
        match kept {
            Ok(()) if announce => toast::success("Modèle par défaut enregistré sur cet appareil.", window, cx),
            Ok(()) => {}
            Err(error) => toast::error(format!("Ce réglage n'a pas pu être enregistré : {error}"), window, cx),
        }
        cx.notify();
    }
}

impl Render for AssistantSettings {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let m = cx.ui().m;
        let is_admin = Self::is_admin(cx);
        let configured = self.status.as_ref().is_some_and(|status| status.configured);
        let typed_key = self.key.trimmed(cx).is_some();
        let other = self.model.value(cx).as_deref() == Some(CUSTOM_MODEL);
        let slug = self.slug.text(cx);
        let slug_refused = other && !slug.trim().is_empty() && !assistant::is_valid_model_slug(&slug);
        let slug_kept = other && AssistantState::global(cx).read(cx).prefs.custom_slug == slug.trim() && !slug.trim().is_empty();
        let busy = self.busy;

        col()
            .max_w(px(640.))
            .gap(m.space_4)
            .child(Alert::info().child("La clé et le modèle restent sur cet ordinateur. Ils ne sont pas répliqués vers les autres postes de l'atelier."))
            .when_some(self.error.clone(), |this, error| this.child(Alert::danger().child(error)))
            .when_some(self.status.clone(), |this, status| {
                this.child(match status.hint.filter(|_| status.configured) {
                    Some(hint) => Alert::success().child(format!("Clé enregistrée ({hint}).")),
                    None => Alert::warning().child("Aucune clé OpenRouter n'est enregistrée sur cet appareil."),
                })
            })
            .when(is_admin, |this| {
                this.child(
                    FormField::new("Clé API OpenRouter")
                        .hint("Créée sur openrouter.ai. Elle n'est jamais renvoyée après enregistrement.")
                        .child(Input::new(&self.key).reveal_toggle().disabled(busy.is_some())),
                )
                .child(
                    row()
                        .gap(m.space_2)
                        .child(
                            Button::new("assistant-key-save")
                                .primary()
                                .label("Enregistrer la clé")
                                .loading(busy == Some(Busy::Saving))
                                .disabled(!typed_key || busy.is_some())
                                .on_click(cx.listener(|this, _, window, cx| this.save_key(window, cx))),
                        )
                        .child(
                            Button::new("assistant-key-test")
                                .label("Tester la connexion")
                                .loading(busy == Some(Busy::Testing))
                                .disabled(!configured || busy.is_some())
                                .on_click(cx.listener(|this, _, window, cx| this.test(window, cx))),
                        )
                        .child(
                            Button::new("assistant-key-erase")
                                .ghost()
                                .label("Effacer")
                                .loading(busy == Some(Busy::Erasing))
                                .disabled(!configured || busy.is_some())
                                .on_click(cx.listener(|this, _, window, cx| this.erase_key(window, cx))),
                        ),
                )
            })
            .when(!is_admin, |this| this.child(Alert::info().child("La clé est gérée par les administrateurs. Le modèle se choisit ci-dessous.")))
            .child(FormField::new("Modèle par défaut").child(Select::new(&self.model)))
            .when(other, |this| {
                let field = FormField::new("Slug OpenRouter").child(Input::new(&self.slug).mono().invalid(slug_refused));
                this.child(if slug_refused {
                    field.error(Some("Format attendu : fournisseur/modèle"))
                } else {
                    field.hint(if slug_kept {
                        "Enregistré sur cet appareil : les messages iront à ce modèle."
                    } else {
                        "Identifiant exact, par exemple anthropic/claude-sonnet-4.5."
                    })
                })
            })
    }
}

#[cfg(test)]
mod tests {
    // Not `super::*`: it would bring in GPUI's own `test` attribute in place of Rust's.
    use motodesk_client::assistant::{CUSTOM_MODEL, DEFAULT_MODEL};

    use super::{choice_to_keep, shown_choice};

    #[test]
    fn a_stored_choice_shows_as_a_listed_model_or_as_another() {
        assert_eq!(shown_choice(DEFAULT_MODEL, ""), (DEFAULT_MODEL.to_string(), String::new()));
        assert_eq!(shown_choice("mistralai/mistral-large", ""), (CUSTOM_MODEL.to_string(), "mistralai/mistral-large".to_string()));
        // What an earlier version stored: the typed identifier in both places, or beside a listed
        // model.
        assert_eq!(shown_choice("mistralai/mistral-large", " mistralai/mistral-large "), (CUSTOM_MODEL.to_string(), "mistralai/mistral-large".to_string()));
        assert_eq!(shown_choice("openai/gpt-4.1", "x-ai/grok-4"), (CUSTOM_MODEL.to_string(), "x-ai/grok-4".to_string()));
    }

    #[test]
    fn choosing_a_listed_model_forgets_the_identifier_typed_before() {
        assert_eq!(choice_to_keep("openai/gpt-4.1", "x-ai/grok-4"), Ok(Some(("openai/gpt-4.1".to_string(), String::new()))));
        assert_eq!(choice_to_keep(CUSTOM_MODEL, " x-ai/grok-4 "), Ok(Some(("x-ai/grok-4".to_string(), "x-ai/grok-4".to_string()))));
        // « Another » with nothing typed yet keeps what was there.
        assert_eq!(choice_to_keep(CUSTOM_MODEL, "  "), Ok(None));
        assert!(choice_to_keep(CUSTOM_MODEL, "grok").is_err());
    }
}
