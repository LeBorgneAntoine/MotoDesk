//! What an inspector asks to see (ports of `RegisterView`, `ClosuresView`, `ArchivesView` and
//! `JournalView`): the register and its integrity, the closures, the archives, and the journal
//! itself. Read-only, except closing a period and producing an archive, which append to the
//! journal and can never be undone.

use std::collections::HashMap;
use std::rc::Rc;

use chrono::{Datelike, NaiveDate};
use motodesk_client::fiscal::{
    self, Archive, ArchiveVerification, Closure, ClosureKind, IntegrityReport, JOURNAL_EVENTS, JournalEntry, JournalQuery,
    RegisterInfo, journal_event_label,
};
use motodesk_ui::gpui::prelude::FluentBuilder as _;
use motodesk_ui::gpui::*;
use motodesk_ui::*;

use super::period::{Period, PeriodHost, closure_period_label, default_closure_day, today};
use super::{filling, framed, kpis, section};
use crate::auth::AuthStore;
use crate::backend::ApiExt as _;
use crate::format::{self, humanize_error};

/// How many closures the list holds.
const CLOSURES_LISTED: u32 = 200;
/// How many journal entries are read for a period.
const ENTRIES_READ: u32 = 2000;
/// How many unclosed days are offered one by one.
const PENDING_SHOWN: usize = 24;
/// "Every event" in the journal's filter.
const ALL: &str = "all";

fn is_admin(cx: &App) -> bool {
    AuthStore::global(cx).read(cx).is_admin()
}

/// A paragraph of explanation under a card's title.
fn note(text: impl Into<SharedString>, cx: &App) -> Div {
    let theme = cx.ui();
    div().fs(theme.m.fs_sm).text_color(theme.colors.text_muted).child(text.into())
}

// ---- caisse et intégrité -----------------------------------------------------------------------

pub struct RegisterView {
    register: Option<RegisterInfo>,
    loading: bool,
    error: Option<SharedString>,
    report: Option<Rc<IntegrityReport>>,
    verifying: bool,
    verify_error: Option<SharedString>,
}

impl RegisterView {
    pub fn new(_: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut view = Self { register: None, loading: true, error: None, report: None, verifying: false, verify_error: None };
        view.load(cx);
        view
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        self.error = None;
        cx.notify();
        cx.api(
            |client| async move { fiscal::register(&client).await },
            |this, result, cx| {
                this.loading = false;
                match result {
                    Ok(register) => this.register = Some(register),
                    Err(error) => {
                        this.register = None;
                        this.error = Some(humanize_error(&error, "Impossible de lire la caisse.").into());
                    }
                }
                cx.notify();
            },
        );
    }

    fn verify(&mut self, cx: &mut Context<Self>) {
        if self.verifying {
            return;
        }
        self.verifying = true;
        self.verify_error = None;
        cx.notify();
        cx.api(
            |client| async move { fiscal::verify_chains(&client).await },
            |this, result, cx| {
                this.verifying = false;
                match result {
                    Ok(report) => this.report = Some(Rc::new(report)),
                    Err(error) => {
                        this.report = None;
                        this.verify_error = Some(humanize_error(&error, "La vérification n'a pas pu aboutir.").into());
                    }
                }
                cx.notify();
            },
        );
    }

