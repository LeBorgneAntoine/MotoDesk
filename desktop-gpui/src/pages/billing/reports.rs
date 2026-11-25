//! The accounting reports (ports of `SalesJournalView`, `CashJournalView`, `VatRecapView`,
//! `AgeingView` and `FecView`). Every figure comes from sealed documents: nothing here writes
//! to the books.

use std::rc::Rc;

use motodesk_client::accounting::{self, Ageing, CashJournal, FecExport, SalesJournal, VatRecap};
use motodesk_client::sales::payment_method_label;
use motodesk_ui::gpui::prelude::FluentBuilder as _;
use motodesk_ui::gpui::*;
use motodesk_ui::*;

use super::period::{Period, PeriodHost, today};
use super::{customer, filling, framed, kpis, section};
use crate::backend::{ApiExt as _, SaveExt as _};
use crate::format::{self, humanize_error};
use crate::pages::lines::percent;

/// How many lines of the FEC are shown before it is saved.
const FEC_PREVIEW_LINES: usize = 12;

// ---- journal des ventes ------------------------------------------------------------------------

pub struct SalesJournalView {
    period: Period,
    data: Option<Rc<SalesJournal>>,
    loading: bool,
    error: Option<SharedString>,
}

impl SalesJournalView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut view = Self { period: Period::new(window, cx), data: None, loading: true, error: None };
        view.reload(cx);
        view
    }
}

impl PeriodHost for SalesJournalView {
    fn period(&self) -> &Period {
        &self.period
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        let (from, to) = self.period.range(cx);
        self.loading = true;
        self.error = None;
        cx.notify();
        cx.api(
            move |client| async move { accounting::sales_journal(&client, &from, &to).await },
            |this, result, cx| {
                this.loading = false;
                match result {
                    Ok(journal) => this.data = Some(Rc::new(journal)),
                    Err(error) => {
                        this.data = None;
                        this.error = Some(humanize_error(&error, "Impossible de charger le journal des ventes.").into());
                    }
                }
                cx.notify();
            },
        );
    }
}

impl Render for SalesJournalView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = section(cx).child(self.period.render(self.loading, cx));
        if let Some(error) = self.error.clone() {
            return body.child(filling(
                StateView::error().compact().title("Journal indisponible").body(error).action(
                    "Réessayer",
                    cx.listener(|this, _, _, cx| this.reload(cx)),
                ),
            ));
        }
        let Some(data) = self.data.clone() else {
            return body.child(filling(StateView::loading().compact().title("Calcul du journal…")));
        };
        let body = body.child(kpis(
            vec![
                KpiTile::new("Documents", data.rows.len().to_string()).icon(IconName::FileText),
                KpiTile::new("Total HT", format::euros(data.total_ht_cents)).icon(IconName::Receipt),
                KpiTile::new("TVA collectée", format::euros(data.total_vat_cents)).icon(IconName::Banknote),
                KpiTile::new("Total TTC", format::euros(data.total_ttc_cents)).icon(IconName::TrendingUp).status(Status::Accent),
            ],
            cx,
        ));
        if data.rows.is_empty() {
            return body.child(filling(
                StateView::empty()
                    .compact()
                    .icon(IconName::FileText)
                    .title("Aucun document sur la période")
                    .body("Seuls les documents validés apparaissent ici."),
            ));
        }
        let rows = data.clone();
        body.child(framed(cx).child(
            DataTable::new(
                "sales-journal",
                vec![
                    Column::new("N°").fixed(px(180.)).mono(),
                    Column::new("Type").fixed(px(100.)),
                    Column::new("Date").fixed(px(110.)),
                    Column::new("Client").flex(1.),
                    Column::new("HT").fixed(px(120.)).right(),
                    Column::new("TVA").fixed(px(110.)).right(),
                    Column::new("TTC").fixed(px(120.)).right(),
                    Column::new("Encaissé").fixed(px(120.)).right(),
                ],
                rows.rows.len(),
                move |ix, column, _, _| {
                    let line = &rows.rows[ix];
                    match column {
                        0 => div().one_line().child(line.reference.clone().unwrap_or_else(|| "—".into())).into_any_element(),
                        1 => Tag::new(line.kind_label())
                            .status(if line.document_kind == "credit_note" { Status::Warning } else { Status::Muted })
                            .into_any_element(),
                        2 => format::date(&line.issued_on).into_any_element(),
                        3 => div().one_line().child(customer(&line.client_name)).into_any_element(),
                        4 => format::euros(line.total_ht_cents).into_any_element(),
                        5 => format::euros(line.total_vat_cents).into_any_element(),
                        6 => format::euros(line.total_ttc_cents).into_any_element(),
                        _ => format::euros(line.paid_cents).into_any_element(),
                    }
                },
            )
            .compact(),
        ))
    }
}

