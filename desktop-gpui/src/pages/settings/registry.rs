//! Every setting the screen shows, and how each one goes from the stored value to the text
//! of its field and back.
//!
//! To add a setting: one entry in [`SETTINGS`]. The screen draws it and the service stores it.
//!
//! Two settings a shop's data may still hold are not here: the corners (`system.radius`),
//! because every corner is square, and the blur (`system.blur`), which this interface does not
//! draw.

use motodesk_ui::IconName;
use serde_json::Value;

pub struct Category {
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub icon: IconName,
    /// Only administrators change it; the others may read it.
    pub admin_only: bool,
    /// Drawn by a view of its own rather than as a grid of fields.
    pub special: bool,
}

const fn category(id: &'static str, label: &'static str, description: &'static str, icon: IconName) -> Category {
    Category { id, label, description, icon, admin_only: true, special: false }
}

pub const CATEGORIES: [Category; 13] = [
    Category {
        special: true,
        ..category("company", "Entreprise", "Identité de la société : nom, raison sociale et logo.", IconName::Building2)
    },
    category("branding", "Image de marque", "Slogan affiché sous le nom sur les documents.", IconName::Tag),
    category("contact", "Contact", "Coordonnées affichées sur les devis et factures.", IconName::Phone),
    category("legal", "Légal & fiscal", "SIRET, mentions légales et régime de TVA.", IconName::FileText),
    category("invoicing", "Facturation", "Délais, pénalités, mentions et coordonnées bancaires.", IconName::Receipt),
    category("stock", "Stock", "Seuil d'alerte par défaut et vente sans stock.", IconName::Boxes),
    category("workshop", "Atelier", "Tarif de main d'œuvre.", IconName::Wrench),
    category("locale", "Régional", "Format des dates affichées.", IconName::Calendar),
    Category {
        admin_only: false,
        ..category("preferences", "Préférences", "Apparence de l'application sur cet appareil.", IconName::Settings)
    },
    Category {
        special: true,
        ..category("system", "Sauvegardes", "Sauvegardes de la base de données et journaux de l'application.", IconName::Database)
    },
    Category {
        special: true,
        ..category("sync", "Synchronisation", "Réseau local, autres postes et état du cloud.", IconName::RefreshCw)
    },
    Category {
        special: true,
        ..category("caisse", "Caisse", "Rôle de cet appareil et périphériques du comptoir.", IconName::Store)
    },
    Category {
        special: true,
        ..category("assistant", "Assistant", "Clé OpenRouter et modèle par défaut, locaux à cet appareil.", IconName::Sparkles)
    },
];

pub fn category_by_id(id: &str) -> Option<&'static Category> {
    CATEGORIES.iter().find(|category| category.id == id)
}