    fn integrity(&self, cx: &mut Context<Self>) -> SectionCard {
        let theme = cx.ui().clone();
        let (c, m) = (theme.colors, theme.m);
        let card = SectionCard::new()
            .title("Vérifier l'intégrité")
            .subtitle("Recalcule chaque empreinte et contrôle chaque signature, de la première écriture à la dernière — y compris les caisses répliquées depuis les autres postes.")
            .icon(IconName::Shield)
            .action(
                Button::new("verify-chains")
                    .primary()
                    .small()
                    .icon(IconName::ShieldCheck)
                    .label("Lancer la vérification")
                    .loading(self.verifying)
                    .on_click(cx.listener(|this, _, _, cx| this.verify(cx))),
            );
        if let Some(error) = self.verify_error.clone() {
            return card.child(Alert::danger().child(error));
        }
        let Some(report) = self.report.clone() else {
            return card.child(note(
                "La vérification ne modifie aucune donnée ; son résultat est inscrit au journal. Elle peut être lancée à tout moment, et devrait l'être après toute restauration de sauvegarde.",
                cx,
            ));
        };

        let verdict = if report.ok {
            let chains = if report.registers.len() == 1 { "chaînes de la caisse sont intactes" } else { "chaînes des caisses sont intactes" };
            Alert::success()
                .child(format!("Aucune anomalie. Les {chains}, vérification du {}.", format::date_time(&report.checked_at)))
        } else {
            Alert::danger().child(
                "Une rupture a été détectée. Conservez la base en l'état et faites-en une copie : le détail ci-dessous indique où la chaîne s'interrompt.",
            )
        };
        // What each register has to add to its line of the table.
        let remarks: Vec<String> = report
            .registers
            .iter()
            .flat_map(|register| {
                let unverifiable = (register.ok && register.declared_unverifiable > 0).then(|| {
                    format!(
                        "{} : {} écriture(s) ont une signature que le journal déclare invérifiable (clé de signature perdue, puis remplacée). Leur chaînage est intact.",
                        register.code, register.declared_unverifiable
                    )
                });
                let findings: &[String] = if register.ok { &[] } else { register.further_findings() };
                let findings: Vec<String> = findings.iter().map(|finding| format!("{} : {finding}", register.code)).collect();
                unverifiable.into_iter().chain(findings)
            })
            .collect();

        let rows = report.clone();
        card.child(
            col()
                .gap(m.space_3)
                .child(verdict)
                .child(
                    div().border_1().border_color(c.border).child(
                        DataTable::new(
                            "integrity",
                            vec![
                                Column::new("Caisse").fixed(px(120.)).mono(),
                                Column::new("Écritures").fixed(px(110.)).right(),
                                Column::new("Dernier n°").fixed(px(110.)).right(),
                                Column::new("Signatures").fixed(px(120.)).right(),
                                Column::new("État").flex(1.),
                            ],
                            rows.registers.len(),
                            move |ix, column, _, _| {
                                let register = &rows.registers[ix];
                                match column {
                                    0 => register.code.clone().into_any_element(),
                                    1 => format::integer(register.entries).into_any_element(),
                                    2 => format::integer(register.last_seq).into_any_element(),
                                    3 => format::integer(register.signatures_verified).into_any_element(),
                                    _ if register.ok => Tag::new("Chaîne intacte").status(Status::Success).into_any_element(),
                                    _ => {
                                        let problem = register.problem.clone().unwrap_or_else(|| "Rupture".into());
                                        let at = register.problem_seq.map(|seq| format!(" (écriture {seq})")).unwrap_or_default();
                                        Tag::new(format!("{problem}{at}")).status(Status::Danger).into_any_element()
                                    }
                                }
                            },
                        )
                        .compact()
                        .auto_height(),
                    ),
                )
                .children(remarks.into_iter().map(|remark| note(remark, cx))),
        )
    }
}

