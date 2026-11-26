//! One conversation as the screen holds it, and what each step of an exchange does to it (port
//! of the logic of `desktop/src/lib/assistant/store.svelte.ts`, `conversation.ts` and `demo.ts`).
//! Nothing here draws or calls the service.

use chrono::{DateTime, Datelike as _, Local, Utc};
use motodesk_client::assistant::{Artifact, ArtifactBody, ChatEvent, Conversation, ConversationSummary, Kpi, Message, Permission, Role, title_from_prompt};

use crate::assistant::Ids;

/// The identifier of a conversation the service has not recorded yet.
pub const PENDING: &str = "pending";

/// The document the model's answer is laid on.
const ANSWER_TITLE: &str = "Réponse";
const ANSWER_ID: &str = "local-answer";

pub fn now() -> String {
    Utc::now().to_rfc3339()
}

fn message(id: String, role: Role, content: &str, now: &str) -> Message {
    Message { id, role, content: content.to_string(), created_at: now.to_string() }
}

pub fn empty(id: String, now: &str) -> Conversation {
    Conversation {
        id,
        title: "Nouvelle conversation".to_string(),
        updated_at: now.to_string(),
        messages: Vec::new(),
        artifacts: Vec::new(),
        active_artifact_id: None,
    }
}

// ---- an exchange with a model -----------------------------------------------------------------

/// The identifiers the two messages of an exchange carry until the service names them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Turn {
    pub user_id: String,
    pub answer_id: String,
}

/// A message is sent: it joins the conversation, followed by an answer still empty. Without a
/// conversation, one opens, named after the message.
pub fn begin(conversation: &mut Option<Conversation>, prompt: &str, ids: &mut Ids, now: &str) -> Turn {
    let turn = Turn { user_id: ids.next("message"), answer_id: ids.next("message") };
    let open = conversation.get_or_insert_with(|| Conversation { title: title_from_prompt(prompt), ..empty(PENDING.to_string(), now) });
    open.messages.push(message(turn.user_id.clone(), Role::User, prompt, now));
    open.messages.push(message(turn.answer_id.clone(), Role::Assistant, "", now));
    open.updated_at = now.to_string();
    turn
}

/// The conversation to name in a request: none while the service has not recorded it.
pub fn recorded_id(conversation: Option<&Conversation>) -> Option<String> {
    conversation.map(|open| open.id.clone()).filter(|id| id != PENDING)
}

/// Lay the answer on the canvas: the conversation's document is rewritten, or created.
fn show_answer(conversation: &mut Conversation, body: &str) {
    let id = match conversation.artifacts.iter_mut().find(|artifact| matches!(artifact.body, ArtifactBody::Markdown(_))) {
        Some(document) => {
            document.body = ArtifactBody::Markdown(body.to_string());
            document.id.clone()
        }
        None => {
            conversation.artifacts.push(Artifact { id: ANSWER_ID.to_string(), title: ANSWER_TITLE.to_string(), body: ArtifactBody::Markdown(body.to_string()) });
            ANSWER_ID.to_string()
        }
    };
    conversation.active_artifact_id = Some(id);
}

/// The answer being written: the last message, when it is the assistant's.
fn answer(conversation: &mut Conversation) -> Option<&mut Message> {
    conversation.messages.last_mut().filter(|last| last.role == Role::Assistant)
}

/// What one event of the service does to the conversation.
pub fn apply(conversation: &mut Conversation, event: &ChatEvent, turn: &Turn, now: &str) {
    match event {
        ChatEvent::Start { conversation_id, user_message_id, assistant_message_id } => {
            conversation.id = conversation_id.clone();
            for message in &mut conversation.messages {
                if message.id == turn.user_id {
                    message.id = user_message_id.clone();
                } else if message.id == turn.answer_id {
                    message.id = assistant_message_id.clone();
                }
            }
        }
        ChatEvent::Delta { text } => {
            let Some(written) = answer(conversation) else { return };
            written.content.push_str(text);
            let so_far = written.content.clone();
            show_answer(conversation, &so_far);
        }
        ChatEvent::Done { title } => {
            if !title.is_empty() {
                conversation.title = title.clone();
            }
        }
        // What was already written stays; with nothing written, the reason stands as the answer.
        ChatEvent::Error { message } => fail(conversation, message),
    }
    conversation.updated_at = now.to_string();
}