// ---- the fields --------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Text,
    Area,
    /// A number as it is stored (a count, an amount in euros).
    Number,
    /// Stored in cents, typed in euros.
    Cents,
    /// Stored in basis points (2000 is 20 %), typed in percent.
    Percent,
    Toggle,
    /// `#rrggbb`, with these swatches offered.
    Colour(&'static [u32]),
    /// One of these `(value, label)`.
    Choice(&'static [(&'static str, &'static str)]),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Initial {
    Text(&'static str),
    Number(i64),
    Flag(bool),
}

pub struct Def {
    pub category: &'static str,
    pub key: &'static str,
    pub kind: Kind,
    pub label: &'static str,
    /// Empty when the label says it all.
    pub description: &'static str,
    pub initial: Initial,
    pub placeholder: &'static str,
    /// Takes the whole width of the grid.
    pub wide: bool,
    pub unit: &'static str,
    /// Bounds, in the unit the value is stored in.
    pub min: Option<i64>,
    pub max: Option<i64>,
}

const FIELD: Def = Def {
    category: "",
    key: "",
    kind: Kind::Text,
    label: "",
    description: "",
    initial: Initial::Text(""),
    placeholder: "",
    wide: false,
    unit: "",
    min: None,
    max: None,
};

const fn text(category: &'static str, key: &'static str, label: &'static str, placeholder: &'static str) -> Def {
    Def { category, key, label, placeholder, ..FIELD }
}

const fn toggle(category: &'static str, key: &'static str, label: &'static str, initial: bool) -> Def {
    Def { category, key, label, kind: Kind::Toggle, initial: Initial::Flag(initial), wide: true, ..FIELD }
}

const fn number(category: &'static str, key: &'static str, label: &'static str, initial: i64, unit: &'static str) -> Def {
    Def { category, key, label, kind: Kind::Number, initial: Initial::Number(initial), unit, min: Some(0), ..FIELD }
}

/// The accent colours offered as swatches.
const ACCENTS: [u32; 4] = [0x1976d2, 0x00897b, 0x2e7d32, 0x546e7a];

pub const SETTINGS: [Def; 38] = [
    // ---- Image de marque ----
    // The company's name, legal name and logo live on the company itself ("Entreprise").
    Def {
        description: "Courte accroche affichée sous le nom sur les documents.",
        wide: true,
        ..text("branding", "shop.tagline", "Slogan", "Vente, entretien et réparation moto")
    },
    // ---- Contact ----
    Def { wide: true, ..text("contact", "contact.address_line1", "Adresse", "12 rue de l'Atelier") },
    Def { wide: true, ..text("contact", "contact.address_line2", "Complément d'adresse", "Bât. B, 1er étage") },
    text("contact", "contact.postal_code", "Code postal", "35000"),
    text("contact", "contact.city", "Ville", "Rennes"),
    text("contact", "contact.country", "Pays", "France"),
    text("contact", "contact.phone", "Téléphone", "02 99 00 00 00"),
    text("contact", "contact.email", "Email", "contact@atelier.fr"),
    Def { wide: true, ..text("contact", "contact.website", "Site web", "https://atelier.fr") },
    // ---- Légal & fiscal ----
    // All of it is printed on invoices. A document cannot be validated while a mandatory
    // mention is missing: once sealed, the only remedy is a credit note.
    Def {
        description: "Raison sociale exacte, telle qu'immatriculée. Reprend le nom de l'entreprise si vide.",
        wide: true,
        ..text("legal", "shop.legal_name", "Dénomination sociale", "Garage Dupont")
    },
    Def { description: "14 chiffres, sans espaces.", ..text("legal", "legal.siret", "SIRET", "12345678900012") },
    Def { description: "9 chiffres. Déduit du SIRET si laissé vide.", ..text("legal", "legal.siren", "SIREN", "123456789") },
    text("legal", "legal.vat_number", "N° TVA intracommunautaire", "FR12345678901"),
    // No form is assumed: it says what the business is, and decides what its invoices must carry.
    Def {
        kind: Kind::Choice(&[
            ("", "Choisir…"),
            ("EI", "Entreprise individuelle"),
            ("AE", "Auto-entrepreneur"),
            ("EURL", "EURL"),
            ("SARL", "SARL"),
            ("SAS", "SAS"),
            ("SASU", "SASU"),
            ("SA", "SA"),
        ]),
        ..text("legal", "legal.legal_form", "Forme juridique", "")
    },
    number("legal", "legal.capital_eur", "Capital social", 0, "€"),
    text("legal", "legal.rcs_city", "Ville d'immatriculation RCS", "Rennes"),
    text("legal", "legal.ape_code", "Code APE / NAF", "4540Z"),
    Def { description: "Décochez si vous êtes en franchise en base.", ..toggle("legal", "vat.is_subject", "Assujetti à la TVA", true) },
    Def {
        kind: Kind::Percent,
        description: "Taux appliqué aux nouveaux produits et prestations.",
        initial: Initial::Number(2000),
        unit: "%",
        min: Some(0),
        max: Some(10_000),
        ..text("legal", "vat.default_rate_bp", "Taux de TVA par défaut", "")
    },
    Def {
        kind: Kind::Area,
        description: "Affichée sur les documents si vous n'êtes pas assujetti.",
        initial: Initial::Text(motodesk_fiscal::vat::FRANCHISE_MENTION),
        wide: true,
        ..text("legal", "vat.exemption_text", "Mention en cas d'exonération", "")
    },
    // ---- Régional ----
    Def {
        kind: Kind::Choice(&[
            ("short", "31 déc. 2026"),
            ("dd/MM/yyyy", "31/12/2026"),
            ("MM/dd/yyyy", "12/31/2026"),
            ("yyyy-MM-dd", "2026-12-31"),
        ]),
        description: "Appliqué aux dates affichées dans l'application.",
        initial: Initial::Text("short"),
        ..text("locale", "locale.date_format", "Format de date", "")
    },
    // ---- Facturation ----
    // No prefix and no next number here: each register owns its series, and a sequence that
    // anyone could reset would not survive an audit.
    Def { max: Some(365), ..number("invoicing", "invoicing.payment_terms_days", "Délai de paiement par défaut", 30, "jours") },
    Def {
        kind: Kind::Percent,
        description: "Mention obligatoire sur toute facture (C. com. art. L441-9).",
        initial: Initial::Number(1000),
        unit: "%",
        min: Some(0),
        max: Some(10_000),
        ..text("invoicing", "invoicing.late_fee_rate_bp", "Taux de pénalités de retard", "")
    },
    Def {
        description: "Fixée à 40 € par l'article D441-5 du code de commerce.",
        ..number("invoicing", "invoicing.recovery_indemnity_eur", "Indemnité forfaitaire de recouvrement", 40, "€")
    },
    toggle("invoicing", "invoicing.show_logo", "Afficher le logo sur les documents", true),
    Def { wide: true, ..text("invoicing", "invoicing.bank_iban", "IBAN", "FR76 …") },
    text("invoicing", "invoicing.bank_bic", "BIC / SWIFT", "AGRIFRPP…"),
    text("invoicing", "invoicing.bank_name", "Banque", ""),
    Def {
        kind: Kind::Area,
        description: "Affichées en bas de chaque document.",
        wide: true,
        ..text("invoicing", "invoicing.footer_text", "Mentions de pied de page", "Merci de votre confiance.")
    },
    // ---- Stock ----
    number("stock", "stock.low_threshold_default", "Seuil d'alerte stock bas par défaut", 2, ""),
    Def {
        description: "Permet de vendre même quand le stock est à zéro.",
        ..toggle("stock", "stock.allow_negative", "Autoriser le stock négatif", false)
    },
    toggle("stock", "stock.alert_low_stock", "Afficher les alertes de stock bas", true),
    // ---- Atelier ----
    Def {
        kind: Kind::Cents,
        description: "Tarif de main d'œuvre appliqué par défaut.",
        initial: Initial::Number(5500),
        unit: "€/h",
        min: Some(0),
        ..text("workshop", "workshop.labor_rate_ht_cents", "Taux horaire HT", "")
    },
    // ---- Préférences (per PC, open to everyone) ----
    Def {
        kind: Kind::Choice(&[("light", "Clair"), ("dark", "Sombre"), ("auto", "Système")]),
        initial: Initial::Text("light"),
        ..text("preferences", "system.theme", "Thème", "")
    },
    Def {
        kind: Kind::Colour(&ACCENTS),
        description: "Boutons, sélection et barre d'état. Locale à cette machine.",
        initial: Initial::Text("#1976d2"),
        ..text("preferences", "system.accent_color", "Couleur d'accent", "#1976d2")
    },
    Def {
        description: "Réduit l'espacement entre les éléments.",
        ..toggle("preferences", "system.compact_mode", "Mode compact", false)
    },
    // ---- Sauvegardes (drawn by the backups view, which writes them at once) ----
    toggle("system", AUTO_BACKUP, "Sauvegarde automatique", true),
    Def { min: Some(1), max: Some(90), ..number("system", BACKUP_FREQUENCY, "Fréquence de sauvegarde", 7, "jours") },
];

pub const AUTO_BACKUP: &str = "system.auto_backup_enabled";
pub const BACKUP_FREQUENCY: &str = "system.backup_frequency_days";

pub fn settings_for(category: &str) -> impl Iterator<Item = &'static Def> {
    SETTINGS.iter().filter(move |def| def.category == category)
}

pub fn def_by_key(key: &str) -> Option<&'static Def> {
    SETTINGS.iter().find(|def| def.key == key)
}