impl Render for RegisterView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.ui().clone();
        let (c, m) = (theme.colors, theme.m);
        if let Some(error) = self.error.clone() {
            return section(cx)
                .child(filling(StateView::error().compact().title("Caisse indisponible").body(error).action(
                    "Réessayer",
                    cx.listener(|this, _, _, cx| this.load(cx)),
                )))
                .into_any_element();
        }
        let Some(register) = self.register.clone() else {
            return section(cx).child(filling(StateView::loading().compact().title("Lecture de la caisse…"))).into_any_element();
        };

        let tiles = kpis(
            vec![
                KpiTile::new("Caisse", register.code.clone()).icon(IconName::HardDrive).hint("Identifiant de cet appareil"),
                KpiTile::new("Écritures scellées", format::integer(register.last_seq)).icon(IconName::Lock),
                KpiTile::new("Total de la période", format::euros(register.period_grand_total_cents)).icon(IconName::TrendingUp),
                KpiTile::new("Grand total perpétuel", format::euros(register.perpetual_total_cents))
                    .icon(IconName::Infinity)
                    .status(Status::Accent)
                    .hint("Depuis la première utilisation, jamais remis à zéro"),
            ],
            cx,
        );
        let clock = (register.journal_ahead_secs > 0).then(|| {
            let hours = (register.journal_ahead_secs as f64 / 3600.).round() as i64;
            Alert::warning().title("L'horloge de cet ordinateur est en retard sur le journal").child(format!(
                "La dernière écriture du journal est datée de {hours} h dans le futur. Le journal ne peut pas revenir en arrière : tant que l'horloge n'aura pas rattrapé cette date, chaque nouvelle écriture reprend la date de la précédente (l'heure réelle est conservée dans l'écriture). Vérifiez la date et l'heure de l'ordinateur."
            ))
        });
        let key_store =
            if register.key_store == "keychain" { "Trousseau du système" } else { "Base locale (trousseau indisponible)" };
        let identity = SectionCard::new().title("Identité de la caisse").icon(IconName::Info).child(InfoList::new(vec![
            InfoItem::new("Série des documents", Some(format!("FA-{}-{}-…", today().year(), register.code))),
            InfoItem::new("Version du logiciel", Some(register.software_version.clone())),
            InfoItem::new("Ouverte le", Some(format::date_time(&register.opened_at))),
            InfoItem::new("Clé de signature", Some(key_store)),
            InfoItem::new("Clé publique", Some(register.public_key.clone())).mono().block(),
            InfoItem::new("Dernière empreinte", Some(register.last_chain_hash.clone())).mono().block(),
        ]));
        let attestation = SectionCard::new()
            .title("Attestation de conformité")
            .subtitle("À présenter en cas de contrôle, avec les archives de l'exercice.")
            .icon(IconName::Scale)
            .child(
                col()
                    .gap(m.space_2)
                    .child(note(
                        "L'article 286 I-3° bis du CGI exige une preuve du respect des conditions d'inaltérabilité, de sécurisation, de conservation et d'archivage. Motodesk étant développé en interne, cette preuve prend la forme d'une attestation que vous signez vous-même, sur le modèle BOI-LETTRE-000242, en y nommant la version :",
                        cx,
                    ))
                    .child(
                        div()
                            .font_family(FONT_MONO)
                            .fs(m.fs_md)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(c.text_strong)
                            .child(register.software_version.clone()),
                    )
                    .child(note(
                        "Le modèle à remplir et la documentation technique correspondante se trouvent dans le dossier docs/conformite-fiscale/ du logiciel. Une nouvelle attestation doit être signée à chaque version qui touche à la caisse, au journal ou aux clôtures.",
                        cx,
                    )),
            );

        div()
            .id("register-view")
            .size_full()
            .overflow_y_scroll()
            .child(
                col()
                    .p(m.space_4)
                    .gap(m.space_4)
                    .child(tiles)
                    .children(clock)
                    .child(identity)
                    .child(self.integrity(cx))
                    .child(attestation),
            )
            .into_any_element()
    }
}

// ---- clôtures ----------------------------------------------------------------------------------

/// What the confirmation says before a period is closed: which one, and what closing means.
fn closure_warning(kind: ClosureKind, on: Option<NaiveDate>, today: NaiveDate) -> String {
    let what = match kind {
        ClosureKind::Day => "journalière",
        ClosureKind::Month => "mensuelle",
        ClosureKind::Year => "annuelle",
    };
    // Name the period that will really be closed: without a date the service takes the latest
    // finished one, and "la clôture mensuelle" alone would not say which.
    let day = on.unwrap_or_else(|| default_closure_day(kind, today));
    let consequence = if day == today {
        "C'est la journée en cours : plus aucune vente ni aucun règlement ne pourra être enregistré aujourd'hui."
    } else {
        "Aucun document ne pourra plus être daté dans cette période."
    };
    format!(
        "Clôture {what} : {}. Elle fige les totaux de la période et les scelle dans le journal ; elle ne peut être ni refaite ni supprimée. {consequence}",
        closure_period_label(kind, day)
    )
}

/// The period a closure covers, by its name: a day, "octobre 2026", "2025".
fn period_of(closure: &Closure) -> String {
    match (closure.kind, NaiveDate::parse_from_str(&closure.period_start, "%Y-%m-%d")) {
        (ClosureKind::Day, _) | (_, Err(_)) => format::date(&closure.period_start),
        (kind, Ok(start)) => closure_period_label(kind, start),
    }
}

pub struct ClosuresView {
    rows: Rc<Vec<Closure>>,
    /// The days with activity that are not closed yet, oldest first.
    pending: Vec<String>,
    loading: bool,
    error: Option<SharedString>,
}

impl ClosuresView {
    pub fn new(_: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut view = Self { rows: Rc::new(Vec::new()), pending: Vec::new(), loading: true, error: None };
        view.load(cx);
        view
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        self.error = None;
        cx.notify();
        cx.api(
            |client| async move {
                let closures = fiscal::list_closures(&client, CLOSURES_LISTED).await?;
                let pending = fiscal::pending_closures(&client).await?;
                Ok::<_, motodesk_client::ApiError>((closures, pending))
            },
            |this, result, cx| {
                this.loading = false;
                match result {
                    Ok((closures, pending)) => {
                        this.rows = Rc::new(closures);
                        this.pending = pending;
                    }
                    Err(error) => this.error = Some(humanize_error(&error, "Impossible de charger les clôtures.").into()),
                }
                cx.notify();
            },
        );
    }

