//! The tables of the local database, one page at a time: look into a table, search it, save a
//! page as CSV, and empty a table after seeing what that removes and typing its name.

use std::rc::Rc;

use motodesk_client::data::{self, ImpactReport, TableInfo, TablePreview};
use motodesk_ui::gpui::*;
use motodesk_ui::*;

use crate::backend::{ApiExt as _, SaveExt as _};
use crate::format::{self, humanize_error};

const PAGE_SIZE: i64 = 100;

/// Emptying a table, in two steps: what it removes, then the name typed back.
struct ClearFlow {
    table: String,
    /// `None` while the service works out what would be removed.
    impact: Option<ImpactReport>,
    /// The second step: the name is being typed.
    confirming: bool,
    busy: bool,
    error: Option<String>,
}

pub struct TablesTab {
    tables: Vec<TableInfo>,
    loading: bool,
    tables_error: Option<String>,
    filter: TextModel,

    selected: Option<String>,
    preview: Option<Rc<TablePreview>>,
    preview_loading: bool,
    preview_error: Option<String>,
    search: TextModel,
    /// The search the page on screen was read with.
    searched: String,
    offset: i64,
    /// Counts the readings of a page: an answer for an older one is dropped.
    loads: u64,

    clear: Option<ClearFlow>,
    typed: TextModel,
    _subscriptions: Vec<Subscription>,
}

