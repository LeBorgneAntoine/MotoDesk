//! Importing a CSV file, step by step (port of `routes/ImportData.svelte` and
//! `routes/import/*`): the file, where it goes, which column fills which field, a look at
//! the rows before anything is written, then the outcome.
//!
//! The rules (what a field is, how a value is read) are in `motodesk_client::import`; this
//! is the screen around them. The file is read and its rows prepared off the interface's
//! thread, so a file of tens of thousands of rows does not freeze the window.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

use motodesk_client::import::csv::{self, ParsedCsv};
use motodesk_client::import::schema::{ENTITIES, Entity, FieldType, Mode};
use motodesk_client::import::transforms::{self, CaseMode, Mapping, RowResult, Source};
use motodesk_client::import::{self, BATCH_SIZE, ImportRequest, ImportResult};
use motodesk_ui::gpui::prelude::FluentBuilder as _;
use motodesk_ui::gpui::*;
use motodesk_ui::*;
use serde_json::{Map, Value};

use crate::backend::{ApiExt as _, PickExt as _, SaveExt as _};
use crate::format::{self, humanize_error};

/// The two choices of a field's source that are not a column of the file.
const IGNORE: &str = "\u{0}ignore";
const CONSTANT: &str = "\u{0}constant";

/// How many rows the look-before-importing shows.
const PREVIEW_LIMIT: usize = 50;
/// How many refused rows the outcome lists before pointing at the report.
const ERRORS_SHOWN: usize = 12;
/// The largest file read. Far above any export of a shop; a guard against the wrong file.
const MAX_FILE_BYTES: usize = 64 * 1024 * 1024;

const STEPS: [(&str, &str); 5] = [
    ("Fichier", "CSV source"),
    ("Destination", "Table et mode"),
    ("Correspondance", "Colonnes → champs"),
    ("Aperçu", "Contrôle avant import"),
    ("Résultat", "Bilan"),
];
const PREVIEW: usize = 3;
const RESULT: usize = 4;

/// The separator of a file, in words when it cannot be seen.
fn separator_name(delimiter: char) -> String {
    match delimiter {
        '\t' => "tabulation".into(),
        other => format!("« {other} »"),
    }
}

/// What the wizard says about a file that was read.
fn file_summary(parsed: &ParsedCsv) -> String {
    format!(
        "{} ligne(s) · {} colonne(s) · séparateur {}",
        format::integer(parsed.rows.len() as i64),
        parsed.headers.len(),
        separator_name(parsed.delimiter)
    )
}

fn read_csv(bytes: &[u8]) -> Result<ParsedCsv, String> {
    csv::parse(&csv::decode(bytes))
}

/// The rows of the file through the mapping, and what the wizard shows about them.
struct Prepared {
    results: Vec<RowResult>,
    /// The rows that share the match key with another row of the file.
    duplicates: BTreeSet<usize>,
    errors: usize,
    /// The fields that have a value in at least one row: the columns worth showing.
    shown_fields: Vec<usize>,
}

impl Prepared {
    fn new(parsed: &ParsedCsv, mappings: &[Mapping], match_key: Option<&str>) -> Self {
        let results = transforms::transform(parsed, mappings);
        let duplicates = match_key.map(|key| transforms::find_duplicate_rows(&results, key)).unwrap_or_default();
        let errors = results.iter().filter(|row| row.has_error()).count();
        let shown_fields =
            (0..mappings.len()).filter(|&field| results.iter().any(|row| row.cells.get(field).is_some_and(|cell| !cell.display.is_empty()))).collect();
        Self { results, duplicates, errors, shown_fields }
    }

    fn valid(&self) -> usize {
        self.results.len() - self.errors
    }
}

/// Rows on their way to the service, a batch at a time.
struct Run {
    /// Written, or only looked at.
    commit: bool,
    /// The request without its rows.
    request: ImportRequest,
    rows: Vec<Map<String, Value>>,
    done: usize,
    whole: ImportResult,
}

/// The controls of one field in the mapping step.
struct FieldControls {
    source: SelectModel,
    constant: TextModel,
    /// How a reference is looked up, or how a text is cased; nothing for the other types.
    detail: Option<SelectModel>,
    default_value: TextModel,
    _subscriptions: Vec<Subscription>,
}

pub struct ImportWizard {
    step: usize,
    file_name: Option<String>,
    reading: bool,
    read_error: Option<String>,
    parsed: Option<Arc<ParsedCsv>>,

    entity: &'static Entity,
    mode: Mode,
    match_key: &'static str,
    ignore_duplicates: bool,
    create_missing_categories: bool,
    entity_select: SelectModel,
    mode_select: SelectModel,
    /// Rebuilt with the entity: its choices are the entity's own keys.
    match_select: Option<SelectModel>,
    _match_subscription: Option<Subscription>,

    mappings: Vec<Mapping>,
    controls: Vec<FieldControls>,

    prepared: Option<Arc<Prepared>>,
    preparing: bool,
    /// Bumped whenever what the rows are made from changes: work done for an older state is
    /// dropped when it comes back.
    generation: u64,
    /// For each row sent, its position among the rows of the file.
    index_map: Vec<usize>,
    run: Option<Run>,
    server_preview: Option<ImportResult>,
    result: Option<ImportResult>,
    action_error: Option<String>,
    _subscriptions: Vec<Subscription>,
}

