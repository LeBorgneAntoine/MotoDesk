//! Periods: the selector shared by the accounting reports (port of `PeriodBar.svelte`), and
//! the days a closure covers.

use chrono::{Datelike, Days, Months, NaiveDate};
use motodesk_client::fiscal::ClosureKind;
use motodesk_ui::gpui::*;
use motodesk_ui::*;

const MONTHS: [&str; 12] = [
    "janvier", "février", "mars", "avril", "mai", "juin", "juillet", "août", "septembre", "octobre", "novembre", "décembre",
];

fn iso(day: NaiveDate) -> String {
    day.format("%Y-%m-%d").to_string()
}

pub fn today() -> NaiveDate {
    chrono::Local::now().date_naive()
}

fn first_of_month(day: NaiveDate) -> NaiveDate {
    day.with_day(1).unwrap_or(day)
}

fn last_of_month(day: NaiveDate) -> NaiveDate {
    (first_of_month(day) + Months::new(1)) - Days::new(1)
}

// ---- the questions actually asked --------------------------------------------------------------

pub fn this_month(today: NaiveDate) -> (NaiveDate, NaiveDate) {
    (first_of_month(today), last_of_month(today))
}

pub fn last_month(today: NaiveDate) -> (NaiveDate, NaiveDate) {
    let before = first_of_month(today) - Days::new(1);
    (first_of_month(before), before)
}

pub fn this_quarter(today: NaiveDate) -> (NaiveDate, NaiveDate) {
    let first = NaiveDate::from_ymd_opt(today.year(), today.month0() / 3 * 3 + 1, 1).unwrap_or(today);
    (first, last_of_month(first + Months::new(2)))
}

pub fn this_year(today: NaiveDate) -> (NaiveDate, NaiveDate) {
    let year = today.year();
    (NaiveDate::from_ymd_opt(year, 1, 1).unwrap_or(today), NaiveDate::from_ymd_opt(year, 12, 31).unwrap_or(today))
}

// ---- closures ----------------------------------------------------------------------------------

/// The day the service closes when none is given: the latest period of that kind that is
/// over. Kept in step with `default_closure_day` on the service, so that the confirmation can
/// name the period before it is sealed.
pub fn default_closure_day(kind: ClosureKind, today: NaiveDate) -> NaiveDate {
    match kind {
        ClosureKind::Day => today - Days::new(1),
        ClosureKind::Month => first_of_month(today) - Days::new(1),
        ClosureKind::Year => NaiveDate::from_ymd_opt(today.year() - 1, 12, 31).unwrap_or(today),
    }
}

/// "10 novembre 2026", "octobre 2026" or "2025", whichever fits the kind.
pub fn closure_period_label(kind: ClosureKind, day: NaiveDate) -> String {
    let month = MONTHS[day.month0() as usize];
    match kind {
        ClosureKind::Day => {
            let number = if day.day() == 1 { "1er".to_string() } else { day.day().to_string() };
            format!("{number} {month} {}", day.year())
        }
        ClosureKind::Month => format!("{month} {}", day.year()),
        ClosureKind::Year => day.year().to_string(),
    }
}

// ---- the selector ------------------------------------------------------------------------------

/// The view that shows a [`Period`]: it reloads when the period is applied.
pub trait PeriodHost: Sized + 'static {
    fn period(&self) -> &Period;
    fn reload(&mut self, cx: &mut Context<Self>);
}

/// Two dates, typed or set by a preset.
pub struct Period {
    from: TextModel,
    to: TextModel,
    _subscriptions: Vec<Subscription>,
}

impl Period {
    /// Starts on the current year. Enter in either field applies the period.
    pub fn new<H: PeriodHost>(window: &mut Window, cx: &mut Context<H>) -> Self {
        let (start, end) = this_year(today());
        let field = |day: NaiveDate, window: &mut Window, cx: &mut Context<H>| {
            TextModel::builder().placeholder("AAAA-MM-JJ").value(iso(day)).build(window, cx)
        };
        let (from, to) = (field(start, window, cx), field(end, window, cx));
        let subscriptions = [&from, &to]
            .into_iter()
            .map(|field| {
                field.on_event(window, cx, |host: &mut H, event, _, cx| {
                    if matches!(event, TextEvent::Submitted { .. }) {
                        host.reload(cx);
                    }
                })
            })
            .collect();
        Self { from, to, _subscriptions: subscriptions }
    }

    /// The two bounds as typed; an empty one leaves that side open.
    pub fn range(&self, cx: &App) -> (String, String) {
        (self.from.trimmed(cx).unwrap_or_default(), self.to.trimmed(cx).unwrap_or_default())
    }

