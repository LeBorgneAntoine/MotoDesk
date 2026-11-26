//! The SQL console (port of `routes/data/AdvancedTab.svelte`): one statement at a time. A
//! read is run by its preview; a write is only estimated, and runs after a confirmation.
//! The service refuses changes to the structure and writes to the application's own tables.

use std::rc::Rc;

use motodesk_client::data::{self, Row, SqlPreview};
use motodesk_ui::gpui::prelude::FluentBuilder as _;
use motodesk_ui::gpui::*;
use motodesk_ui::*;

use crate::backend::ApiExt as _;
use crate::format::{self, humanize_error};

const STARTING_SQL: &str = "SELECT * FROM clients LIMIT 10";

/// One entry of the output.
struct Line {
    /// The time on the clock of the PC.
    at: String,
    failed: bool,
    message: String,
    /// The preview of a write: what it would touch.
    write: Option<SqlPreview>,
    columns: Vec<String>,
    rows: Rc<Vec<Row>>,
}

/// The rows a write would touch, as indented JSON.
fn sample_text(rows: &[Row]) -> String {
    serde_json::to_string_pretty(rows).unwrap_or_default()
}

pub struct SqlConsole {
    sql: TextModel,
    busy: bool,
    /// The preview of what is in the editor: gone as soon as the text changes.
    last_preview: Option<SqlPreview>,
    /// The output, the newest entry first.
    lines: Vec<Line>,
    _subscriptions: Vec<Subscription>,
}