impl ImportWizard {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let entity = &ENTITIES[0];
        let entity_select = SelectModel::new(ENTITIES.iter().map(|e| SelectOption::new(e.id, e.label)).collect(), Some(entity.id), window, cx);
        let mode_select =
            SelectModel::new(Mode::ALL.into_iter().map(|mode| SelectOption::new(mode.id(), mode.label())).collect(), Some(Mode::Insert.id()), window, cx);
        let subscriptions = vec![
            entity_select.on_change(window, cx, |this: &mut Self, chosen, window, cx| {
                if let Some(entity) = chosen.as_deref().and_then(|id| ENTITIES.iter().find(|e| e.id == id)) {
                    this.set_entity(entity, window, cx);
                }
            }),
            mode_select.on_change(window, cx, |this: &mut Self, chosen, _, cx| {
                if let Some(mode) = chosen.as_deref().and_then(Mode::from_id) {
                    this.set_mode(mode, cx);
                }
            }),
        ];
        let mut wizard = Self {
            step: 0,
            file_name: None,
            reading: false,
            read_error: None,
            parsed: None,
            entity,
            mode: Mode::Insert,
            match_key: entity.match_keys.first().copied().unwrap_or_default(),
            ignore_duplicates: false,
            create_missing_categories: true,
            entity_select,
            mode_select,
            match_select: None,
            _match_subscription: None,
            mappings: Vec::new(),
            controls: Vec::new(),
            prepared: None,
            preparing: false,
            generation: 0,
            index_map: Vec::new(),
            run: None,
            server_preview: None,
            result: None,
            action_error: None,
            _subscriptions: subscriptions,
        };
        wizard.rebuild_mappings(window, cx);
        wizard
    }

    // ---- the file ------------------------------------------------------------------------------

    fn pick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.reading || self.run.is_some() {
            return;
        }
        cx.pick_file(window, "Choisir un fichier CSV", MAX_FILE_BYTES, |this, picked, window, cx| match picked {
            Ok(Some(file)) if file.size > MAX_FILE_BYTES => {
                this.read_error = Some(format!("Ce fichier fait {} : c'est trop pour un import.", format::bytes(file.size as i64)));
                cx.notify();
            }
            Ok(Some(file)) => this.read(file.name, Ok(file.bytes), window, cx),
            // The dialog was closed.
            Ok(None) => {}
            Err(error) => {
                this.read_error = Some(error.to_string());
                cx.notify();
            }
        });
    }

    /// A file dropped on the window, or named by the development switch.
    fn read_path(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        let too_large = std::fs::metadata(&path).is_ok_and(|file| file.len() > MAX_FILE_BYTES as u64);
        if too_large {
            self.read_error = Some("Ce fichier est trop volumineux pour un import.".into());
            cx.notify();
            return;
        }
        self.read(name, Err(path), window, cx);
    }

    /// Read a file: its bytes when they are already here, its path otherwise. Off the
    /// interface's thread, then the wizard starts again from this file.
    fn read(&mut self, name: String, source: Result<Vec<u8>, PathBuf>, window: &mut Window, cx: &mut Context<Self>) {
        self.reading = true;
        self.read_error = None;
        self.file_name = Some(name);
        self.parsed = None;
        self.invalidate();
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let parsed = cx
                .background_executor()
                .spawn(async move {
                    let bytes = match source {
                        Ok(bytes) => bytes,
                        Err(path) => std::fs::read(&path).map_err(|error| error.to_string())?,
                    };
                    read_csv(&bytes)
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.reading = false;
                match parsed {
                    Ok(parsed) => this.parsed = Some(Arc::new(parsed)),
                    Err(error) => this.read_error = Some(error),
                }
                this.rebuild_mappings(window, cx);
                cx.notify();
            });
        })
        .detach();
    }

    // ---- the destination -----------------------------------------------------------------------

    fn set_entity(&mut self, entity: &'static Entity, window: &mut Window, cx: &mut Context<Self>) {
        if self.entity.id == entity.id {
            return;
        }
        let had_keys = !self.entity.match_keys.is_empty();
        self.entity = entity;
        if entity.match_keys.is_empty() {
            // Rows of this kind are only ever added, each as it comes.
            self.mode = Mode::Insert;
            self.ignore_duplicates = true;
            self.mode_select.set_value(Mode::Insert.id(), window, cx);
        } else if !had_keys {
            self.ignore_duplicates = false;
        }
        self.rebuild_mappings(window, cx);
        cx.notify();
    }

    fn set_mode(&mut self, mode: Mode, cx: &mut Context<Self>) {
        self.mode = mode;
        // Adding blindly only makes sense when adding.
        if mode != Mode::Insert {
            self.ignore_duplicates = false;
        }
        self.invalidate();
        cx.notify();
    }

    fn set_ignore_duplicates(&mut self, ignore: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.ignore_duplicates = ignore;
        if ignore {
            self.mode = Mode::Insert;
            self.mode_select.set_value(Mode::Insert.id(), window, cx);
        }
        self.invalidate();
        cx.notify();
    }

    /// What was prepared or checked no longer stands.
    fn invalidate(&mut self) {
        self.generation += 1;
        self.prepared = None;
        self.server_preview = None;
        self.action_error = None;
    }

    // ---- the mapping ---------------------------------------------------------------------------

    /// Start the mapping again from the headers of the file, for the entity chosen.
    fn rebuild_mappings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let headers = self.parsed.as_ref().map(|parsed| parsed.headers.clone()).unwrap_or_default();
        self.mappings = transforms::suggest_mappings(self.entity, &headers);
        if !self.entity.match_keys.contains(&self.match_key) {
            self.match_key = self.entity.match_keys.first().copied().unwrap_or_default();
        }
        self.match_select = (!self.entity.match_keys.is_empty()).then(|| {
            let options = self.entity.match_keys.iter().map(|key| SelectOption::new(*key, self.entity.label_of(key))).collect();
            SelectModel::new(options, Some(self.match_key), window, cx)
        });
        let entity = self.entity;
        self._match_subscription = self.match_select.as_ref().map(|select| {
            select.on_change(window, cx, move |this: &mut Self, chosen, _, cx| {
                if let Some(key) = chosen.and_then(|key| entity.match_keys.iter().find(|k| **k == key)) {
                    this.match_key = key;
                    this.invalidate();
                    cx.notify();
                }
            })
        });
        self.controls = (0..self.mappings.len()).map(|field| self.field_controls(field, &headers, window, cx)).collect();
        self.invalidate();
    }

    fn field_controls(&self, index: usize, headers: &[String], window: &mut Window, cx: &mut Context<Self>) -> FieldControls {
        let mapping = &self.mappings[index];
        let field = mapping.field;
        let mut options = vec![SelectOption::new(IGNORE, "— Ignorer —"), SelectOption::new(CONSTANT, "Valeur constante…")];
        options.extend(headers.iter().map(|header| SelectOption::new(header.clone(), header.clone())));
        let chosen = match &mapping.source {
            Source::Ignore => IGNORE,
            Source::Constant => CONSTANT,
            Source::Column(header) => header.as_str(),
        };
        let source = SelectModel::new(options, Some(chosen), window, cx);
        let constant = TextModel::builder().placeholder("Valeur appliquée à toutes les lignes").build(window, cx);
        let default_value = TextModel::builder().placeholder("Défaut si vide").build(window, cx);
        let detail = match field.kind {
            FieldType::Reference(target) => {
                let options = target.lookups().iter().map(|(key, label)| SelectOption::new(*key, *label)).collect();
                Some(SelectModel::new(options, Some(mapping.ref_lookup), window, cx))
            }
            FieldType::Text => {
                let options = CaseMode::ALL.into_iter().map(|case| SelectOption::new(case.id(), case.label())).collect();
                Some(SelectModel::new(options, Some(mapping.case.id()), window, cx))
            }
            _ => None,
        };

        let mut subscriptions = vec![
            source.on_change(window, cx, move |this: &mut Self, chosen, _, cx| {
                let Some(chosen) = chosen else { return };
                this.change(index, cx, |mapping| {
                    mapping.source = match chosen.as_str() {
                        IGNORE => Source::Ignore,
                        CONSTANT => Source::Constant,
                        _ => Source::Column(chosen),
                    }
                });
            }),
            constant.on_event(window, cx, move |this: &mut Self, event, _, cx| {
                if matches!(event, TextEvent::Changed) {
                    let text = this.controls[index].constant.text(cx);
                    this.change(index, cx, |mapping| mapping.constant = text);
                }
            }),
            default_value.on_event(window, cx, move |this: &mut Self, event, _, cx| {
                if matches!(event, TextEvent::Changed) {
                    let text = this.controls[index].default_value.text(cx);
                    this.change(index, cx, |mapping| mapping.default_value = text);
                }
            }),
        ];
        if let Some(select) = &detail {
            subscriptions.push(select.on_change(window, cx, move |this: &mut Self, chosen, _, cx| {
                let Some(chosen) = chosen else { return };
                this.change(index, cx, |mapping| match mapping.field.kind {
                    FieldType::Reference(target) => {
                        if let Some((key, _)) = target.lookups().iter().find(|(key, _)| *key == chosen) {
                            mapping.ref_lookup = key;
                        }
                    }
                    _ => mapping.case = CaseMode::from_id(&chosen),
                });
            }));
        }
        FieldControls { source, constant, detail, default_value, _subscriptions: subscriptions }
    }

    /// Change how one field is filled.
    fn change(&mut self, index: usize, cx: &mut Context<Self>, edit: impl FnOnce(&mut Mapping)) {
        if let Some(mapping) = self.mappings.get_mut(index) {
            edit(mapping);
            self.invalidate();
            cx.notify();
        }
    }

    // ---- moving through the steps --------------------------------------------------------------

    fn can_next(&self) -> bool {
        match self.step {
            0 => self.parsed.is_some() && !self.reading,
            1 => self.ignore_duplicates || self.entity.match_keys.is_empty() || self.entity.match_keys.contains(&self.match_key),
            2 => transforms::unmapped_required(&self.mappings).is_empty(),
            _ => false,
        }
    }

    fn go_to(&mut self, step: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.run.is_some() || self.step == RESULT || step > PREVIEW {
            return;
        }
        self.step = step;
        if step == PREVIEW && self.prepared.is_none() {
            self.prepare(window, cx);
        }
        cx.notify();
    }

    fn next(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.can_next() {
            self.go_to(self.step + 1, window, cx);
        }
    }

    fn previous(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.step > 0 {
            self.go_to(self.step - 1, window, cx);
        }
    }

    /// Put every row of the file through the mapping, off the interface's thread.
    fn prepare(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(parsed) = self.parsed.clone() else { return };
        let mappings = self.mappings.clone();
        let match_key = (!self.ignore_duplicates && !self.match_key.is_empty()).then_some(self.match_key);
        let generation = self.generation;
        self.preparing = true;
        cx.spawn_in(window, async move |this, cx| {
            let prepared = cx.background_executor().spawn(async move { Prepared::new(&parsed, &mappings, match_key) }).await;
            let _ = this.update(cx, |this, cx| {
                // The mapping changed meanwhile: this answer is for another question.
                if this.generation == generation {
                    this.prepared = Some(Arc::new(prepared));
                    this.preparing = false;
                }
                cx.notify();
            });
        })
        .detach();
    }

    // ---- sending -------------------------------------------------------------------------------

    /// Send the rows without an error: to be looked at (`commit` false) or written.
    fn start(&mut self, commit: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(prepared) = self.prepared.clone() else { return };
        if self.run.is_some() || prepared.valid() == 0 {
            return;
        }
        let (rows, index_map) = transforms::sendable(&prepared.results);
        self.index_map = index_map;
        self.action_error = None;
        self.run = Some(Run {
            commit,
            request: ImportRequest {
                entity: self.entity.id,
                mode: self.mode,
                match_key: self.match_key.to_string(),
                ignore_duplicates: self.ignore_duplicates,
                create_missing_categories: self.create_missing_categories,
                references: transforms::references(&self.mappings),
                rows: Vec::new(),
            },
            rows,
            done: 0,
            whole: ImportResult::default(),
        });
        self.send_batch(window, cx);
        cx.notify();
    }

    fn send_batch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(run) = &self.run else { return };
        let offset = run.done;
        let end = (offset + BATCH_SIZE).min(run.rows.len());
        let body = run.request.with_rows(&run.rows[offset..end]);
        let commit = run.commit;
        cx.api_in(
            window,
            move |client| async move { if commit { import::commit(&client, &body).await } else { import::preview(&client, &body).await } },
            move |this, answer, window, cx| {
                let Some(run) = &mut this.run else { return };
                match answer {
                    Ok(batch) => {
                        run.whole.absorb(batch, offset);
                        run.done = end;
                        if end < run.rows.len() {
                            this.send_batch(window, cx);
                        } else if let Some(run) = this.run.take() {
                            if run.commit {
                                this.result = Some(run.whole);
                                this.step = RESULT;
                            } else {
                                this.server_preview = Some(run.whole);
                            }
                        }
                    }
                    Err(error) => {
                        // What was written before the failure stays written: say how far it got.
                        let so_far = if run.commit && offset > 0 {
                            format!(" Les {} premières lignes ont été traitées.", format::integer(offset as i64))
                        } else {
                            String::new()
                        };
                        this.action_error = Some(format!("{}{so_far}", humanize_error(&error, "L'import a échoué.")));
                        this.run = None;
                    }
                }
                cx.notify();
            },
        );
    }

    /// The rows that did not go in, as a file to correct and import again.
    fn error_report(&self) -> Option<String> {
        let (parsed, prepared) = (self.parsed.as_ref()?, self.prepared.as_ref()?);
        transforms::error_report(self.entity, parsed, &prepared.results, self.result.as_ref(), &self.index_map)
    }

    fn save_errors(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(report) = self.error_report() else { return };
        let name = format!("erreurs_import_{}.csv", self.entity.id);
        cx.save_file(window, &name, report.into_bytes(), |_, outcome, window, cx| match outcome {
            Ok(Some(path)) => toast::success(format!("Rapport enregistré : {}", path.display()), window, cx),
            Ok(None) => {}
            Err(error) => toast::error(format!("Impossible d'enregistrer le rapport : {error}"), window, cx),
        });
    }

    fn reset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.step = 0;
        self.file_name = None;
        self.parsed = None;
        self.read_error = None;
        self.result = None;
        self.index_map.clear();
        self.ignore_duplicates = self.entity.match_keys.is_empty();
        self.rebuild_mappings(window, cx);
        cx.notify();
    }

    // ---- drawing: the steps --------------------------------------------------------------------

    fn muted(text: impl Into<SharedString>, cx: &App) -> Div {
        let theme = cx.ui();
        div().fs(theme.m.fs_sm).text_color(theme.colors.text_muted).child(text.into())
    }

    fn source_step(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.ui().clone();
        let (c, m) = (theme.colors, theme.m);
        let pick = |id: &'static str, label: &'static str, primary: bool, cx: &mut Context<Self>| {
            let button = Button::new(id).icon(IconName::Upload).label(label).on_click(cx.listener(|this, _, window, cx| {
                // The area around the button opens the same dialog: once is enough.
                cx.stop_propagation();
                this.pick(window, cx);
            }));
            if primary { button.primary() } else { button }
        };
        let zone = match (&self.file_name, &self.parsed) {
            _ if self.reading => DropZone::new("import-drop", "Lecture du fichier…").busy(true),
            (Some(name), parsed) => DropZone::new("import-drop", name.clone())
                .icon(IconName::FileText)
                .lines(parsed.as_deref().map(file_summary))
                .line("Glissez ou choisissez un autre fichier pour le remplacer.")
                .action(pick("import-replace", "Remplacer le fichier", false, cx)),
            (None, _) => DropZone::new("import-drop", "Glissez-déposez un fichier CSV ici")
                .line("Formats acceptés : .csv · séparateur détecté automatiquement")
                .action(pick("import-pick", "Choisir un fichier", true, cx)),
        };
        let wizard = cx.entity();
        let zone = zone.on_click(cx.listener(|this, _, window, cx| this.pick(window, cx))).on_paths(move |paths, window, cx| {
            if let Some(path) = paths.first().cloned() {
                wizard.update(cx, |this, cx| this.read_path(path, window, cx));
            }
        });

        let headers = self.parsed.as_ref().filter(|parsed| !parsed.headers.is_empty()).map(|parsed| {
            row()
                .flex_wrap()
                .gap(m.space_2)
                .child(div().fs(m.fs_sm).text_color(c.text_muted).child("Colonnes détectées :"))
                .children(parsed.headers.iter().map(|header| Tag::new(header.clone()).status(Status::Muted)))
        });
        col()
            .gap(m.space_4)
            .child(zone)
            .children(self.read_error.clone().map(|error| Alert::danger().title("Impossible de lire le fichier").child(error)))
            .children(headers)
    }

    fn target_step(&self, cx: &mut Context<Self>) -> Div {
        let m = cx.ui().m;
        let wizard = cx.entity();
        let has_keys = !self.entity.match_keys.is_empty();
        let adding = self.mode == Mode::Insert;
        let mut step = col()
            .max_w(px(560.))
            .gap(m.space_4)
            .child(FormField::new("Type de données à importer").child(Select::new(&self.entity_select)))
            .child(Self::muted(self.entity.description, cx));

        if !has_keys {
            step = step.child(Alert::info().child(
                "Chaque ligne est ajoutée à la base — il n'y a pas de clé de correspondance pour ce type de données. Reliez les colonnes aux autres tables à l'étape suivante.",
            ));
        } else {
            let ignore_hint = if self.ignore_duplicates {
                "Import aveugle : chaque ligne est ajoutée sans vérifier la base. Les conflits d'unicité (référence, e-mail, etc.) apparaîtront comme erreurs à l'import."
            } else if !adding {
                "Disponible uniquement en mode « Ajouter uniquement »."
            } else {
                "Aucune détection par rapport aux données déjà enregistrées — vous n'avez pas à choisir une colonne de correspondance."
            };
            let toggle = wizard.clone();
            step = step.child(FormField::new("Mode d'import").hint(self.mode.hint()).child(Select::new(&self.mode_select))).child(
                Checkbox::new("import-ignore-duplicates", self.ignore_duplicates)
                    .label("Ignorer les doublons")
                    .hint(ignore_hint)
                    .disabled(!adding)
                    .on_change(move |on, window, cx| toggle.update(cx, |this, cx| this.set_ignore_duplicates(on, window, cx))),
            );
            if let Some(select) = self.match_select.as_ref().filter(|_| !self.ignore_duplicates) {
                let (label, hint) = if adding {
                    ("Détection des doublons", "Les lignes dont cette valeur existe déjà seront ignorées.")
                } else {
                    ("Clé de correspondance", "Champ utilisé pour retrouver les lignes existantes.")
                };
                step = step.child(FormField::new(label).hint(hint).child(Select::new(select)));
            }
        }
        if self.entity.id == "products" {
            step = step.child(
                Checkbox::new("import-create-categories", self.create_missing_categories)
                    .label("Créer automatiquement les catégories manquantes")
                    .hint("Sinon, les lignes dont la catégorie est inconnue seront signalées en erreur.")
                    .on_change(move |on, _, cx| {
                        wizard.update(cx, |this, cx| {
                            this.create_missing_categories = on;
                            cx.notify();
                        })
                    }),
            );
        }
        step
    }

    fn mapping_step(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.ui().clone();
        let (c, m) = (theme.colors, theme.m);
        let wizard = cx.entity();
        let heading = |text: &'static str| div().fs_tight(m.fs_xs).font_weight(FontWeight::SEMIBOLD).text_color(c.text_muted).child(text);
        let head = row()
            .items_start()
            .gap(m.space_4)
            .pb(m.space_2)
            .border_b_1()
            .border_color(c.border)
            .child(div().w(relative(0.28)).flex_none().child(heading("CHAMP CIBLE")))
            .child(div().w(relative(0.26)).flex_none().child(heading("COLONNE SOURCE")))
            .child(div().flex_1().min_w_0().child(heading("MISE EN FORME")));

        let lines = self.mappings.iter().zip(&self.controls).enumerate().map(|(index, (mapping, controls))| {
            let field = mapping.field;
            let missing = field.required && mapping.is_ignored();
            let mut kind = field.kind.label().to_string();
            if let Some(target) = field.reference() {
                kind.push_str(&format!(" → {}", target.table()));
            }
            if field.unique {
                kind.push_str(" · unique");
            }
            let target = col()
                .w(relative(0.28))
                .flex_none()
                .gap(px(2.))
                .child(
                    row()
                        .gap(m.space_1)
                        .fs(m.fs_sm)
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(c.text_strong)
                        .child(field.label)
                        .when(field.required, |this| this.child(div().text_color(c.danger).child("*"))),
                )
                .child(div().fs(m.fs_xs).text_color(c.text_subtle).child(kind))
                .when(!field.hint.is_empty(), |this| this.child(div().fs(m.fs_xs).text_color(c.text_muted).child(field.hint)));

            let source = col()
                .w(relative(0.26))
                .flex_none()
                .gap(m.space_2)
                .child(Select::new(&controls.source).menu_width(px(320.)))
                .when(mapping.source == Source::Constant, |this| this.child(Input::new(&controls.constant)))
                .when(missing, |this| {
                    this.child(
                        row()
                            .gap(m.space_1)
                            .fs(m.fs_xs)
                            .text_color(c.danger)
                            .child(Icon::new(IconName::TriangleAlert).sm())
                            .child("Champ requis non associé"),
                    )
                });

            let options: Div = if mapping.is_ignored() {
                div().flex_1().min_w_0().fs(m.fs_sm).text_color(c.text_subtle).child("Non importé")
            } else {
                let (trim, empty) = (wizard.clone(), wizard.clone());
                let detail = controls.detail.as_ref().map(|select| {
                    let label = if field.reference().is_some() { "Connecté par" } else { "Casse" };
                    row()
                        .gap(m.space_2)
                        .child(div().flex_none().fs(m.fs_xs).text_color(c.text_muted).child(label))
                        .child(div().w(px(190.)).flex_none().child(Select::new(select)))
                });
                row()
                    .flex_1()
                    .min_w_0()
                    .flex_wrap()
                    .gap(m.space_3)
                    .children(detail)
                    .child(Checkbox::new(("import-trim", index), mapping.trim).label("Espaces").on_change(move |on, _, cx| {
                        trim.update(cx, |this, cx| this.change(index, cx, |mapping| mapping.trim = on))
                    }))
                    .when(!field.required, |this| {
                        this.child(Checkbox::new(("import-empty", index), mapping.empty_to_null).label("Vide → nul").on_change(
                            move |on, _, cx| empty.update(cx, |this, cx| this.change(index, cx, |mapping| mapping.empty_to_null = on)),
                        ))
                    })
                    .child(div().w(px(170.)).flex_none().child(Input::new(&controls.default_value)))
            };

            row().items_start().gap(m.space_4).py(m.space_3).border_b_1().border_color(c.border_subtle).child(target).child(source).child(options)
        });
        col().child(head).children(lines)
    }

    fn preview_step(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.ui().clone();
        let (c, m) = (theme.colors, theme.m);
        let Some(prepared) = self.prepared.clone().filter(|_| !self.preparing) else {
            return div().size_full().child(StateView::loading().title("Préparation des lignes…"));
        };
        let total = prepared.results.len();
        let key_label = self.entity.label_of(self.match_key).to_string();
        let duplicates = prepared.duplicates.len();
        let busy = self.run.is_some();
        let looking = self.run.as_ref().filter(|run| !run.commit);

        let mut stats = row()
            .flex_none()
            .flex_wrap()
            .gap(m.space_2)
            .child(Tag::new(format!("{} ligne(s)", format::integer(total as i64))))
            .child(Tag::new(format!("{} valide(s)", format::integer(prepared.valid() as i64))).status(Status::Success));
        if prepared.errors > 0 {
            stats = stats.child(Tag::new(format!("{} en erreur", format::integer(prepared.errors as i64))).status(Status::Danger));
        }
        if self.ignore_duplicates {
            stats = stats.child(Tag::new("Doublons ignorés").status(Status::Info));
        } else if duplicates > 0 {
            stats = stats.child(Tag::new(format!("{duplicates} doublon(s) « {key_label} »")).status(Status::Warning));
        }

        let mut notes: Vec<Alert> = Vec::new();
        if self.ignore_duplicates {
            notes.push(Alert::info().child(
                "Import aveugle : aucune vérification par rapport à la base. Les lignes en double dans le fichier ou déjà présentes (référence unique, etc.) peuvent échouer à l'import.",
            ));
        }
        if prepared.errors > 0 {
            notes.push(
                Alert::warning().child("Les lignes en erreur (champ requis manquant ou valeur invalide) seront ignorées lors de l'import."),
            );
        }
        if duplicates > 0 {
            notes.push(Alert::info().child(format!(
                "Plusieurs lignes du fichier partagent la même valeur « {key_label} ». En mode « Ajouter ou mettre à jour », seule la dernière l'emportera."
            )));
        }

        // The first rows, with only the columns that carry something.
        let mut columns = vec![Column::new("").fixed(px(44.)), Column::new("#").fixed(px(60.)).right()];
        for &field in &prepared.shown_fields {
            let field = &self.entity.fields[field];
            let column = Column::new(field.label).flex(1.);
            columns.push(if field.kind == FieldType::Money || field.key == "sku" { column.mono() } else { column });
        }
        let shown = total.min(PREVIEW_LIMIT);
        let rows = prepared.clone();
        let table = DataTable::new("import-preview", columns, shown, move |ix, column, _, cx| {
            let c = cx.ui().colors;
            let Some(result) = rows.results.get(ix) else { return div().into_any_element() };
            match column {
                0 if result.has_error() => div()
                    .id(("import-row-error", ix))
                    .text_color(c.danger)
                    .child(Icon::new(IconName::TriangleAlert).sm())
                    .tooltip(tooltip(result.errors.join(" · ")))
                    .into_any_element(),
                0 if rows.duplicates.contains(&ix) => div()
                    .id(("import-row-duplicate", ix))
                    .text_color(c.warning)
                    .child(Icon::new(IconName::CircleAlert).sm())
                    .tooltip(tooltip("Doublon dans le fichier"))
                    .into_any_element(),
                0 => div().text_color(c.success).child(Icon::new(IconName::Check).sm()).into_any_element(),
                1 => (ix + 1).to_string().into_any_element(),
                _ => {
                    let Some(cell) = rows.shown_fields.get(column - 2).and_then(|&field| result.cells.get(field)) else {
                        return div().into_any_element();
                    };
                    match &cell.error {
                        Some(error) => div()
                            .id(("import-cell-error", ix * 64 + column))
                            .text_color(c.danger)
                            .child(if cell.display.is_empty() { "—".to_string() } else { cell.display.clone() })
                            .tooltip(tooltip(error.clone()))
                            .into_any_element(),
                        None => cell.display.clone().into_any_element(),
                    }
                }
            }
        })
        .compact();

        let checked = self.server_preview.as_ref().map(|preview| {
            let mut tags = row()
                .flex_wrap()
                .gap(m.space_2)
                .child(Tag::new(format!("{} à créer", preview.inserted)).status(Status::Success))
                .child(Tag::new(format!("{} à mettre à jour", preview.updated)).status(Status::Info))
                .child(Tag::new(format!("{} ignorée(s)", preview.skipped)).status(Status::Muted));
            if preview.errored > 0 {
                tags = tags.child(Tag::new(format!("{} refusée(s)", preview.errored)).status(Status::Danger));
            }
            tags
        });

        col()
            .size_full()
            .gap(m.space_3)
            .child(stats)
            .children(notes)
            .child(div().flex_1().min_h(px(160.)).border_1().border_color(c.border).child(table))
            .when(total > PREVIEW_LIMIT, |this| {
                this.child(Self::muted(
                    format!("Aperçu des {PREVIEW_LIMIT} premières lignes — les {} lignes seront importées.", format::integer(total as i64)),
                    cx,
                ))
            })
            .child(
                row()
                    .flex_none()
                    .flex_wrap()
                    .gap(m.space_3)
                    .child(
                        Button::new("import-check")
                            .label("Vérifier la correspondance")
                            .loading(looking.is_some())
                            .disabled(prepared.valid() == 0 || busy)
                            .on_click(cx.listener(|this, _, window, cx| this.start(false, window, cx))),
                    )
                    .children(looking.map(|run| Self::muted(format!("{} / {}", run.done, run.rows.len()), cx)))
                    .children(checked),
            )
            .children(self.action_error.clone().map(|error| Alert::danger().title("Erreur").child(error)))
    }

    fn result_step(&self, result: &ImportResult, cx: &mut Context<Self>) -> Div {
        let theme = cx.ui().clone();
        let (c, m) = (theme.colors, theme.m);
        let refused: Vec<_> = result.errors().collect();
        let mut counts = row()
            .flex_wrap()
            .justify_center()
            .gap(m.space_2)
            .child(Tag::new(format!("{} créée(s)", result.inserted)).status(Status::Success).medium())
            .child(Tag::new(format!("{} mise(s) à jour", result.updated)).status(Status::Info).medium())
            .child(Tag::new(format!("{} ignorée(s)", result.skipped)).status(Status::Muted).medium());
        if result.errored > 0 {
            counts = counts.child(Tag::new(format!("{} en erreur", result.errored)).status(Status::Danger).medium());
        }
        let left_out = self.prepared.as_ref().map_or(0, |prepared| prepared.errors);

        let listed = (!refused.is_empty()).then(|| {
            let lines = refused.iter().take(ERRORS_SHOWN).map(|outcome| {
                let line = transforms::line_of(self.index_map.get(outcome.row).copied().unwrap_or_default());
                row()
                    .items_start()
                    .gap(m.space_3)
                    .py(m.space_1)
                    .fs(m.fs_sm)
                    .child(div().w(px(90.)).flex_none().font_weight(FontWeight::MEDIUM).text_color(c.text_strong).child(format!("Ligne {line}")))
                    .child(div().min_w_0().child(match outcome.message.as_deref() {
                        Some(message) => transforms::explain_refusal(self.entity, message),
                        None => "Erreur inconnue".to_string(),
                    }))
            });
            col()
                .gap(m.space_3)
                .child(Alert::warning().title("Certaines lignes n'ont pas pu être importées").child(format!(
                    "{} ligne(s) ont été refusées. Enregistrez le rapport pour les corriger.",
                    refused.len()
                )))
                .child(col().children(lines).when(refused.len() > ERRORS_SHOWN, |this| {
                    this.child(Self::muted(format!("+ {} autre(s)…", refused.len() - ERRORS_SHOWN), cx))
                }))
        });

        col()
            .gap(m.space_4)
            .child(
                col()
                    .items_center()
                    .gap(m.space_3)
                    .py(m.space_4)
                    .child(Icon::new(IconName::CircleCheck).xl().color(c.success))
                    .child(Heading::new("Import terminé").size(HeadingSize::Lg))
                    .child(counts),
            )
            .children(listed)
            .when(left_out > 0, |this| {
                this.child(Self::muted(
                    format!("{left_out} ligne(s) du fichier n'ont pas été envoyées : elles avaient une erreur dès l'aperçu. Elles sont dans le rapport."),
                    cx,
                ))
            })
            .child(
                row()
                    .justify_center()
                    .gap(m.space_3)
                    .when(self.error_report().is_some(), |this| {
                        this.child(
                            Button::new("import-save-errors")
                                .icon(IconName::FileText)
                                .label("Enregistrer les erreurs (CSV)")
                                .on_click(cx.listener(|this, _, window, cx| this.save_errors(window, cx))),
                        )
                    })
                    .child(Button::new("import-again").primary().label("Nouvel import").on_click(cx.listener(|this, _, window, cx| this.reset(window, cx)))),
            )
    }
}