// ---- values ------------------------------------------------------------------------------------

impl Def {
    /// The value that applies while nothing is stored.
    pub fn default_value(&self) -> Value {
        match self.initial {
            Initial::Text(text) => Value::from(text),
            Initial::Number(number) => Value::from(number),
            Initial::Flag(flag) => Value::from(flag),
        }
    }

    /// The text of the field for a stored value.
    pub fn shown(&self, value: &Value) -> String {
        let number = || value.as_f64().or_else(|| value.as_str().and_then(typed_number)).unwrap_or(0.);
        match self.kind {
            Kind::Number => plain(number()),
            Kind::Cents => format!("{:.2}", number() / 100.).replace('.', ","),
            Kind::Percent => plain(number() / 100.),
            _ => value.as_str().unwrap_or_default().to_string(),
        }
    }

    /// The value to store for what was typed, or why it cannot be stored.
    pub fn read(&self, typed: &str) -> Result<Value, String> {
        match self.kind {
            Kind::Number => self.bounded(typed, 1.),
            Kind::Cents | Kind::Percent => self.bounded(typed, 100.),
            Kind::Colour(_) => match motodesk_ui::parse_hex(typed) {
                Some(colour) => Ok(Value::from(format!("#{colour:06x}"))),
                None => Err("Couleur attendue sous la forme #rrggbb.".into()),
            },
            _ => Ok(Value::from(typed)),
        }
    }