    fn ask_close(&mut self, kind: ClosureKind, on: Option<NaiveDate>, window: &mut Window, cx: &mut Context<Self>) {
        let page = cx.entity();
        Confirm::new("Clôturer la période ?", closure_warning(kind, on, today())).confirm_label("Clôturer").open(
            window,
            cx,
            move |window, cx| {
                let on = on.map(|day| day.format("%Y-%m-%d").to_string());
                page.update(cx, |_, cx| {
                    cx.api_in(
                        window,
                        move |client| async move { fiscal::close_period(&client, kind, on.as_deref()).await },
                        |this, result, window, cx| match result {
                            Ok(closure) => {
                                toast::success(
                                    format!(
                                        "Clôture {} scellée — {}.",
                                        closure.kind.label().to_lowercase(),
                                        format::euros(closure.total_ttc_cents)
                                    ),
                                    window,
                                    cx,
                                );
                                this.load(cx);
                            }
                            Err(error) => toast::error(humanize_error(&error, "La clôture a échoué."), window, cx),
                        },
                    )
                })
            },
        );
    }

    fn controls(&self, cx: &mut Context<Self>) -> SectionCard {
        let m = cx.ui().m;
        let card = SectionCard::new()
            .title("Clôturer une période")
            .subtitle("Les trois niveaux sont cumulatifs et obligatoires. La clôture annuelle remet le total de la période à zéro ; le grand total perpétuel, jamais.")
            .icon(IconName::Lock);
        if !is_admin(cx) {
            return card.child(Alert::info().child(
                "Seul un administrateur peut clôturer une période. Les totaux ci-dessous restent consultables.",
            ));
        }
        let button = |id: &'static str, label: &'static str, kind: ClosureKind, on: Option<NaiveDate>, cx: &mut Context<Self>| {
            Button::new(id).small().icon(IconName::Calendar).label(label).on_click(cx.listener(move |this, _, window, cx| {
                this.ask_close(kind, on, window, cx)
            }))
        };
        let buttons = row()
            .flex_wrap()
            .gap(m.space_2)
            .child(button("close-yesterday", "Journalière (hier)", ClosureKind::Day, None, cx))
            .child(button("close-today", "Aujourd'hui (Z)", ClosureKind::Day, Some(today()), cx))
            .child(button("close-month", "Mensuelle (mois dernier)", ClosureKind::Month, None, cx))
            .child(button("close-year", "Annuelle (année dernière)", ClosureKind::Year, None, cx));
        // What the days still to close call for, under the buttons.
        let state: Option<Alert> = if !self.pending.is_empty() {
            let count = self.pending.len();
            let lead = if count == 1 {
                "Une journée d'activité n'est pas clôturée".to_string()
            } else {
                format!("{count} journées d'activité ne sont pas clôturées")
            };
            let days = self.pending.iter().take(PENDING_SHOWN).enumerate().filter_map(|(index, day)| {
                let on = NaiveDate::parse_from_str(day, "%Y-%m-%d").ok()?;
                Some(Button::new(("close-pending", index)).ghost().small().label(format::date(day)).on_click(cx.listener(
                    move |this, _, window, cx| this.ask_close(ClosureKind::Day, Some(on), window, cx),
                )))
            });
            Some(Alert::warning().child(
                col()
                    .gap(m.space_2)
                    .child(format!("{lead}. Clôturez-les dans l'ordre, de la plus ancienne à la plus récente."))
                    .child(row().flex_wrap().gap(m.space_1).children(days).when(count > PENDING_SHOWN, |this| {
                        this.child(format!("et {} autres…", count - PENDING_SHOWN))
                    })),
            ))
        } else if self.loading {
            None
        } else {
            Some(Alert::success().child("Toutes les journées d'activité sont clôturées."))
        };
        card.child(col().gap(m.space_3).child(buttons).children(state))
    }
}