/// The answer could not be written (or not to the end): an answer left empty says why.
pub fn fail(conversation: &mut Conversation, reason: &str) {
    let Some(written) = answer(conversation) else { return };
    if written.content.is_empty() {
        written.content = reason.to_string();
    }
    let shown = written.content.clone();
    show_answer(conversation, &shown);
}

/// Where a conversation goes in the list once the service has recorded or renamed it: first.
pub fn list_first(rows: &mut Vec<ConversationSummary>, conversation: &Conversation) {
    rows.retain(|row| row.id != conversation.id && row.id != PENDING);
    rows.insert(0, conversation.summary());
}

// ---- the demonstration, on a PC without a key --------------------------------------------------

pub struct Suggestion {
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub prompt: &'static str,
}

pub const SUGGESTIONS: [Suggestion; 4] = [
    Suggestion { id: "clients", label: "Chercher un client", description: "Exemple de tableau", prompt: "Trouve le client Dupont" },
    Suggestion { id: "stock", label: "Stock bas", description: "Indicateurs de démo", prompt: "Quels articles sont en stock bas ?" },
    Suggestion { id: "devis", label: "Créer un devis", description: "Demande d'autorisation", prompt: "Prépare un devis pour Dupont Jean — révision 600 km" },
    Suggestion { id: "mois", label: "Résumé du mois", description: "Note sur le canevas", prompt: "Fais-moi un résumé d'activité du mois" },
];

pub const STUB_REPLY: &str = "L'assistant n'est pas encore connecté à un modèle. Configurez une clé OpenRouter dans Paramètres › Assistant. En attendant, les suggestions montrent le canevas.";

const DEMO_NOTE: &str = "# Activité du mois

**Août 2026** — synthèse de démonstration. Les montants sont TTC.

## Atelier

- 42 passages atelier
- 3 devis en attente
- CA TTC **18 400 €**

## Par canal

| Canal | Passages | CA TTC |
| --- | ---: | ---: |
| Comptoir | 28 | 11 200 € |
| Atelier | 14 | 7 200 € |
| **Total** | **42** | **18 400 €** |

Les chiffres viendront des outils de lecture. Voir le module [Ventes](#/sales) et la fiche `clients`.

> Aucune écriture fiscale n'est générée par cette note.
";

/// What the assistant answers to a suggestion, and what it lays on the canvas. Made-up data.
pub fn demo_turn(suggestion: &str, ids: &mut Ids) -> (&'static str, Artifact) {
    let text = |cells: &[&str]| cells.iter().map(|cell| cell.to_string()).collect::<Vec<_>>();
    let artifact = |title: &str, body: ArtifactBody, ids: &mut Ids| Artifact { id: ids.next("artifact"), title: title.to_string(), body };
    match suggestion {
        "clients" => (
            "Voici les fiches qui correspondent à « Dupont » (données de démonstration).",
            artifact(
                "Clients — Dupont",
                ArtifactBody::Table {
                    columns: text(&["Nom", "Immatriculation", "Téléphone"]),
                    rows: vec![text(&["Dupont Jean", "AB-123-CD", "06 12 34 56 78"]), text(&["Dupont Marie", "EF-456-GH", "06 98 76 54 32"])],
                },
                ids,
            ),
        ),
        "stock" => (
            "Trois références sous le seuil d'alerte (démonstration).",
            artifact(
                "Stock bas",
                ArtifactBody::Kpis(vec![
                    Kpi { label: "Références sous seuil".into(), value: "3".into(), hint: Some("casques et consommables".into()) },
                    Kpi { label: "Valeur HT".into(), value: "1 240 €".into(), hint: None },
                    Kpi { label: "Ruptures".into(), value: "1".into(), hint: Some("filtre à huile 10W40".into()) },
                ]),
                ids,
            ),
        ),
        "devis" => (
            "Je peux préparer un brouillon de devis. Confirmez l'autorisation sur le canevas — aucune écriture n'est faite dans cette démonstration.",
            artifact(
                "Autorisation — devis",
                ArtifactBody::Permission {
                    tool: "sales.create_quote".into(),
                    summary: "Créer un devis pour Dupont Jean, révision 600 km (démonstration).".into(),
                    status: Permission::Pending,
                },
                ids,
            ),
        ),
        _ => ("Voici une synthèse fictive pour illustrer le canevas.", artifact("Activité du mois", ArtifactBody::Markdown(DEMO_NOTE.to_string()), ids)),
    }
}

/// A whole demonstration exchange at once: the message, its answer, and what goes on the canvas.
pub fn demo_exchange(conversation: &mut Conversation, prompt: &str, reply: &str, shown: Option<Artifact>, ids: &mut Ids, now: &str) {
    if conversation.messages.is_empty() {
        conversation.title = title_from_prompt(prompt);
    }
    conversation.messages.push(message(ids.next("message"), Role::User, prompt, now));
    conversation.messages.push(message(ids.next("message"), Role::Assistant, reply, now));
    if let Some(artifact) = shown {
        conversation.active_artifact_id = Some(artifact.id.clone());
        conversation.artifacts.push(artifact);
    }
    conversation.updated_at = now.to_string();
}

/// The answer to a request for permission: the request keeps it, and the assistant says so.
/// A request already answered is left as it is.
pub fn resolve_permission(conversation: &mut Conversation, artifact_id: &str, approved: bool, ids: &mut Ids, now: &str) {
    let pending = conversation.artifacts.iter_mut().find_map(|artifact| match &mut artifact.body {
        ArtifactBody::Permission { status, .. } if artifact.id == artifact_id && *status == Permission::Pending => Some(status),
        _ => None,
    });
    let Some(status) = pending else { return };
    *status = if approved { Permission::Approved } else { Permission::Denied };
    let note = if approved { "Autorisation accordée (démonstration — aucune écriture)." } else { "Autorisation refusée. Rien n'a été modifié." };
    conversation.messages.push(message(ids.next("message"), Role::Assistant, note, now));
    conversation.updated_at = now.to_string();
}

// ---- the list of conversations -----------------------------------------------------------------

/// The conversations whose title contains what was typed, whatever the case.
pub fn matching<'a>(rows: &'a [ConversationSummary], query: &str) -> Vec<&'a ConversationSummary> {
    let query = query.trim().to_lowercase();
    rows.iter().filter(|row| query.is_empty() || row.title.to_lowercase().contains(&query)).collect()
}