// ---- journal de caisse -------------------------------------------------------------------------

pub struct CashJournalView {
    period: Period,
    data: Option<Rc<CashJournal>>,
    loading: bool,
    error: Option<SharedString>,
}

impl CashJournalView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut view = Self { period: Period::new(window, cx), data: None, loading: true, error: None };
        view.reload(cx);
        view
    }
}

impl PeriodHost for CashJournalView {
    fn period(&self) -> &Period {
        &self.period
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        let (from, to) = self.period.range(cx);
        self.loading = true;
        self.error = None;
        cx.notify();
        cx.api(
            move |client| async move { accounting::cash_journal(&client, &from, &to).await },
            |this, result, cx| {
                this.loading = false;
                match result {
                    Ok(journal) => this.data = Some(Rc::new(journal)),
                    Err(error) => {
                        this.data = None;
                        this.error = Some(humanize_error(&error, "Impossible de charger le journal de caisse.").into());
                    }
                }
                cx.notify();
            },
        );
    }
}

impl Render for CashJournalView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = cx.ui().colors;
        let body = section(cx).child(self.period.render(self.loading, cx));
        if let Some(error) = self.error.clone() {
            return body.child(filling(
                StateView::error().compact().title("Journal indisponible").body(error).action(
                    "Réessayer",
                    cx.listener(|this, _, _, cx| this.reload(cx)),
                ),
            ));
        }
        let Some(data) = self.data.clone() else {
            return body.child(filling(StateView::loading().compact().title("Calcul des encaissements…")));
        };
        let mut tiles = vec![
            KpiTile::new("Mouvements", data.rows.len().to_string()).icon(IconName::List),
            KpiTile::new("Total encaissé", format::euros(data.total_cents)).icon(IconName::Banknote).status(Status::Accent),
        ];
        // One tile per way of paying: what the drawer, the terminal and the bank should each show.
        tiles.extend(data.methods_by_amount().into_iter().map(|(method, cents)| {
            KpiTile::new(payment_method_label(Some(method)).to_string(), format::euros(cents)).icon(IconName::Receipt)
        }));
        let body = body.child(kpis(tiles, cx));
        if data.rows.is_empty() {
            return body.child(filling(
                StateView::empty().compact().icon(IconName::Banknote).title("Aucun encaissement sur la période"),
            ));
        }
        let rows = data.clone();
        body.child(framed(cx).child(
            DataTable::new(
                "cash-journal",
                vec![
                    Column::new("Date").fixed(px(110.)),
                    Column::new("Document").fixed(px(180.)).mono(),
                    Column::new("Client").flex(1.),
                    Column::new("Mode").fixed(px(150.)),
                    Column::new("Montant").fixed(px(130.)).right(),
                ],
                rows.rows.len(),
                move |ix, column, _, _| {
                    let line = &rows.rows[ix];
                    match column {
                        0 => format::date(&line.paid_on).into_any_element(),
                        1 => div().one_line().child(line.reference.clone().unwrap_or_else(|| "—".into())).into_any_element(),
                        2 => div().one_line().child(customer(&line.client_name)).into_any_element(),
                        3 if line.reverses_payment_id.is_some() => Tag::new("Annulation").status(Status::Warning).into_any_element(),
                        3 => payment_method_label(line.method.as_deref()).to_string().into_any_element(),
                        _ => div()
                            .when(line.amount_cents < 0, |this| this.text_color(c.danger))
                            .child(format::euros(line.amount_cents))
                            .into_any_element(),
                    }
                },
            )
            .compact(),
        ))
    }
}

// ---- TVA ---------------------------------------------------------------------------------------

pub struct VatRecapView {
    period: Period,
    data: Option<Rc<VatRecap>>,
    loading: bool,
    error: Option<SharedString>,
}

impl VatRecapView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut view = Self { period: Period::new(window, cx), data: None, loading: true, error: None };
        view.reload(cx);
        view
    }
}