impl Render for ClosuresView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let m = cx.ui().m;
        let body = section(cx).child(div().flex_none().child(col().gap(m.space_3).child(self.controls(cx))));
        if let Some(error) = self.error.clone() {
            return body.child(filling(
                StateView::error().compact().title("Clôtures indisponibles").body(error).action(
                    "Réessayer",
                    cx.listener(|this, _, _, cx| this.load(cx)),
                ),
            ));
        }
        if self.loading && self.rows.is_empty() {
            return body.child(filling(StateView::loading().compact().title("Lecture des clôtures…")));
        }
        if self.rows.is_empty() {
            return body.child(filling(
                StateView::empty()
                    .compact()
                    .icon(IconName::Lock)
                    .title("Aucune clôture")
                    .body("La première clôture journalière peut être lancée dès qu'un document a été validé."),
            ));
        }
        let rows = self.rows.clone();
        body.child(framed(cx).child(
            DataTable::new(
                "closures",
                vec![
                    Column::new("Type").fixed(px(110.)),
                    Column::new("Période").fixed(px(140.)),
                    Column::new("Docs").fixed(px(60.)).right(),
                    Column::new("HT").fixed(px(110.)).right(),
                    Column::new("TVA").fixed(px(100.)).right(),
                    Column::new("TTC").fixed(px(110.)).right(),
                    Column::new("Total perpétuel").fixed(px(170.)).right(),
                    Column::new("Clôturée le").flex(1.),
                ],
                rows.len(),
                move |ix, column, _, _| {
                    let closure = &rows[ix];
                    match column {
                        0 => Tag::new(closure.kind.label())
                            .status(match closure.kind {
                                ClosureKind::Year => Status::Accent,
                                ClosureKind::Month => Status::Info,
                                ClosureKind::Day => Status::Muted,
                            })
                            .into_any_element(),
                        1 => div().one_line().child(period_of(closure)).into_any_element(),
                        2 => closure.document_count.to_string().into_any_element(),
                        3 => format::euros(closure.total_ht_cents).into_any_element(),
                        4 => format::euros(closure.total_vat_cents).into_any_element(),
                        5 => format::euros(closure.total_ttc_cents).into_any_element(),
                        6 => format::euros(closure.perpetual_total_cents).into_any_element(),
                        _ => format::date_time(&closure.closed_at).into_any_element(),
                    }
                },
            )
            .compact(),
        ))
    }
}

// ---- archives ----------------------------------------------------------------------------------

pub struct ArchivesView {
    rows: Rc<Vec<Archive>>,
    loading: bool,
    error: Option<SharedString>,
    year: TextModel,
    creating: bool,
    create_error: Option<SharedString>,
    /// The archive whose file is being checked.
    verifying: Option<String>,
    /// What each check found, by archive.
    results: Rc<HashMap<String, ArchiveVerification>>,
    _subscriptions: Vec<Subscription>,
}