impl Render for ImportWizard {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.ui().clone();
        let (c, m) = (theme.colors, theme.m);
        let busy = self.run.is_some();
        let writing = self.run.as_ref().filter(|run| run.commit);

        let stepper = STEPS
            .iter()
            .fold(Stepper::new("import-steps", self.step), |stepper, (label, hint)| stepper.step(*label, *hint))
            .on_select({
                let wizard = cx.entity();
                move |step, window, cx| wizard.update(cx, |this, cx| this.go_to(step, window, cx))
            });

        // The preview fills the card and scrolls its own table; the other steps scroll whole.
        let body = match self.step {
            PREVIEW => div().size_full().p(m.space_5).child(self.preview_step(cx)),
            step => {
                let content = match (step, self.result.clone()) {
                    (0, _) => self.source_step(cx),
                    (1, _) => self.target_step(cx),
                    (2, _) => self.mapping_step(cx),
                    (_, Some(result)) => self.result_step(&result, cx),
                    _ => div(),
                };
                div().size_full().child(div().id(("import-step", step)).size_full().overflow_y_scroll().child(div().p(m.space_5).child(content)))
            }
        };

        let footer = (self.step < RESULT).then(|| {
            let valid = self.prepared.as_ref().map_or(0, |prepared| prepared.valid());
            let forward = if self.step == PREVIEW {
                Button::new("import-commit")
                    .primary()
                    .icon(IconName::Upload)
                    .label(format!("Importer {} ligne(s)", format::integer(valid as i64)))
                    .loading(writing.is_some())
                    .disabled(valid == 0 || busy || self.preparing)
                    .on_click(cx.listener(|this, _, window, cx| this.start(true, window, cx)))
            } else {
                Button::new("import-next")
                    .primary()
                    .label("Suivant")
                    .icon_after(IconName::ChevronRight)
                    .disabled(!self.can_next())
                    .on_click(cx.listener(|this, _, window, cx| this.next(window, cx)))
            };
            row()
                .flex_none()
                .justify_between()
                .gap(m.space_3)
                .child(
                    Button::new("import-previous")
                        .ghost()
                        .icon(IconName::ChevronLeft)
                        .label("Précédent")
                        .disabled(self.step == 0 || busy)
                        .on_click(cx.listener(|this, _, window, cx| this.previous(window, cx))),
                )
                .child(
                    row()
                        .gap(m.space_3)
                        .children(writing.map(|run| Self::muted(format!("{} / {}", run.done, run.rows.len()), cx)))
                        .child(forward),
                )
        });