impl PeriodHost for VatRecapView {
    fn period(&self) -> &Period {
        &self.period
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        let (from, to) = self.period.range(cx);
        self.loading = true;
        self.error = None;
        cx.notify();
        cx.api(
            move |client| async move { accounting::vat_recap(&client, &from, &to).await },
            |this, result, cx| {
                this.loading = false;
                match result {
                    Ok(recap) => this.data = Some(Rc::new(recap)),
                    Err(error) => {
                        this.data = None;
                        this.error = Some(humanize_error(&error, "Impossible de calculer la TVA.").into());
                    }
                }
                cx.notify();
            },
        );
    }
}

impl Render for VatRecapView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = cx.ui().colors;
        let body = section(cx).child(self.period.render(self.loading, cx));
        if let Some(error) = self.error.clone() {
            return body.child(filling(
                StateView::error().compact().title("Récapitulatif indisponible").body(error).action(
                    "Réessayer",
                    cx.listener(|this, _, _, cx| this.reload(cx)),
                ),
            ));
        }
        let Some(data) = self.data.clone() else {
            return body.child(filling(StateView::loading().compact().title("Calcul de la TVA…")));
        };
        let body = body.child(kpis(
            vec![
                KpiTile::new("Documents", data.document_count.to_string()).icon(IconName::FileText),
                KpiTile::new("Base HT totale", format::euros(data.total_base_ht_cents)).icon(IconName::Receipt),
                KpiTile::new("TVA collectée", format::euros(data.total_vat_cents))
                    .icon(IconName::Banknote)
                    .status(Status::Accent)
                    .hint("À reporter sur la CA3"),
            ],
            cx,
        ));
        let note = Alert::info().child(
            "Ces montants couvrent la TVA collectée sur les ventes. La TVA déductible sur vos achats n'est pas suivie ici : votre expert-comptable la reprend depuis vos factures fournisseurs.",
        );
        if data.rates.is_empty() {
            return body
                .child(div().h(px(180.)).child(StateView::empty().compact().icon(IconName::Receipt).title("Aucune TVA sur la période")))
                .child(note);
        }
        let rows = data.clone();
        body.child(
            div().flex_none().bg(c.bg_surface).border_1().border_color(c.border).child(
                DataTable::new(
                    "vat-recap",
                    vec![
                        Column::new("Taux").flex(1.),
                        Column::new("Base HT").fixed(px(180.)).right(),
                        Column::new("TVA collectée").fixed(px(180.)).right(),
                        Column::new("TTC").fixed(px(180.)).right(),
                    ],
                    rows.rates.len(),
                    move |ix, column, _, _| {
                        let rate = &rows.rates[ix];
                        match column {
                            0 => percent(rate.rate_bp).into_any_element(),
                            1 => format::euros(rate.base_ht_cents).into_any_element(),
                            2 => format::euros(rate.vat_cents).into_any_element(),
                            _ => format::euros(rate.total_ttc_cents).into_any_element(),
                        }
                    },
                )
                .compact()
                .auto_height(),
            ),
        )
        .child(note)
    }
}

// ---- balance âgée ------------------------------------------------------------------------------

/// How late an invoice is, as its chip says it.
fn lateness(days_overdue: i64) -> (String, Status) {
    match days_overdue {
        days if days < 0 => ("À échoir".to_string(), Status::Muted),
        days if days <= 30 => (format!("{days} j"), Status::Warning),
        days => (format!("{days} j"), Status::Danger),
    }
}

pub struct AgeingView {
    data: Option<Rc<Ageing>>,
    loading: bool,
    error: Option<SharedString>,
}

impl AgeingView {
    pub fn new(_: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut view = Self { data: None, loading: true, error: None };
        view.load(cx);
        view
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        self.error = None;
        cx.notify();
        cx.api(
            |client| async move { accounting::ageing(&client).await },
            |this, result, cx| {
                this.loading = false;
                match result {
                    Ok(ageing) => this.data = Some(Rc::new(ageing)),
                    Err(error) => {
                        this.data = None;
                        this.error = Some(humanize_error(&error, "Impossible de calculer la balance âgée.").into());
                    }
                }
                cx.notify();
            },
        );
    }
}