impl ArchivesView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let year = TextModel::builder().placeholder("2025").value((today().year() - 1).to_string()).build(window, cx);
        let subscriptions = vec![year.on_event(window, cx, |this, event, window, cx| {
            if matches!(event, TextEvent::Submitted { .. }) {
                this.create(window, cx);
            }
        })];
        let mut view = Self {
            rows: Rc::new(Vec::new()),
            loading: true,
            error: None,
            year,
            creating: false,
            create_error: None,
            verifying: None,
            results: Rc::new(HashMap::new()),
            _subscriptions: subscriptions,
        };
        view.load(cx);
        view
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        self.error = None;
        cx.notify();
        cx.api(
            |client| async move { fiscal::list_archives(&client).await },
            |this, result, cx| {
                this.loading = false;
                match result {
                    Ok(archives) => this.rows = Rc::new(archives),
                    Err(error) => this.error = Some(humanize_error(&error, "Impossible de lire les archives.").into()),
                }
                cx.notify();
            },
        );
    }

    fn create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.creating || !is_admin(cx) {
            return;
        }
        let Some(year) = self.year.text(cx).trim().parse::<i32>().ok() else {
            self.create_error = Some("Indiquez une année sur quatre chiffres.".into());
            return cx.notify();
        };
        self.creating = true;
        self.create_error = None;
        cx.notify();
        cx.api_in(
            window,
            move |client| async move { fiscal::create_archive(&client, year).await },
            move |this, result, window, cx| {
                this.creating = false;
                match result {
                    Ok(archive) => {
                        toast::success(
                            format!("Archive {year} créée — {} écritures.", format::integer(archive.entry_count)),
                            window,
                            cx,
                        );
                        this.load(cx);
                    }
                    Err(error) => {
                        this.create_error = Some(humanize_error(&error, "L'archivage a échoué.").into());
                        cx.notify();
                    }
                }
            },
        );
    }

    fn verify(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.verifying.is_some() {
            return;
        }
        self.verifying = Some(id.clone());
        cx.notify();
        cx.api_in(
            window,
            {
                let id = id.clone();
                move |client| async move { fiscal::verify_archive(&client, &id).await }
            },
            move |this, result, window, cx| {
                this.verifying = None;
                match result {
                    Ok(found) => {
                        let mut results = (*this.results).clone();
                        results.insert(id, found);
                        this.results = Rc::new(results);
                    }
                    Err(error) => toast::error(humanize_error(&error, "Le contrôle a échoué."), window, cx),
                }
                cx.notify();
            },
        );
    }

    fn form(&self, cx: &mut Context<Self>) -> SectionCard {
        let m = cx.ui().m;
        let card = SectionCard::new()
            .title("Produire une archive")
            .subtitle("Un fichier ZIP contenant les écritures au format CSV ouvert, un manifeste avec l'empreinte SHA-256 de chaque fichier, et la notice explicative en français exigée par le § 230.")
            .icon(IconName::Archive);
        let controls: AnyElement = if is_admin(cx) {
            row()
                .items_end()
                .gap(m.space_3)
                .child(div().w(px(160.)).child(FormField::new("Exercice clos").child(Input::new(&self.year))))
                .child(
                    Button::new("archive-create")
                        .primary()
                        .icon(IconName::Archive)
                        .label("Archiver")
                        .loading(self.creating)
                        .on_click(cx.listener(|this, _, window, cx| this.create(window, cx))),
                )
                .into_any_element()
        } else {
            Alert::info().child("Seul un administrateur peut produire une archive fiscale.").into_any_element()
        };
        card.child(
            col()
                .gap(m.space_3)
                .child(controls)
                .children(self.create_error.clone().map(|e| Alert::danger().child(e)))
                .child(note(
                    "L'archive doit être conservée six ans au minimum, sur un support distinct de celui qui héberge le logiciel.",
                    cx,
                )),
        )
    }
}

impl Render for ArchivesView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.ui().clone();
        let (c, m) = (theme.colors, theme.m);
        let body = section(cx).child(div().flex_none().child(self.form(cx)));
        if let Some(error) = self.error.clone() {
            return body.child(filling(
                StateView::error().compact().title("Archives indisponibles").body(error).action(
                    "Réessayer",
                    cx.listener(|this, _, _, cx| this.load(cx)),
                ),
            ));
        }
        if self.loading && self.rows.is_empty() {
            return body.child(filling(StateView::loading().compact().title("Lecture des archives…")));
        }
        if self.rows.is_empty() {
            return body.child(filling(
                StateView::empty()
                    .compact()
                    .icon(IconName::Archive)
                    .title("Aucune archive")
                    .body("La première archive peut être produite une fois l'exercice précédent clôturé."),
            ));
        }
        let (rows, results) = (self.rows.clone(), self.results.clone());
        let (page, verifying) = (cx.entity(), self.verifying.clone());
        body.child(framed(cx).child(
            DataTable::new(
                "archives",
                vec![
                    Column::new("Période").fixed(px(200.)),
                    Column::new("Écritures").fixed(px(100.)).right(),
                    Column::new("Séquences").fixed(px(130.)).right(),
                    Column::new("Taille").fixed(px(100.)).right(),
                    Column::new("Créée le").fixed(px(150.)),
                    Column::new("Contrôle").flex(1.),
                    Column::new("").fixed(px(260.)).right(),
                ],
                rows.len(),
                move |ix, column, _, _| {
                    let archive = &rows[ix];
                    match column {
                        0 => div()
                            .one_line()
                            .child(format!("{} → {}", format::date(&archive.period_start), format::date(&archive.period_end)))
                            .into_any_element(),
                        1 => format::integer(archive.entry_count).into_any_element(),
                        2 => format!("{} – {}", archive.first_seq, archive.last_seq).into_any_element(),
                        3 => format::bytes(archive.file_bytes).into_any_element(),
                        4 => format::date_time(&archive.created_at).into_any_element(),
                        5 => match results.get(&archive.id) {
                            // Until it is checked, the line says where the file is.
                            None => div()
                                .one_line()
                                .font_family(FONT_MONO)
                                .fs(m.fs_xs)
                                .text_color(c.text_muted)
                                .child(archive.file_path.clone())
                                .into_any_element(),
                            Some(found) if found.ok => Tag::new("Empreinte conforme").status(Status::Success).into_any_element(),
                            Some(found) => Tag::new(found.problem.clone().unwrap_or_else(|| "Archive altérée".into()))
                                .status(Status::Danger)
                                .into_any_element(),
                        },
                        _ => {
                            let (to_verify, path, id) = (page.clone(), archive.file_path.clone(), archive.id.clone());
                            row()
                                .gap(m.space_1)
                                .child(Button::new(("archive-copy", ix)).ghost().small().icon(IconName::Copy).label("Copier le chemin").on_click(
                                    move |_, window, cx| {
                                        cx.write_to_clipboard(ClipboardItem::new_string(path.clone()));
                                        toast::success("Chemin de l'archive copié.", window, cx);
                                    },
                                ))
                                .child(
                                    Button::new(("archive-verify", ix))
                                        .ghost()
                                        .small()
                                        .icon(IconName::ShieldCheck)
                                        .label("Contrôler")
                                        .loading(verifying.as_deref() == Some(archive.id.as_str()))
                                        .on_click(move |_, window, cx| {
                                            let id = id.clone();
                                            to_verify.update(cx, |this, cx| this.verify(id, window, cx))
                                        }),
                                )
                                .into_any_element()
                        }
                    }
                },
            )
            .compact(),
        ))
    }
}