    /// A number typed in the unit shown, stored `scale` times larger, within the bounds.
    fn bounded(&self, typed: &str, scale: f64) -> Result<Value, String> {
        // An emptied field is zero, as it always was.
        let typed = typed.trim();
        let entered = if typed.is_empty() { 0. } else { typed_number(typed).ok_or("Nombre attendu.")? };
        let stored = entered * scale;
        let shown = |bound: i64| plain(bound as f64 / scale);
        let too_small = self.min.filter(|min| stored < *min as f64 - 1e-6);
        let too_large = self.max.filter(|max| stored > *max as f64 + 1e-6);
        if too_small.is_some() || too_large.is_some() {
            return Err(match (self.min, self.max) {
                (Some(min), Some(max)) => format!("Entre {} et {}.", shown(min), shown(max)),
                (Some(min), None) => format!("{} au minimum.", shown(min)),
                (None, Some(max)) => format!("{} au maximum.", shown(max)),
                (None, None) => unreachable!("a bound was crossed"),
            });
        }
        // Whole numbers are stored whole: 30, not 30.0.
        let rounded = if scale > 1. { stored.round() } else { stored };
        Ok(if rounded.fract() == 0. { Value::from(rounded as i64) } else { Value::from(rounded) })
    }
}

/// A number typed the French or the English way: `12,5` or `12.5`.
fn typed_number(typed: &str) -> Option<f64> {
    let cleaned: String = typed.trim().chars().filter(|c| !c.is_whitespace() && *c != '\u{202f}').collect();
    cleaned.replace(',', ".").parse::<f64>().ok().filter(|number| number.is_finite())
}

/// A number without needless decimals, with a comma: `30`, `5,5`.
fn plain(number: f64) -> String {
    let text = format!("{number:.2}");
    text.trim_end_matches('0').trim_end_matches('.').replace('.', ",")
}

/// Whether two stored values are the same setting: `30` and `30.0` are.
pub fn same(a: &Value, b: &Value) -> bool {
    match (a.as_f64(), b.as_f64()) {
        (Some(a), Some(b)) => a == b,
        _ => a == b,
    }
}

#[cfg(test)]
mod tests {
    // Not `super::*`: the page next door imports GPUI, whose `test` attribute would take over.
    use serde_json::{Value, json};

    use super::{CATEGORIES, Kind, SETTINGS, category_by_id, def_by_key, same, settings_for};