impl Render for AgeingView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = section(cx);
        if let Some(error) = self.error.clone() {
            return body.child(filling(
                StateView::error().compact().title("Balance indisponible").body(error).action(
                    "Réessayer",
                    cx.listener(|this, _, _, cx| this.load(cx)),
                ),
            ));
        }
        let Some(data) = self.data.clone() else {
            return body.child(filling(StateView::loading().compact().title("Calcul des impayés…")));
        };
        let b = &data.buckets;
        let body = body.child(kpis(
            vec![
                KpiTile::new("À échoir", format::euros(b.not_due_cents)).icon(IconName::Clock),
                KpiTile::new("0 – 30 j", format::euros(b.overdue_0_30_cents)).icon(IconName::CircleAlert).status(Status::Warning),
                KpiTile::new("31 – 60 j", format::euros(b.overdue_31_60_cents)).icon(IconName::CircleAlert).status(Status::Warning),
                KpiTile::new("61 – 90 j", format::euros(b.overdue_61_90_cents)).icon(IconName::TriangleAlert).status(Status::Danger),
                KpiTile::new("+ 90 j", format::euros(b.overdue_90_plus_cents)).icon(IconName::TriangleAlert).status(Status::Danger),
                KpiTile::new("Total dû", format::euros(data.total_outstanding_cents))
                    .icon(IconName::Banknote)
                    .status(Status::Accent)
                    .hint(format!("Délai de paiement : {} jours", data.payment_terms_days)),
            ],
            cx,
        ));
        if data.rows.is_empty() {
            return body.child(filling(
                StateView::empty()
                    .compact()
                    .icon(IconName::CircleCheck)
                    .title("Aucun impayé")
                    .body("Toutes les factures émises sont soldées."),
            ));
        }
        let rows = data.clone();
        body.child(framed(cx).child(
            DataTable::new(
                "ageing",
                vec![
                    Column::new("Facture").fixed(px(180.)).mono(),
                    Column::new("Client").flex(1.),
                    Column::new("Émise le").fixed(px(120.)),
                    Column::new("Retard").fixed(px(120.)),
                    Column::new("Total TTC").fixed(px(130.)).right(),
                    Column::new("Restant dû").fixed(px(130.)).right(),
                ],
                rows.rows.len(),
                move |ix, column, _, _| {
                    let line = &rows.rows[ix];
                    match column {
                        0 => div().one_line().child(line.reference.clone().unwrap_or_else(|| "—".into())).into_any_element(),
                        1 => div().one_line().child(customer(&line.client_name)).into_any_element(),
                        2 => format::date(&line.issued_on).into_any_element(),
                        3 => {
                            let (label, status) = lateness(line.days_overdue);
                            Tag::new(label).status(status).into_any_element()
                        }
                        4 => format::euros(line.total_ttc_cents).into_any_element(),
                        _ => div().font_weight(FontWeight::SEMIBOLD).child(format::euros(line.outstanding_cents)).into_any_element(),
                    }
                },
            )
            .compact(),
        ))
    }
}

// ---- FEC ---------------------------------------------------------------------------------------

/// The year typed, when it is one: four digits, this century or the last.
fn year_typed(text: &str) -> Option<i32> {
    text.trim().parse::<i32>().ok().filter(|year| (2000..=2999).contains(year))
}

pub struct FecView {
    year: TextModel,
    result: Option<FecExport>,
    building: bool,
    error: Option<SharedString>,
    _subscriptions: Vec<Subscription>,
}

impl FecView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        // Accountants ask for the closed year, so that is what the field starts on.
        let last_year = chrono::Datelike::year(&today()) - 1;
        let year = TextModel::builder().placeholder("2025").value(last_year.to_string()).build(window, cx);
        let subscriptions = vec![year.on_event(window, cx, |this, event, _, cx| {
            if matches!(event, TextEvent::Submitted { .. }) {
                this.build(cx);
            }
        })];
        Self { year, result: None, building: false, error: None, _subscriptions: subscriptions }
    }

    fn build(&mut self, cx: &mut Context<Self>) {
        if self.building {
            return;
        }
        let Some(year) = year_typed(&self.year.text(cx)) else {
            self.error = Some("Indiquez une année sur quatre chiffres.".into());
            return cx.notify();
        };
        self.building = true;
        self.error = None;
        cx.notify();
        cx.api(
            move |client| async move { accounting::fec_export(&client, year).await },
            |this, result, cx| {
                this.building = false;
                match result {
                    Ok(fec) => this.result = Some(fec),
                    Err(error) => {
                        this.result = None;
                        this.error = Some(humanize_error(&error, "Impossible de générer le fichier FEC.").into());
                    }
                }
                cx.notify();
            },
        );
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(fec) = self.result.as_ref() else { return };
        let name = fec.file_name.clone();
        cx.save_file(window, &fec.file_name, fec.bytes(), move |_, outcome, window, cx| match outcome {
            Ok(Some(_)) => toast::success(format!("{name} enregistré."), window, cx),
            // The user closed the dialog: nothing to say.
            Ok(None) => {}
            Err(error) => toast::error(format!("Le fichier n'a pas pu être écrit : {error}"), window, cx),
        });
    }

    fn copy(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(fec) = self.result.as_ref() else { return };
        cx.write_to_clipboard(ClipboardItem::new_string(fec.content.clone()));
        toast::success("Fichier copié dans le presse-papiers.", window, cx);
    }
}