// ---- journal fiscal ----------------------------------------------------------------------------

/// The colour of an event's chip: a correction, a closure, or the ordinary run of things.
fn event_status(event: &str) -> Status {
    if event == "credit_note" || event == "payment_reversal" {
        Status::Warning
    } else if event.starts_with("closure") {
        Status::Info
    } else {
        Status::Muted
    }
}

pub struct JournalView {
    period: Period,
    event: SelectModel,
    search: TextModel,
    entries: Vec<JournalEntry>,
    /// The entries the search lets through: what the table shows.
    shown: Rc<Vec<JournalEntry>>,
    loading: bool,
    error: Option<SharedString>,
    _subscriptions: Vec<Subscription>,
}

impl JournalView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut events = vec![SelectOption::new(ALL, "Tous les événements")];
        events.extend(JOURNAL_EVENTS.iter().map(|(value, label)| SelectOption::new(*value, *label)));
        let event = SelectModel::new(events, Some(ALL), window, cx);
        let search = TextModel::builder().placeholder("N° de document, empreinte, séquence…").build(window, cx);
        let subscriptions = vec![
            event.on_change(window, cx, |this, _, _, cx| this.reload(cx)),
            search.on_event(window, cx, |this, event, _, cx| {
                if event == TextEvent::Changed {
                    this.apply_search(cx);
                }
            }),
        ];
        let mut view = Self {
            period: Period::new(window, cx),
            event,
            search,
            entries: Vec::new(),
            shown: Rc::new(Vec::new()),
            loading: true,
            error: None,
            _subscriptions: subscriptions,
        };
        view.reload(cx);
        view
    }

    fn apply_search(&mut self, cx: &mut Context<Self>) {
        let needle = self.search.text(cx);
        self.shown = Rc::new(self.entries.iter().filter(|entry| entry.matches(&needle)).cloned().collect());
        cx.notify();
    }
}

impl PeriodHost for JournalView {
    fn period(&self) -> &Period {
        &self.period
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        let (from, to) = self.period.range(cx);
        let event_type = self.event.value(cx).filter(|event| event != ALL).unwrap_or_default();
        self.loading = true;
        self.error = None;
        cx.notify();
        cx.api(
            move |client| async move {
                fiscal::journal(&client, &JournalQuery { from, to, event_type, limit: Some(ENTRIES_READ) }).await
            },
            |this, result, cx| {
                this.loading = false;
                match result {
                    Ok(entries) => this.entries = entries,
                    Err(error) => {
                        this.entries = Vec::new();
                        this.error = Some(humanize_error(&error, "Impossible de lire le journal.").into());
                    }
                }
                this.apply_search(cx);
            },
        );
    }
}