    fn set(&self, (start, end): (NaiveDate, NaiveDate), window: &mut Window, cx: &mut App) {
        self.from.set_text(iso(start), window, cx);
        self.to.set_text(iso(end), window, cx);
    }

    pub fn render<H: PeriodHost>(&self, busy: bool, cx: &mut Context<H>) -> Div {
        let theme = cx.ui().clone();
        let (c, m) = (theme.colors, theme.m);
        let preset = |id: &'static str, label: &'static str, range: fn(NaiveDate) -> (NaiveDate, NaiveDate), cx: &mut Context<H>| {
            Button::new(id).ghost().small().label(label).on_click(cx.listener(move |host: &mut H, _, window, cx| {
                host.period().set(range(today()), window, cx);
                host.reload(cx);
            }))
        };
        let label = |text: &'static str| div().flex_none().fs(m.fs_sm).text_color(c.text_muted).child(text);
        row()
            .flex_none()
            .flex_wrap()
            .justify_between()
            .gap(m.space_3)
            .child(
                row()
                    .gap(m.space_1)
                    .child(preset("period-month", "Ce mois", this_month, cx))
                    .child(preset("period-last-month", "Mois dernier", last_month, cx))
                    .child(preset("period-quarter", "Ce trimestre", this_quarter, cx))
                    .child(preset("period-year", "Cette année", this_year, cx)),
            )
            .child(
                row()
                    .gap(m.space_2)
                    .child(label("Du"))
                    .child(div().w(px(130.)).child(Input::new(&self.from)))
                    .child(label("au"))
                    .child(div().w(px(130.)).child(Input::new(&self.to)))
                    .child(Button::new("period-apply").icon(IconName::RefreshCw).label("Actualiser").loading(busy).on_click(
                        cx.listener(|host: &mut H, _, _, cx| host.reload(cx)),
                    )),
            )
    }
}

#[cfg(test)]
mod tests {
    // Not `super::*`: it would bring in GPUI's own `test` attribute in place of Rust's.
    use chrono::NaiveDate;
    use motodesk_client::fiscal::ClosureKind;

    use super::{closure_period_label, default_closure_day, iso, last_month, this_month, this_quarter, this_year};

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn span((start, end): (NaiveDate, NaiveDate)) -> (String, String) {
        (iso(start), iso(end))
    }

    #[test]
    fn the_presets_cover_whole_months_quarters_and_years() {
        let today = day(2026, 11, 25);
        assert_eq!(span(this_month(today)), ("2026-11-01".into(), "2026-11-30".into()));
        assert_eq!(span(last_month(today)), ("2026-10-01".into(), "2026-10-31".into()));
        assert_eq!(span(this_quarter(today)), ("2026-10-01".into(), "2026-12-31".into()));
        assert_eq!(span(this_year(today)), ("2026-01-01".into(), "2026-12-31".into()));
    }

    #[test]
    fn the_presets_hold_at_the_turn_of_a_year_and_in_a_leap_february() {
        assert_eq!(span(last_month(day(2026, 1, 15))), ("2025-12-01".into(), "2025-12-31".into()));
        assert_eq!(span(this_month(day(2028, 2, 10))), ("2028-02-01".into(), "2028-02-29".into()));
        assert_eq!(span(this_quarter(day(2026, 2, 28))), ("2026-01-01".into(), "2026-03-31".into()));
        assert_eq!(span(this_month(day(2026, 12, 31))), ("2026-12-01".into(), "2026-12-31".into()));
    }

    #[test]
    fn a_closure_without_a_date_is_the_latest_period_that_is_over() {
        let today = day(2026, 11, 25);
        assert_eq!(iso(default_closure_day(ClosureKind::Day, today)), "2026-11-24");
        assert_eq!(iso(default_closure_day(ClosureKind::Month, today)), "2026-10-31");
        assert_eq!(iso(default_closure_day(ClosureKind::Year, today)), "2025-12-31");
        assert_eq!(iso(default_closure_day(ClosureKind::Day, day(2026, 1, 1))), "2025-12-31");
    }

    #[test]
    fn a_closure_names_its_period() {
        assert_eq!(closure_period_label(ClosureKind::Day, day(2026, 11, 10)), "10 novembre 2026");
        assert_eq!(closure_period_label(ClosureKind::Day, day(2026, 8, 1)), "1er août 2026");
        assert_eq!(closure_period_label(ClosureKind::Month, day(2026, 10, 31)), "octobre 2026");
        assert_eq!(closure_period_label(ClosureKind::Year, day(2025, 12, 31)), "2025");
    }
}