const MONTHS: [&str; 12] = ["janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.", "déc."];

/// How long ago, in words: « à l'instant », « il y a 5 min », « hier », then the day.
pub fn relative(iso: &str, now: DateTime<Utc>) -> String {
    let Ok(then) = DateTime::parse_from_rfc3339(iso) else { return String::new() };
    let minutes = (now - then.with_timezone(&Utc)).num_minutes().max(0);
    match minutes {
        0 => "à l'instant".to_string(),
        1..60 => format!("il y a {minutes} min"),
        60..1440 => format!("il y a {} h", minutes / 60),
        1440..2880 => "hier".to_string(),
        _ => {
            let day = then.with_timezone(&Local);
            format!("{} {}", day.day(), MONTHS[day.month0() as usize])
        }
    }
}

/// One line about an artifact, for the list of what a conversation produced.
pub fn artifact_meta(artifact: &Artifact) -> String {
    match &artifact.body {
        ArtifactBody::Markdown(_) => "Document".to_string(),
        ArtifactBody::Table { rows, .. } => format!("{} ligne(s)", rows.len()),
        ArtifactBody::Kpis(items) => format!("{} indicateur(s)", items.len()),
        ArtifactBody::Permission { status, .. } => status.label().to_string(),
        ArtifactBody::Question { .. } => "Question".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone as _;

    use super::*;

    const NOW: &str = "2026-11-26T10:00:00+00:00";

    fn start() -> ChatEvent {
        ChatEvent::Start { conversation_id: "c1".into(), user_message_id: "u1".into(), assistant_message_id: "a1".into() }
    }

    fn delta(text: &str) -> ChatEvent {
        ChatEvent::Delta { text: text.into() }
    }

    #[test]
    fn a_first_message_opens_a_conversation_named_after_it() {
        let (mut open, mut ids) = (None, Ids::default());
        let turn = begin(&mut open, "Que prévoir pour la révision des 600 km ?", &mut ids, NOW);
        assert_eq!(recorded_id(open.as_ref()), None, "the service has not recorded it yet");
        let conversation = open.as_ref().unwrap();
        assert_eq!((conversation.id.as_str(), conversation.title.as_str()), (PENDING, "Que prévoir pour la révision des 600 km ?"));
        assert_eq!(
            conversation.messages.iter().map(|m| (m.id.as_str(), m.role, m.content.as_str())).collect::<Vec<_>>(),
            [(turn.user_id.as_str(), Role::User, "Que prévoir pour la révision des 600 km ?"), (turn.answer_id.as_str(), Role::Assistant, "")]
        );
        assert_ne!(turn.user_id, turn.answer_id);
    }

    #[test]
    fn an_answer_is_written_piece_by_piece_onto_the_canvas() {
        let (mut open, mut ids) = (None, Ids::default());
        let turn = begin(&mut open, "Révision", &mut ids, NOW);
        let conversation = open.as_mut().unwrap();

        apply(conversation, &start(), &turn, NOW);
        assert_eq!(recorded_id(Some(conversation)).as_deref(), Some("c1"));
        assert_eq!(conversation.messages.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["u1", "a1"]);
        assert!(conversation.artifacts.is_empty(), "nothing on the canvas before the first words");

        apply(conversation, &delta("## Révision\n"), &turn, NOW);
        apply(conversation, &delta("- vidange"), &turn, NOW);
        assert_eq!(conversation.messages[1].content, "## Révision\n- vidange");
        // One document, rewritten as the answer grows.
        assert_eq!(conversation.artifacts.len(), 1);
        assert_eq!(conversation.active_artifact().map(|a| &a.body), Some(&ArtifactBody::Markdown("## Révision\n- vidange".into())));

        apply(conversation, &ChatEvent::Done { title: "Révision des 600 km".into() }, &turn, "2026-11-26T10:00:09+00:00");
        assert_eq!((conversation.title.as_str(), conversation.updated_at.as_str()), ("Révision des 600 km", "2026-11-26T10:00:09+00:00"));

        // The next exchange of the same conversation writes over the same document.
        let mut open = Some(conversation.clone());
        let next = begin(&mut open, "Et les pneus ?", &mut ids, NOW);
        let conversation = open.as_mut().unwrap();
        assert_eq!(conversation.title, "Révision des 600 km");
        apply(conversation, &delta("À contrôler."), &next, NOW);
        assert_eq!(conversation.artifacts.len(), 1);
        assert_eq!(conversation.active_artifact().map(|a| &a.body), Some(&ArtifactBody::Markdown("À contrôler.".into())));
    }

    #[test]
    fn an_empty_title_from_the_service_keeps_the_one_shown() {
        let (mut open, mut ids) = (None, Ids::default());
        let turn = begin(&mut open, "Révision", &mut ids, NOW);
        let conversation = open.as_mut().unwrap();
        apply(conversation, &ChatEvent::Done { title: String::new() }, &turn, NOW);
        assert_eq!(conversation.title, "Révision");
    }

    #[test]
    fn a_failure_stands_as_the_answer_only_when_nothing_was_written() {
        let (mut open, mut ids) = (None, Ids::default());
        let turn = begin(&mut open, "Révision", &mut ids, NOW);
        let conversation = open.as_mut().unwrap();
        apply(conversation, &ChatEvent::Error { message: "Clé OpenRouter refusée.".into() }, &turn, NOW);
        assert_eq!(conversation.messages[1].content, "Clé OpenRouter refusée.");
        assert_eq!(conversation.active_artifact().map(|a| &a.body), Some(&ArtifactBody::Markdown("Clé OpenRouter refusée.".into())));

        // Words already written are not replaced by the reason the rest did not come.
        let mut open = None;
        let turn = begin(&mut open, "Révision", &mut ids, NOW);
        let conversation = open.as_mut().unwrap();
        apply(conversation, &delta("Vidange, puis"), &turn, NOW);
        fail(conversation, "Flux OpenRouter interrompu.");
        assert_eq!(conversation.messages[1].content, "Vidange, puis");
    }

    #[test]
    fn a_recorded_conversation_goes_first_in_the_list_once() {
        let row = |id: &str, title: &str| ConversationSummary { id: id.into(), title: title.into(), updated_at: NOW.into() };
        let mut rows = vec![row("old", "Stock"), row("c1", "Révision"), row(PENDING, "…")];
        let conversation = Conversation { title: "Révision des 600 km".into(), ..empty("c1".into(), NOW) };
        list_first(&mut rows, &conversation);
        assert_eq!(rows.iter().map(|r| (r.id.as_str(), r.title.as_str())).collect::<Vec<_>>(), [("c1", "Révision des 600 km"), ("old", "Stock")]);
    }

    #[test]
    fn every_suggestion_shows_something_different_on_the_canvas() {
        let mut ids = Ids::default();
        let mut conversation = empty(ids.next("conversation"), NOW);
        for suggestion in &SUGGESTIONS {
            let (reply, artifact) = demo_turn(suggestion.id, &mut ids);
            demo_exchange(&mut conversation, suggestion.prompt, reply, Some(artifact), &mut ids, NOW);
        }
        assert_eq!(conversation.title, "Trouve le client Dupont", "named after its first message");
        assert_eq!(conversation.messages.len(), 8);
        let kinds: Vec<&str> = conversation
            .artifacts
            .iter()
            .map(|artifact| match artifact.body {
                ArtifactBody::Table { .. } => "table",
                ArtifactBody::Kpis(_) => "kpis",
                ArtifactBody::Permission { .. } => "permission",
                ArtifactBody::Markdown(_) => "markdown",
                ArtifactBody::Question { .. } => "question",
            })
            .collect();
        assert_eq!(kinds, ["table", "kpis", "permission", "markdown"]);
        assert_eq!(conversation.active_artifact().map(|a| a.title.as_str()), Some("Activité du mois"));
        let mut seen: Vec<&str> = conversation.artifacts.iter().map(|a| a.id.as_str()).chain(conversation.messages.iter().map(|m| m.id.as_str())).collect();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), 12, "every message and artifact has its own identifier");

        // A message typed by hand gets the stub answer and leaves the canvas alone.
        demo_exchange(&mut conversation, "Bonjour", STUB_REPLY, None, &mut ids, NOW);
        assert_eq!(conversation.artifacts.len(), 4);
        assert_eq!(conversation.title, "Trouve le client Dupont");
    }

    #[test]
    fn a_permission_is_answered_once() {
        let mut ids = Ids::default();
        let mut conversation = empty("demo".into(), NOW);
        let (reply, artifact) = demo_turn("devis", &mut ids);
        let id = artifact.id.clone();
        demo_exchange(&mut conversation, "Prépare un devis", reply, Some(artifact), &mut ids, NOW);
        let status = |conversation: &Conversation| match &conversation.artifacts[0].body {
            ArtifactBody::Permission { status, .. } => *status,
            other => panic!("not a permission: {other:?}"),
        };
        assert_eq!(artifact_meta(&conversation.artifacts[0]), "En attente");

        resolve_permission(&mut conversation, "another", true, &mut ids, NOW);
        assert_eq!((status(&conversation), conversation.messages.len()), (Permission::Pending, 2));

        resolve_permission(&mut conversation, &id, false, &mut ids, NOW);
        assert_eq!(status(&conversation), Permission::Denied);
        assert_eq!(conversation.messages.last().unwrap().content, "Autorisation refusée. Rien n'a été modifié.");

        // Answered: a second click changes nothing.
        resolve_permission(&mut conversation, &id, true, &mut ids, NOW);
        assert_eq!((status(&conversation), conversation.messages.len()), (Permission::Denied, 3));
    }

    #[test]
    fn the_list_is_narrowed_by_title_whatever_the_case() {
        let row = |title: &str| ConversationSummary { id: title.into(), title: title.into(), updated_at: NOW.into() };
        let rows = [row("Révision des 600 km"), row("Stock bas"), row("Devis révision")];
        assert_eq!(matching(&rows, "  ").len(), 3);
        assert_eq!(matching(&rows, "RÉVISION").iter().map(|r| r.title.as_str()).collect::<Vec<_>>(), ["Révision des 600 km", "Devis révision"]);
        assert!(matching(&rows, "pneu").is_empty());
    }

    #[test]
    fn how_long_ago_reads_in_words() {
        let now = Utc.with_ymd_and_hms(2026, 11, 26, 12, 0, 0).unwrap();
        let ago = |iso: &str| relative(iso, now);
        assert_eq!(ago("2026-11-26T11:59:40Z"), "à l'instant");
        assert_eq!(ago("2026-11-26T11:59:00+00:00"), "il y a 1 min");
        assert_eq!(ago("2026-11-26T13:15:00+02:00"), "il y a 45 min");
        assert_eq!(ago("2026-11-26T09:00:00Z"), "il y a 3 h");
        assert_eq!(ago("2026-11-25T10:00:00Z"), "hier");
        // A clock a little ahead on the PC that wrote it is not « in the future ».
        assert_eq!(ago("2026-11-26T12:00:30Z"), "à l'instant");
        assert_eq!(ago("pas une date"), "");
        // Older: the day, as read on this PC. Noon keeps it the same day in every usual time zone.
        assert_eq!(ago("2026-08-03T12:00:00Z"), "3 août");
        assert_eq!(ago("2026-02-14T12:00:00Z"), "14 févr.");
    }
}
