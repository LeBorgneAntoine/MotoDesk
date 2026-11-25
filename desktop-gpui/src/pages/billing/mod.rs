//! Comptabilité (port of `routes/Billing.svelte` and `routes/billing/*`): the read-only side
//! of the fiscal module, plus the two controls that write to the journal without touching a
//! document: closing a period and producing an archive.
//!
//! The split mirrors how the questions arrive: an accountant asks for journals and a FEC, an
//! inspector asks for the register, the closures and the archive.

mod fiscal;
mod period;
mod reports;

use motodesk_ui::gpui::*;
use motodesk_ui::*;

use crate::router::Router;

/// A section of the screen: one entry of the rail, one view.
struct Section {
    id: &'static str,
    label: &'static str,
    icon: IconName,
    title: &'static str,
    subtitle: &'static str,
    /// What an inspector asks for, rather than an accountant.
    fiscal: bool,
}

const SECTIONS: [Section; 9] = [
    Section {
        id: "sales-journal",
        label: "Journal des ventes",
        icon: IconName::BookOpen,
        title: "Journal des ventes",
        subtitle: "Tous les documents scellés sur la période, dans l'ordre où ils l'ont été.",
        fiscal: false,
    },
    Section {
        id: "cash-journal",
        label: "Journal de caisse",
        icon: IconName::Banknote,
        title: "Journal de caisse",
        subtitle: "Les encaissements de la période, avec la répartition par mode de règlement.",
        fiscal: false,
    },
    Section {
        id: "vat",
        label: "TVA",
        icon: IconName::Receipt,
        title: "Récapitulatif de TVA",
        subtitle: "TVA collectée par taux — les montants qui remplissent la déclaration CA3.",
        fiscal: false,
    },
    Section {
        id: "ageing",
        label: "Balance âgée",
        icon: IconName::Clock,
        title: "Balance âgée",
        subtitle: "Ce qui reste dû, classé par ancienneté du retard.",
        fiscal: false,
    },
    Section {
        id: "fec",
        label: "Export FEC",
        icon: IconName::Download,
        title: "Export FEC",
        subtitle: "Le fichier des écritures comptables, tel que votre expert-comptable l'importe.",
        fiscal: false,
    },
    Section {
        id: "register",
        label: "Caisse & intégrité",
        icon: IconName::Shield,
        title: "Caisse et intégrité",
        subtitle: "L'identité de cet appareil, ses compteurs, et le contrôle des chaînes d'empreintes.",
        fiscal: true,
    },
    Section {
        id: "closures",
        label: "Clôtures",
        icon: IconName::Lock,
        title: "Clôtures",
        subtitle: "Clôtures journalières, mensuelles et annuelles, scellées dans le journal.",
        fiscal: true,
    },
    Section {
        id: "archives",
        label: "Archives",
        icon: IconName::Archive,
        title: "Archives fiscales",
        subtitle: "Exports d'archivage au format ouvert, avec empreinte et notice explicative.",
        fiscal: true,
    },
    Section {
        id: "journal",
        label: "Journal fiscal",
        icon: IconName::List,
        title: "Journal fiscal",
        subtitle: "L'enregistrement inaltérable lui-même, écriture par écriture.",
        fiscal: true,
    },
];

// ---- what the sections share -------------------------------------------------------------------

/// The body of a section: padded, its parts stacked.
fn section(cx: &App) -> Div {
    let m = cx.ui().m;
    col().size_full().p(m.space_4).gap(m.space_4)
}

/// A row of headline figures.
fn kpis(tiles: Vec<KpiTile>, cx: &App) -> Div {
    row().flex_none().items_stretch().gap(cx.ui().m.space_3).children(tiles)
}

/// The frame of a table that takes what is left of the section.
fn framed(cx: &App) -> Div {
    let c = cx.ui().colors;
    div().flex_1().min_h_0().bg(c.bg_surface).border_1().border_color(c.border)
}

/// A message (loading, empty, error) that takes what is left of the section.
fn filling(state: StateView) -> Div {
    div().flex_1().min_h_0().child(state)
}

/// The customer of a document, or the words for "nobody in particular".
fn customer(name: &Option<String>) -> String {
    name.as_deref().map(str::trim).filter(|name| !name.is_empty()).unwrap_or("Client de passage").to_string()
}