impl TablesTab {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let filter = TextModel::builder().placeholder("Filtrer les tables…").build(window, cx);
        let search = TextModel::builder().placeholder("Rechercher dans les colonnes texte…").build(window, cx);
        let typed = TextModel::new(window, cx);
        let subscriptions = vec![
            filter.on_event(window, cx, |_, _, _, cx| cx.notify()),
            search.on_event(window, cx, |this: &mut Self, event, _, cx| {
                if matches!(event, TextEvent::Submitted { .. }) {
                    this.apply_search(cx);
                }
            }),
            typed.on_event(window, cx, |this: &mut Self, event, window, cx| match event {
                TextEvent::Submitted { .. } => this.do_clear(window, cx),
                // The button follows what is typed.
                _ => cx.notify(),
            }),
        ];
        let mut tab = Self {
            tables: Vec::new(),
            loading: true,
            tables_error: None,
            filter,
            selected: None,
            preview: None,
            preview_loading: false,
            preview_error: None,
            search,
            searched: String::new(),
            offset: 0,
            loads: 0,
            clear: None,
            typed,
            _subscriptions: subscriptions,
        };
        tab.load_tables(cx);
        tab
    }

    fn selected_info(&self) -> Option<&TableInfo> {
        let name = self.selected.as_deref()?;
        self.tables.iter().find(|table| table.name == name)
    }

    // ---- reading -------------------------------------------------------------------------------

    fn load_tables(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        self.tables_error = None;
        cx.notify();
        cx.api(
            |client| async move { data::list_tables(&client).await },
            |this, result, cx| {
                this.loading = false;
                match result {
                    Ok(tables) => {
                        this.tables = tables;
                        // The screen opens on the first table.
                        if this.selected.is_none() {
                            this.selected = this.tables.first().map(|table| table.name.clone());
                            this.load_preview(cx);
                        }
                    }
                    Err(error) => this.tables_error = Some(humanize_error(&error, "Impossible de charger les tables.")),
                }
                cx.notify();
            },
        );
    }

    fn load_preview(&mut self, cx: &mut Context<Self>) {
        let Some(name) = self.selected.clone() else { return };
        let (q, offset) = (self.searched.clone(), self.offset);
        self.loads += 1;
        let load = self.loads;
        self.preview_loading = true;
        self.preview_error = None;
        cx.notify();
        cx.api(
            move |client| async move { data::preview_table(&client, &name, &q, PAGE_SIZE, offset).await },
            move |this, result, cx| {
                if this.loads != load {
                    return;
                }
                this.preview_loading = false;
                match result {
                    Ok(preview) => this.preview = Some(Rc::new(preview)),
                    Err(error) => {
                        this.preview = None;
                        this.preview_error = Some(humanize_error(&error, "Erreur d'aperçu."));
                    }
                }
                cx.notify();
            },
        );
    }

    fn select(&mut self, name: String, window: &mut Window, cx: &mut Context<Self>) {
        self.selected = Some(name);
        self.offset = 0;
        self.searched.clear();
        self.search.clear(window, cx);
        self.preview = None;
        self.load_preview(cx);
    }

    fn apply_search(&mut self, cx: &mut Context<Self>) {
        self.searched = self.search.text(cx).trim().to_string();
        self.offset = 0;
        self.load_preview(cx);
    }

    fn turn_page(&mut self, forward: bool, cx: &mut Context<Self>) {
        self.offset = if forward { self.offset + PAGE_SIZE } else { (self.offset - PAGE_SIZE).max(0) };
        self.load_preview(cx);
    }

    /// Save the page on screen as a CSV file.
    fn export(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(preview), Some(name)) = (self.preview.clone(), self.selected.clone()) else { return };
        if preview.rows.is_empty() {
            return;
        }
        let csv = data::rows_to_csv(&preview.columns, &preview.rows);
        cx.save_file(window, &format!("{name}_page.csv"), csv.into_bytes(), |_, outcome, window, cx| match outcome {
            Ok(Some(path)) => toast::success(format!("Page enregistrée : {}", path.display()), window, cx),
            Ok(None) => {}
            Err(error) => toast::error(format!("Impossible d'enregistrer le fichier : {error}"), window, cx),
        });
    }

    // ---- emptying a table ----------------------------------------------------------------------

    fn open_clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(table) = self.selected_info().filter(|table| table.can_clear).map(|table| table.name.clone()) else { return };
        self.clear = Some(ClearFlow { table: table.clone(), impact: None, confirming: false, busy: false, error: None });
        self.typed.clear(window, cx);
        cx.api(
            move |client| async move { data::impact_clear(&client, &table).await },
            |this, result, cx| {
                let Some(flow) = &mut this.clear else { return };
                match result {
                    Ok(impact) => flow.impact = Some(impact),
                    Err(error) => flow.error = Some(humanize_error(&error, "Impossible de préparer l'action.")),
                }
                cx.notify();
            },
        );

        let (for_body, for_footer) = (cx.entity(), cx.entity());
        Modal::new("Vider la table").width(px(520.)).keep_on_overlay_click().open(
            window,
            cx,
            move |_, cx| {
                let theme = cx.ui().clone();
                let (c, m) = (theme.colors, theme.m);
                let tab = for_body.read(cx);
                let Some(flow) = &tab.clear else { return element(div()) };
                let error = flow.error.clone().map(|error| Alert::danger().child(error));
                let body = match (&flow.impact, flow.confirming) {
                    (None, _) if flow.error.is_some() => col(),
                    (None, _) => col().child(div().fs(m.fs_sm).text_color(c.text_muted).child("Analyse en cours…")),
                    (Some(impact), false) => col()
                        .gap(m.space_3)
                        .child(
                            row()
                                .gap(m.space_2)
                                .child(div().font_family(FONT_MONO).font_weight(FontWeight::SEMIBOLD).text_color(c.text_strong).child(impact.table.clone()))
                                .child(div().text_color(c.text_muted).child("—"))
                                .child(div().min_w_0().child(impact.operation.clone())),
                        )
                        .child(row().child(
                            Tag::new(format!("{} ligne(s) concernée(s)", format::integer(impact.rows_affected))).status(Status::Warning).medium(),
                        ))
                        .children(impact.warnings.iter().map(|warning| {
                            row()
                                .items_start()
                                .gap(m.space_2)
                                .fs(m.fs_sm)
                                .child(div().flex_none().text_color(c.warning).child(Icon::new(IconName::TriangleAlert).sm()))
                                .child(div().min_w_0().child(warning.clone()))
                        })),
                    (Some(_), true) => col()
                        .gap(m.space_3)
                        .child(div().fs(m.fs_sm).child(
                            "Cette action est irréversible pour les données affichées. Tapez le nom exact de la table pour confirmer :",
                        ))
                        .child(div().font_family(FONT_MONO).font_weight(FontWeight::SEMIBOLD).text_color(c.text_strong).child(flow.table.clone()))
                        .child(Input::new(&tab.typed).mono()),
                };
                element(col().gap(m.space_3).children(error).child(body))
            },
            move |_, cx| {
                let tab = for_footer.read(cx);
                let Some(flow) = &tab.clear else { return Vec::new() };
                let (cancel, forward) = (for_footer.clone(), for_footer.clone());
                let cancel = Button::new("clear-cancel").ghost().label("Annuler").disabled(flow.busy).on_click(move |_, window, cx| {
                    cancel.update(cx, |this, _| this.clear = None);
                    close_modal(window, cx);
                });
                let next = if flow.confirming {
                    Button::new("clear-go")
                        .danger()
                        .label("Vider définitivement")
                        .loading(flow.busy)
                        .disabled(tab.typed.text(cx).trim() != flow.table || flow.busy)
                        .on_click(move |_, window, cx| forward.update(cx, |this, cx| this.do_clear(window, cx)))
                } else {
                    Button::new("clear-continue").danger().label("Continuer…").disabled(flow.impact.is_none()).on_click(
                        move |_, window, cx| {
                            forward.update(cx, |this, cx| {
                                if let Some(flow) = &mut this.clear {
                                    flow.confirming = true;
                                }
                                this.typed.focus(window, cx);
                                cx.notify();
                            })
                        },
                    )
                };
                vec![element(cancel), element(next)]
            },
        );
    }

    fn do_clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let typed = self.typed.text(cx).trim().to_string();
        let Some(flow) = &mut self.clear else { return };
        let Some(token) = flow.impact.as_ref().map(|impact| impact.confirm_token.clone()) else { return };
        if flow.busy || !flow.confirming || typed != flow.table {
            return;
        }
        flow.busy = true;
        flow.error = None;
        let table = flow.table.clone();
        cx.notify();
        cx.api_in(
            window,
            move |client| async move { data::clear_table(&client, &table, &token).await },
            |this, result, window, cx| {
                match result {
                    Ok(cleared) => {
                        this.clear = None;
                        close_modal(window, cx);
                        toast::success(
                            format!("{} ligne(s) retirée(s) de {}.", format::integer(cleared.rows_affected), cleared.table),
                            window,
                            cx,
                        );
                        this.offset = 0;
                        this.load_tables(cx);
                        this.load_preview(cx);
                    }
                    Err(error) => {
                        if let Some(flow) = &mut this.clear {
                            flow.busy = false;
                            flow.error = Some(humanize_error(&error, "Échec du vidage."));
                        }
                    }
                }
                cx.notify();
            },
        );
    }

    // ---- drawing -------------------------------------------------------------------------------

    fn list(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.ui().clone();
        let (c, m) = (theme.colors, theme.m);
        let needle = self.filter.text(cx).trim().to_lowercase();
        let shown: Vec<&TableInfo> = self.tables.iter().filter(|table| table.name.to_lowercase().contains(&needle)).collect();
        let body: AnyElement = if self.loading && self.tables.is_empty() {
            StateView::loading().compact().into_any_element()
        } else if let Some(error) = self.tables_error.clone() {
            div().p(m.space_3).child(Alert::danger().child(error)).into_any_element()
        } else if shown.is_empty() {
            StateView::empty().compact().icon(IconName::Table).title("Aucune table").into_any_element()
        } else {
            let rows = shown.into_iter().map(|table| {
                let name = table.name.clone();
                ListRow::new(SharedString::from(table.name.clone()), table.name.clone())
                    .icon(if table.is_system() { IconName::Lock } else { IconName::Table })
                    .count(format::integer(table.row_count), None)
                    .selected(self.selected.as_deref() == Some(table.name.as_str()))
                    .on_click(cx.listener(move |this, _, window, cx| this.select(name.clone(), window, cx)))
            });
            div().id("data-tables").size_full().overflow_y_scroll().children(rows.collect::<Vec<_>>()).into_any_element()
        };
        col()
            .w(px(280.))
            .flex_none()
            .bg(c.bg_surface)
            .border_1()
            .border_color(c.border)
            .child(
                row()
                    .flex_none()
                    .justify_between()
                    .px(m.space_3)
                    .py(m.space_2)
                    .border_b_1()
                    .border_color(c.border)
                    .child(div().fs_tight(m.fs_xs).font_weight(FontWeight::SEMIBOLD).text_color(c.text_muted).child("TABLES"))
                    .child(
                        IconButton::new("data-tables-refresh", IconName::RefreshCw)
                            .title("Actualiser la liste")
                            .on_click(cx.listener(|this, _, _, cx| this.load_tables(cx))),
                    ),
            )
            .child(div().flex_none().p(m.space_2).child(Input::new(&self.filter).icon(IconName::Search)))
            .child(div().flex_1().min_h_0().child(body))
    }

    fn table(&self, preview: &Rc<TablePreview>) -> AnyElement {
        if preview.columns.is_empty() {
            let (title, body) = if self.searched.is_empty() {
                ("Table vide", "Cette table ne contient aucune ligne.")
            } else {
                ("Aucune ligne trouvée", "Aucune ligne de cette page ne correspond à la recherche.")
            };
            return StateView::empty().icon(IconName::Table).title(title).body(body).into_any_element();
        }
        let columns = preview
            .columns
            .iter()
            .map(|name| {
                let column = Column::new(name.clone());
                if data::is_id_column(name) { column.mono().flex(1.6) } else { column }
            })
            .collect();
        let rows = preview.clone();
        DataTable::new("data-preview", columns, preview.rows.len(), move |ix, column, _, cx| {
            let value = rows.rows.get(ix).zip(rows.columns.get(column)).and_then(|(row, name)| data::cell_text(row.get(name)));
            match value {
                Some(text) => text.into_any_element(),
                None => div().text_color(cx.ui().colors.text_subtle).child("—").into_any_element(),
            }
        })
        .compact()
        .scroll_x()
        .into_any_element()
    }

    fn main(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.ui().clone();
        let (c, m) = (theme.colors, theme.m);
        let panel = col().flex_1().min_w_0().bg(c.bg_surface).border_1().border_color(c.border);
        let Some(name) = self.selected.clone() else {
            return panel.child(
                StateView::empty()
                    .icon(IconName::Table)
                    .title("Aucune table sélectionnée")
                    .body("Choisissez une table dans la liste pour en inspecter le contenu."),
            );
        };
        let info = self.selected_info();
        let system = info.is_some_and(TableInfo::is_system);
        let has_rows = self.preview.as_ref().is_some_and(|preview| !preview.rows.is_empty());
        let searching = !self.searched.is_empty();

        let toolbar = row()
            .flex_none()
            .flex_wrap()
            .justify_between()
            .gap(m.space_3)
            .px(m.space_4)
            .py(m.space_2)
            .border_b_1()
            .border_color(c.border)
            .child(
                row()
                    .min_w_0()
                    .gap(m.space_2)
                    .child(Icon::new(if system { IconName::Lock } else { IconName::Table }))
                    .child(div().font_family(FONT_MONO).font_weight(FontWeight::SEMIBOLD).text_color(c.text_strong).child(name))
                    .children(info.map(|table| {
                        Tag::new(format!("{} ligne(s)", format::integer(table.row_count)))
                            .status(if system { Status::Muted } else { Status::Accent })
                    })),
            )
            .child(
                row()
                    .gap(m.space_2)
                    .child(
                        Button::new("data-refresh")
                            .ghost()
                            .icon(IconName::RefreshCw)
                            .label("Actualiser")
                            .disabled(self.preview_loading)
                            .on_click(cx.listener(|this, _, _, cx| this.load_preview(cx))),
                    )
                    .child(
                        Button::new("data-export")
                            .icon(IconName::Download)
                            .label("Exporter CSV")
                            .disabled(!has_rows)
                            .on_click(cx.listener(|this, _, window, cx| this.export(window, cx))),
                    )
                    .child(
                        Button::new("data-clear")
                            .danger()
                            .icon(IconName::Trash)
                            .label("Vider la table")
                            .disabled(!info.is_some_and(|table| table.can_clear))
                            .on_click(cx.listener(|this, _, window, cx| this.open_clear(window, cx))),
                    ),
            );

        let search = row()
            .flex_none()
            .gap(m.space_2)
            .px(m.space_4)
            .py(m.space_2)
            .border_b_1()
            .border_color(c.border_subtle)
            .child(div().flex_1().min_w_0().child(Input::new(&self.search).icon(IconName::Search)))
            .child(Button::new("data-search").label("Filtrer").on_click(cx.listener(|this, _, _, cx| this.apply_search(cx))));

        let body: AnyElement = match (&self.preview, self.preview_loading) {
            (None, true) => StateView::loading().title("Chargement de l'aperçu…").into_any_element(),
            (Some(preview), _) => self.table(preview),
            (None, false) => div().into_any_element(),
        };

        let (shown, total) = self.preview.as_ref().map_or((0, 0), |preview| (preview.rows.len(), preview.total));
        let pager = row()
            .flex_none()
            .justify_end()
            .gap(m.space_2)
            .px(m.space_4)
            .py(m.space_2)
            .border_t_1()
            .border_color(c.border)
            .child(div().fs(m.fs_sm).text_color(c.text_muted).child(data::page_label(self.offset, shown, total, searching)))
            .child(
                Button::new("data-previous")
                    .ghost()
                    .small()
                    .icon(IconName::ChevronLeft)
                    .label("Précédent")
                    .disabled(self.offset == 0 || self.preview_loading)
                    .on_click(cx.listener(|this, _, _, cx| this.turn_page(false, cx))),
            )
            .child(
                Button::new("data-next")
                    .ghost()
                    .small()
                    .label("Suivant")
                    .icon_after(IconName::ChevronRight)
                    .disabled(!data::has_next_page(self.offset, shown, total, searching, PAGE_SIZE) || self.preview_loading)
                    .on_click(cx.listener(|this, _, _, cx| this.turn_page(true, cx))),
            );

        panel
            .child(toolbar)
            .child(search)
            .children(self.preview_error.clone().map(|error| div().flex_none().p(m.space_3).child(Alert::danger().child(error))))
            .child(div().flex_1().min_h_0().child(body))
            .child(pager)
    }
}

impl Render for TablesTab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let m = cx.ui().m;
        row().size_full().items_stretch().gap(m.space_4).p(m.space_4).child(self.list(cx)).child(self.main(cx))
    }
}