    fn def(key: &str) -> &'static super::Def {
        def_by_key(key).unwrap_or_else(|| panic!("no setting {key}"))
    }

    /// What a field shows before anything is saved must be what applies: a default that only
    /// the screen knows looks filled in while the service reports it missing.
    #[test]
    fn a_field_nothing_is_saved_for_shows_what_applies() {
        // Nobody has said what the business is: the field asks.
        assert_eq!(def("legal.legal_form").default_value(), json!(""));
        assert!(matches!(def("legal.legal_form").kind, Kind::Choice(options) if options[0].0.is_empty()));
        // The service applies this one until it is worded otherwise.
        assert_eq!(def("vat.exemption_text").default_value(), json!(motodesk_fiscal::vat::FRANCHISE_MENTION));
        // Nothing prints a country nobody typed.
        assert_eq!(def("contact.country").default_value(), json!(""));
    }

    #[test]
    fn every_setting_has_its_own_key_and_a_known_category() {
        let mut keys: Vec<&str> = SETTINGS.iter().map(|def| def.key).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), SETTINGS.len());
        for def in &SETTINGS {
            assert!(category_by_id(def.category).is_some(), "{} is in no category", def.key);
        }
    }

    #[test]
    fn a_category_drawn_as_a_grid_has_fields_and_a_special_one_draws_itself() {
        for category in CATEGORIES.iter().filter(|category| !category.special) {
            assert!(settings_for(category.id).count() > 0, "{} is empty", category.id);
        }
        assert_eq!(CATEGORIES.iter().filter(|category| category.special).count(), 5);
        // Everyone may change how the app looks on their own PC.
        assert!(!category_by_id("preferences").unwrap().admin_only);
    }

    #[test]
    fn a_default_is_of_the_kind_of_its_field() {
        for def in &SETTINGS {
            let default = def.default_value();
            match def.kind {
                Kind::Toggle => assert!(default.is_boolean(), "{}", def.key),
                Kind::Number | Kind::Cents | Kind::Percent => assert!(default.is_number(), "{}", def.key),
                Kind::Choice(options) => {
                    assert!(options.iter().any(|(value, _)| Some(*value) == default.as_str()), "{}", def.key)
                }
                Kind::Colour(_) => assert!(def.read(default.as_str().unwrap()).is_ok(), "{}", def.key),
                Kind::Text | Kind::Area => assert!(default.is_string(), "{}", def.key),
            }
            // What is shown for the default reads back as the default.
            if !matches!(def.kind, Kind::Toggle) {
                let back = def.read(&def.shown(&default)).unwrap();
                assert!(same(&back, &default), "{}: {back} is not {default}", def.key);
            }
        }
    }

    #[test]
    fn amounts_and_rates_are_typed_in_their_unit_and_stored_in_the_smallest() {
        let rate = def("workshop.labor_rate_ht_cents");
        assert_eq!(rate.shown(&json!(5500)), "55,00");
        assert_eq!(rate.read("62,5").unwrap(), json!(6250));
        assert_eq!(rate.read("62.50").unwrap(), json!(6250));
        assert_eq!(rate.read("").unwrap(), json!(0));

        let vat = def("vat.default_rate_bp");
        assert_eq!(vat.shown(&json!(2000)), "20");
        assert_eq!(vat.shown(&json!(550)), "5,5");
        assert_eq!(vat.read("5,5").unwrap(), json!(550));
        assert_eq!(vat.read("100").unwrap(), json!(10_000));
    }

    #[test]
    fn a_number_out_of_bounds_or_not_a_number_is_refused_with_the_bounds() {
        let terms = def("invoicing.payment_terms_days");
        assert_eq!(terms.read("45").unwrap(), json!(45));
        assert_eq!(terms.read("366").unwrap_err(), "Entre 0 et 365.");
        assert_eq!(terms.read("-1").unwrap_err(), "Entre 0 et 365.");
        assert_eq!(terms.read("un mois").unwrap_err(), "Nombre attendu.");

        assert_eq!(def("vat.default_rate_bp").read("120").unwrap_err(), "Entre 0 et 100.");
        assert_eq!(def("legal.capital_eur").read("-5").unwrap_err(), "0 au minimum.");
        assert_eq!(def("legal.capital_eur").read("7 500").unwrap(), json!(7500));
        assert_eq!(def("system.backup_frequency_days").read("0").unwrap_err(), "Entre 1 et 90.");
    }

    #[test]
    fn a_colour_is_six_hex_digits() {
        let accent = def("system.accent_color");
        assert_eq!(accent.read(" #C2410C ").unwrap(), json!("#c2410c"));
        assert_eq!(accent.read("#abc").unwrap(), json!("#aabbcc"));
        assert!(accent.read("bleu").is_err());
        assert!(accent.read("").is_err());
    }

    #[test]
    fn text_is_stored_as_typed_and_whole_numbers_compare_equal_however_written() {
        assert_eq!(def("legal.siret").read("123 456").unwrap(), json!("123 456"));
        assert!(same(&json!(30), &json!(30.0)));
        assert!(!same(&json!(30), &json!(31)));
        assert!(same(&json!("EI"), &json!("EI")));
        assert!(!same(&json!("30"), &json!(30)));
        assert!(!same(&Value::Null, &json!(false)));
    }
}