// ---- the screen --------------------------------------------------------------------------------

pub struct BillingPage {
    active: &'static str,
    /// The view of the active section. A section left is a section closed: coming back reads
    /// its figures again.
    view: AnyView,
}

fn open(id: &str, window: &mut Window, cx: &mut App) -> AnyView {
    match id {
        "cash-journal" => cx.new(|cx| reports::CashJournalView::new(window, cx)).into(),
        "vat" => cx.new(|cx| reports::VatRecapView::new(window, cx)).into(),
        "ageing" => cx.new(|cx| reports::AgeingView::new(window, cx)).into(),
        "fec" => cx.new(|cx| reports::FecView::new(window, cx)).into(),
        "register" => cx.new(|cx| fiscal::RegisterView::new(window, cx)).into(),
        "closures" => cx.new(|cx| fiscal::ClosuresView::new(window, cx)).into(),
        "archives" => cx.new(|cx| fiscal::ArchivesView::new(window, cx)).into(),
        "journal" => cx.new(|cx| fiscal::JournalView::new(window, cx)).into(),
        _ => cx.new(|cx| reports::SalesJournalView::new(window, cx)).into(),
    }
}

impl BillingPage {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        // `section=closures` opens a section directly (a reminder to close past days uses it).
        let asked = Router::global(cx).read(cx).param("section").map(str::to_string);
        let active = SECTIONS.iter().find(|s| Some(s.id) == asked.as_deref()).unwrap_or(&SECTIONS[0]).id;
        Self { active, view: open(active, window, cx) }
    }

    fn select(&mut self, id: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        if self.active == id {
            return;
        }
        self.active = id;
        self.view = open(id, window, cx);
        Router::global(cx).update(cx, |router, _| router.set_param("section", Some(id.to_string())));
        cx.notify();
    }

    fn rail(&self, cx: &mut Context<Self>) -> FilterRail {
        let theme = cx.ui().clone();
        let (c, m) = (theme.colors, theme.m);
        let group = |label: &'static str| {
            div().px(m.space_3).pt(m.space_3).pb(m.space_1).fs_tight(m.fs_xs).font_weight(FontWeight::SEMIBOLD).text_color(c.text_muted).child(label)
        };
        let entries = |fiscal: bool, cx: &mut Context<Self>| -> Vec<ListRow> {
            SECTIONS
                .iter()
                .filter(|s| s.fiscal == fiscal)
                .map(|s| {
                    let id = s.id;
                    ListRow::new(id, s.label).icon(s.icon).selected(self.active == id).on_click(cx.listener(
                        move |this, _, window, cx| this.select(id, window, cx),
                    ))
                })
                .collect()
        };
        FilterRail::new("billing-rail", "Comptabilité")
            .child(group("RAPPORTS"))
            .children(entries(false, cx))
            .child(group("CONTRÔLE FISCAL"))
            .children(entries(true, cx))
            .footer("Données scellées, en lecture seule")
    }
}

impl Render for BillingPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = cx.ui().colors;
        let active = SECTIONS.iter().find(|s| s.id == self.active).unwrap_or(&SECTIONS[0]);
        row()
            .size_full()
            .items_stretch()
            .bg(c.bg_app)
            .child(self.rail(cx))
            .child(
                col()
                    .flex_1()
                    .min_w_0()
                    .child(PageToolbar::new(active.title).icon(active.icon).subtitle(active.subtitle))
                    .child(div().flex_1().min_h_0().child(self.view.clone())),
            )
    }
}

#[cfg(test)]
mod tests {
    // Not `super::*`: it would bring in GPUI's own `test` attribute in place of Rust's.
    use super::{SECTIONS, customer};

    #[test]
    fn a_document_without_a_customer_is_a_passer_by() {
        assert_eq!(customer(&Some(" Léa Martin ".into())), "Léa Martin");
        assert_eq!(customer(&Some("  ".into())), "Client de passage");
        assert_eq!(customer(&None), "Client de passage");
    }

    #[test]
    fn every_section_has_its_own_id() {
        let mut ids: Vec<&str> = SECTIONS.iter().map(|s| s.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), SECTIONS.len());
        assert_eq!(SECTIONS.iter().filter(|s| s.fiscal).count(), 4);
    }
}