impl Render for FecView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.ui().clone();
        let (c, m) = (theme.colors, theme.m);
        let form = SectionCard::new()
            .title("Fichier des écritures comptables")
            .subtitle("18 colonnes séparées par des barres verticales, encodage UTF-8 — format imposé par l'article A47 A-1 du livre des procédures fiscales.")
            .icon(IconName::FileText)
            .child(
                col()
                    .gap(m.space_3)
                    .child(
                        row()
                            .items_end()
                            .gap(m.space_3)
                            .child(div().w(px(160.)).child(FormField::new("Exercice").child(Input::new(&self.year))))
                            .child(
                                Button::new("fec-build")
                                    .primary()
                                    .icon(IconName::Play)
                                    .label("Générer")
                                    .loading(self.building)
                                    .on_click(cx.listener(|this, _, _, cx| this.build(cx))),
                            ),
                    )
                    .children(self.error.clone().map(|e| Alert::danger().child(e))),
            );

        let result = self.result.as_ref().map(|fec| {
            let card = SectionCard::new()
                .title(fec.file_name.clone())
                .subtitle(format!("{} écritures", format::integer(fec.line_count)))
                .icon(IconName::CircleCheck)
                .action(Button::new("fec-copy").ghost().small().icon(IconName::Copy).label("Copier").on_click(cx.listener(
                    |this, _, window, cx| this.copy(window, cx),
                )))
                .action(Button::new("fec-save").primary().small().icon(IconName::Download).label("Enregistrer").on_click(
                    cx.listener(|this, _, window, cx| this.save(window, cx)),
                ));
            if fec.line_count == 0 {
                card.child(
                    div().h(px(160.)).child(
                        StateView::empty()
                            .compact()
                            .icon(IconName::FileText)
                            .title("Aucune écriture sur l'exercice")
                            .body("Le fichier ne contient que sa ligne d'en-tête."),
                    ),
                )
            } else {
                card.child(
                    col()
                        .gap(m.space_2)
                        .child(Preformatted::new(fec.preview(FEC_PREVIEW_LINES).join("\n")))
                        .child(div().fs(m.fs_xs).text_color(c.text_muted).child("Aperçu des douze premières lignes.")),
                )
            }
        });

        div().id("fec-view").size_full().overflow_y_scroll().child(
            col().p(m.space_4).gap(m.space_4).child(form).children(result).child(Alert::info().child(
                "Le FEC reprend les ventes et les encaissements scellés dans le journal fiscal. Vos achats, salaires et opérations diverses restent tenus par votre expert-comptable : ce fichier est un apport, pas une comptabilité complète.",
            )),
        )
    }
}

#[cfg(test)]
mod tests {
    // Not `super::*`: it would bring in GPUI's own `test` attribute in place of Rust's.
    use super::{Status, lateness, year_typed};

    #[test]
    fn an_invoice_is_not_late_then_late_then_very_late() {
        assert_eq!(lateness(-12), ("À échoir".to_string(), Status::Muted));
        assert_eq!(lateness(0), ("0 j".to_string(), Status::Warning));
        assert_eq!(lateness(30), ("30 j".to_string(), Status::Warning));
        assert_eq!(lateness(31), ("31 j".to_string(), Status::Danger));
        assert_eq!(lateness(120), ("120 j".to_string(), Status::Danger));
    }

    #[test]
    fn a_year_is_four_digits() {
        assert_eq!(year_typed(" 2025 "), Some(2025));
        assert_eq!(year_typed("25"), None);
        assert_eq!(year_typed("1999"), None);
        assert_eq!(year_typed("l'an dernier"), None);
    }
}