impl SqlConsole {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let sql = TextModel::builder().placeholder(STARTING_SQL).value(STARTING_SQL).multiline(7).build(window, cx);
        let subscriptions = vec![sql.on_event(window, cx, |this: &mut Self, event, _, cx| match event {
            TextEvent::Changed => {
                // What was previewed is no longer what would run.
                this.last_preview = None;
                cx.notify();
            }
            TextEvent::Submitted { with_modifier: true } => this.preview(cx),
            _ => {}
        })];
        Self { sql, busy: false, last_preview: None, lines: Vec::new(), _subscriptions: subscriptions }
    }

    fn push(&mut self, failed: bool, message: String, write: Option<SqlPreview>, columns: Option<Vec<String>>, rows: Option<Vec<Row>>) {
        let at = chrono::Local::now().format("%H:%M:%S").to_string();
        let line = Line { at, failed, message, write, columns: columns.unwrap_or_default(), rows: Rc::new(rows.unwrap_or_default()) };
        self.lines.insert(0, line);
    }

    fn can_execute(&self) -> bool {
        self.last_preview.as_ref().is_some_and(|preview| preview.token_to_execute().is_some())
    }

    fn preview(&mut self, cx: &mut Context<Self>) {
        let Some(sql) = self.sql.trimmed(cx) else { return };
        if self.busy {
            return;
        }
        self.busy = true;
        self.last_preview = None;
        cx.notify();
        cx.api(
            move |client| async move { data::preview_sql(&client, &sql).await },
            |this, result, cx| {
                this.busy = false;
                match result {
                    Ok(preview) => {
                        let write = (!preview.read_only).then(|| preview.clone());
                        this.push(false, preview.summary(), write, preview.columns.clone(), preview.rows.clone());
                        this.last_preview = Some(preview);
                    }
                    Err(error) => this.push(true, humanize_error(&error, "Erreur d'aperçu SQL."), None, None, None),
                }
                cx.notify();
            },
        );
    }

    fn ask_execute(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_execute() || self.busy {
            return;
        }
        let console = cx.entity();
        Confirm::new(
            "Exécuter la requête ?",
            "Cette action modifiera la base locale et pourra déclencher une synchronisation cloud. Continuer ?",
        )
        .confirm_label("Exécuter")
        .danger()
        .open(window, cx, move |_, cx| console.update(cx, |this, cx| this.execute(cx)));
    }

    fn execute(&mut self, cx: &mut Context<Self>) {
        let (Some(sql), Some(token)) =
            (self.sql.trimmed(cx), self.last_preview.as_ref().and_then(|preview| preview.token_to_execute()).map(str::to_string))
        else {
            return;
        };
        self.busy = true;
        cx.notify();
        cx.api(
            move |client| async move { data::execute_sql(&client, &sql, &token).await },
            |this, result, cx| {
                this.busy = false;
                match result {
                    Ok(done) => {
                        let message = format!(
                            "Exécuté — {} ligne(s) affectée(s) ({} ms)",
                            format::integer(done.rows_affected),
                            done.elapsed_ms
                        );
                        this.push(false, message, None, done.columns, done.rows);
                        // A token runs once.
                        this.last_preview = None;
                    }
                    Err(error) => this.push(true, humanize_error(&error, "Erreur d'exécution."), None, None, None),
                }
                cx.notify();
            },
        );
    }

    // ---- drawing -------------------------------------------------------------------------------

    fn panel(cx: &App) -> Div {
        let c = cx.ui().colors;
        col().bg(c.bg_surface).border_1().border_color(c.border)
    }

    fn head(title: &'static str, note: &'static str, cx: &App) -> Div {
        let theme = cx.ui();
        let (c, m) = (theme.colors, theme.m);
        row()
            .flex_none()
            .justify_between()
            .px(m.space_4)
            .py(m.space_2)
            .border_b_1()
            .border_color(c.border)
            .child(div().fs_tight(m.fs_xs).font_weight(FontWeight::SEMIBOLD).text_color(c.text_muted).child(title))
            .child(div().fs_tight(m.fs_xs).text_color(c.text_subtle).child(note))
    }

    fn editor(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.ui().clone();
        let (c, m) = (theme.colors, theme.m);
        let has_sql = self.sql.trimmed(cx).is_some();
        Self::panel(cx)
            .flex_none()
            .child(Self::head("CONSOLE SQL", "une seule instruction", cx))
            .child(div().p(m.space_3).child(Textarea::new(&self.sql).mono()))
            .child(
                row()
                    .flex_wrap()
                    .justify_between()
                    .gap(m.space_3)
                    .px(m.space_4)
                    .py(m.space_2)
                    .border_t_1()
                    .border_color(c.border)
                    .child(
                        row()
                            .gap(m.space_2)
                            .fs(m.fs_sm)
                            .text_color(c.text_muted)
                            .child(Icon::new(IconName::Info).sm())
                            .child("Structure non modifiable · tables système en lecture seule"),
                    )
                    .child(
                        row()
                            .gap(m.space_2)
                            .child(
                                Button::new("sql-clear")
                                    .ghost()
                                    .label("Effacer la sortie")
                                    .disabled(self.lines.is_empty())
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.lines.clear();
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("sql-preview")
                                    .icon(IconName::Eye)
                                    .label("Aperçu")
                                    .loading(self.busy && self.last_preview.is_none())
                                    .disabled(self.busy || !has_sql)
                                    .on_click(cx.listener(|this, _, _, cx| this.preview(cx))),
                            )
                            .child(
                                Button::new("sql-execute")
                                    .primary()
                                    .icon(IconName::Zap)
                                    .label("Exécuter")
                                    .disabled(self.busy || !self.can_execute())
                                    .on_click(cx.listener(|this, _, window, cx| this.ask_execute(window, cx))),
                            ),
                    ),
            )
    }

    /// What a write would touch, under its line of the output.
    fn impact(preview: &SqlPreview, cx: &App) -> Div {
        let theme = cx.ui();
        let (c, m) = (theme.colors, theme.m);
        let sample = preview.sample_rows.as_ref().filter(|rows| !rows.is_empty());
        col()
            .gap(m.space_2)
            .pt(m.space_2)
            .child(
                row()
                    .gap(m.space_2)
                    .child(Tag::new(preview.statement_type.clone()).status(Status::Warning))
                    .child(div().fs(m.fs_sm).child(preview.estimate())),
            )
            .children(preview.message.clone().map(|message| Alert::info().child(message)))
            .children(sample.map(|rows| {
                col()
                    .gap(m.space_1)
                    .child(div().fs(m.fs_xs).text_color(c.text_muted).child("Échantillon (5 lignes max.)"))
                    .child(Preformatted::new(sample_text(rows)))
            }))
    }

    fn output(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.ui().clone();
        let (c, m) = (theme.colors, theme.m);
        let panel = Self::panel(cx).flex_1().min_h_0().child(Self::head("SORTIE", "la plus récente en premier", cx));
        if self.lines.is_empty() {
            return panel.child(
                StateView::empty()
                    .compact()
                    .icon(IconName::Braces)
                    .title("Aucune sortie")
                    .body("Les résultats d'aperçu et d'exécution s'affichent ici."),
            );
        }
        let lines = self.lines.iter().map(|line| {
            let badge = if line.failed { Tag::new("Erreur").status(Status::Danger) } else { Tag::new("OK").status(Status::Success) };
            col()
                .px(m.space_4)
                .py(m.space_2)
                .border_b_1()
                .border_color(c.border_subtle)
                .child(
                    row()
                        .items_start()
                        .gap(m.space_3)
                        .child(div().flex_none().child(badge))
                        .child(div().flex_none().font_family(FONT_MONO).fs(m.fs_xs).text_color(c.text_subtle).child(line.at.clone()))
                        .child(div().min_w_0().fs(m.fs_sm).when(line.failed, |this| this.text_color(c.danger)).child(line.message.clone())),
                )
                .children(line.write.as_ref().map(|preview| Self::impact(preview, cx)))
        });

        // The rows of the newest entry, when it has any.
        let table = self.lines.first().filter(|line| !line.columns.is_empty() && !line.rows.is_empty()).map(|line| {
            let columns: Vec<Column> = line
                .columns
                .iter()
                .map(|name| {
                    let column = Column::new(name.clone());
                    if data::is_id_column(name) { column.mono().flex(1.6) } else { column }
                })
                .collect();
            let (names, rows) = (line.columns.clone(), line.rows.clone());
            let table = DataTable::new("sql-result", columns, rows.len(), move |ix, column, _, cx| {
                match rows.get(ix).zip(names.get(column)).and_then(|(row, name)| data::cell_text(row.get(name))) {
                    Some(text) => text.into_any_element(),
                    None => div().text_color(cx.ui().colors.text_subtle).child("—").into_any_element(),
                }
            })
            .compact()
            .scroll_x();
            div().flex_1().min_h(px(160.)).border_t_1().border_color(c.border).child(table)
        });
        let has_table = table.is_some();
        panel
            .child(
                div()
                    .id("sql-lines")
                    // With a table under it, the list of entries keeps to a part of the panel.
                    .map(|this| if has_table { this.flex_none().max_h(px(190.)) } else { this.flex_1().min_h_0() })
                    .overflow_y_scroll()
                    .children(lines.collect::<Vec<_>>()),
            )
            .children(table)
    }
}

impl Render for SqlConsole {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let m = cx.ui().m;
        col().size_full().gap(m.space_4).p(m.space_4).child(self.editor(cx)).child(self.output(cx))
    }
}

#[cfg(test)]
mod tests {
    // Not `super::*`: it would bring in GPUI's own `test` attribute in place of Rust's.
    use serde_json::json;

    use super::sample_text;

    #[test]
    fn the_rows_a_write_would_touch_are_shown_as_indented_json() {
        let rows = vec![json!({ "id": "c-1", "notes": null }).as_object().cloned().unwrap()];
        let text = sample_text(&rows);
        assert!(text.starts_with("[\n  {\n"), "{text}");
        assert!(text.contains("\"id\": \"c-1\"") && text.contains("\"notes\": null"));
        assert_eq!(sample_text(&[]), "[]");
    }
}