        col()
            .size_full()
            .p(m.space_4)
            .gap(m.space_4)
            .child(div().flex_none().child(stepper))
            .child(div().flex_1().min_h_0().bg(c.bg_surface).border_1().border_color(c.border).child(body))
            .children(footer)
    }
}

#[cfg(test)]
mod tests {
    // Not `super::*`: it would bring in GPUI's own `test` attribute in place of Rust's.
    use motodesk_client::import::schema::entity;
    use motodesk_client::import::transforms::suggest_mappings;

    use super::{Prepared, file_summary, read_csv, separator_name};

    #[test]
    fn a_file_is_summed_up_by_its_rows_columns_and_separator() {
        let parsed = read_csv("Nom;Email\nMartin;lea@example.fr\nDubois;remi@example.fr\n".as_bytes()).unwrap();
        assert_eq!(file_summary(&parsed), "2 ligne(s) · 2 colonne(s) · séparateur « ; »");
        assert_eq!(separator_name('\t'), "tabulation");
        // A file saved by Excel, accents and all.
        let excel = read_csv(&[0x50, 0x72, 0xe9, 0x6e, 0x6f, 0x6d, 0x0a, 0x4c, 0xe9, 0x61]).unwrap();
        assert_eq!((excel.headers[0].as_str(), excel.rows[0][0].as_str()), ("Prénom", "Léa"));
        assert!(read_csv(b"").is_err());
    }

    #[test]
    fn what_is_prepared_counts_the_rows_and_keeps_the_columns_worth_showing() {
        let parsed = read_csv("Nom;Email;Ville\nMartin;lea@example.fr;Rennes\n;sans@example.fr;\nDubois;LEA@example.fr;\n".as_bytes()).unwrap();
        let clients = entity("clients").unwrap();
        let mappings = suggest_mappings(clients, &parsed.headers);
        let prepared = Prepared::new(&parsed, &mappings, Some("email"));
        assert_eq!((prepared.results.len(), prepared.errors, prepared.valid()), (3, 1, 2));
        assert_eq!(prepared.duplicates.iter().copied().collect::<Vec<_>>(), [0, 2]);
        // Name and e-mail have values; nothing fills the first name, the phone or the notes.
        let shown: Vec<&str> = prepared.shown_fields.iter().map(|&field| clients.fields[field].key).collect();
        assert_eq!(shown, ["last_name", "email"]);
        // Adding blindly looks for no duplicate.
        assert!(Prepared::new(&parsed, &mappings, None).duplicates.is_empty());
    }
}