impl Render for JournalView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let m = cx.ui().m;
        let body = section(cx).child(self.period.render(self.loading, cx)).child(
            row()
                .flex_none()
                .gap(m.space_3)
                .child(SearchField::new(&self.search).width(px(340.)))
                .child(div().w(px(240.)).child(Select::new(&self.event))),
        );
        if let Some(error) = self.error.clone() {
            return body.child(filling(
                StateView::error().compact().title("Journal indisponible").body(error).action(
                    "Réessayer",
                    cx.listener(|this, _, _, cx| this.reload(cx)),
                ),
            ));
        }
        if self.loading && self.entries.is_empty() {
            return body.child(filling(StateView::loading().compact().title("Lecture du journal…")));
        }
        if self.shown.is_empty() {
            let searching = self.search.trimmed(cx).is_some();
            return body.child(filling(
                StateView::empty()
                    .compact()
                    .icon(IconName::List)
                    .title(if searching { "Aucun résultat" } else { "Journal vide sur la période" }),
            ));
        }
        let rows = self.shown.clone();
        body.child(framed(cx).child(
            DataTable::new(
                "fiscal-journal",
                vec![
                    Column::new("N°").fixed(px(80.)).right(),
                    Column::new("Caisse").fixed(px(90.)).mono(),
                    Column::new("Événement").fixed(px(190.)),
                    Column::new("Horodatage").fixed(px(160.)),
                    Column::new("Document").fixed(px(180.)).mono(),
                    Column::new("TTC").fixed(px(120.)).right(),
                    Column::new("Empreinte").flex(1.).mono(),
                ],
                rows.len(),
                move |ix, column, _, _| {
                    let entry = &rows[ix];
                    match column {
                        0 => entry.seq.to_string().into_any_element(),
                        1 => entry.register_code.clone().into_any_element(),
                        2 => Tag::new(journal_event_label(&entry.event_type).to_string())
                            .status(event_status(&entry.event_type))
                            .into_any_element(),
                        3 => format::date_time(&entry.occurred_at).into_any_element(),
                        4 => div().one_line().child(entry.document_ref.clone().unwrap_or_else(|| "—".into())).into_any_element(),
                        5 if entry.total_ttc_cents == 0 => "—".into_any_element(),
                        5 => format::euros(entry.total_ttc_cents).into_any_element(),
                        // The start of the hash tells two entries apart; the whole of it is in the archive.
                        _ => format!("{}…", entry.chain_hash.chars().take(16).collect::<String>()).into_any_element(),
                    }
                },
            )
            .compact(),
        ))
    }
}

#[cfg(test)]
mod tests {
    // Not `super::*`: it would bring in GPUI's own `test` attribute in place of Rust's.
    use chrono::NaiveDate;
    use motodesk_client::fiscal::ClosureKind;

    use super::{Closure, Status, closure_warning, event_status, period_of};

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn the_confirmation_names_the_period_that_will_be_closed() {
        let today = day(2026, 11, 25);
        let month = closure_warning(ClosureKind::Month, None, today);
        assert!(month.starts_with("Clôture mensuelle : octobre 2026."), "{month}");
        assert!(month.contains("Aucun document ne pourra plus être daté dans cette période."));
        let year = closure_warning(ClosureKind::Year, None, today);
        assert!(year.starts_with("Clôture annuelle : 2025."), "{year}");
        let chosen = closure_warning(ClosureKind::Day, Some(day(2026, 11, 3)), today);
        assert!(chosen.starts_with("Clôture journalière : 3 novembre 2026."), "{chosen}");
    }

    #[test]
    fn closing_the_day_in_progress_says_what_it_stops() {
        let today = day(2026, 11, 25);
        let z = closure_warning(ClosureKind::Day, Some(today), today);
        assert!(z.contains("C'est la journée en cours"), "{z}");
        assert!(!closure_warning(ClosureKind::Day, None, today).contains("journée en cours"));
    }

    #[test]
    fn a_closure_is_listed_under_the_name_of_its_period() {
        let closure = |kind: &str, start: &str| -> Closure {
            serde_json::from_value(serde_json::json!({ "id": "c", "kind": kind, "period_start": start, "period_end": start })).unwrap()
        };
        assert_eq!(period_of(&closure("day", "2026-11-25")), "25/11/2026");
        assert_eq!(period_of(&closure("month", "2026-10-01")), "octobre 2026");
        assert_eq!(period_of(&closure("year", "2025-01-01")), "2025");
        assert_eq!(period_of(&closure("month", "octobre")), "octobre", "what is not a date is shown as it came");
    }

    #[test]
    fn corrections_and_closures_stand_out_in_the_journal() {
        assert_eq!(event_status("credit_note"), Status::Warning);
        assert_eq!(event_status("payment_reversal"), Status::Warning);
        assert_eq!(event_status("closure_month"), Status::Info);
        assert_eq!(event_status("invoice"), Status::Muted);
    }
}
