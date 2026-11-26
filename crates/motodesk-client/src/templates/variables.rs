//! The variables a template can show, by group and by kind of template. A plain variable
//! goes in a text (or as the source of a picture); a list feeds a table.

use super::schema::Format;
use super::TemplateKind;

use TemplateKind::{BonLivraison, Devis, Facture};

pub struct Variable {
    pub group: &'static str,
    /// Where the value is in the document's data.
    pub path: &'static str,
    pub label: &'static str,
    /// What is written in the template: the path, with how to write the value when it matters.
    pub token: &'static str,
    /// The kinds of template that have it. Empty: all of them.
    pub kinds: &'static [TemplateKind],
}

const fn var(group: &'static str, path: &'static str, label: &'static str, token: &'static str) -> Variable {
    Variable { group, path, label, token, kinds: &[] }
}

const fn only(kinds: &'static [TemplateKind], variable: Variable) -> Variable {
    Variable { kinds, ..variable }
}

const COMMERCIAL: &[TemplateKind] = &[Facture, Devis, BonLivraison];

pub const SCALARS: &[Variable] = &[
    var("Boutique", "shop.name", "Nom commercial", "{{shop.name}}"),
    var("Boutique", "shop.legal_name", "Raison sociale", "{{shop.legal_name}}"),
    var("Boutique", "shop.tagline", "Slogan", "{{shop.tagline}}"),
    var("Boutique", "shop.logo_url", "Logo", "{{shop.logo_url}}"),
    var("Contact", "contact.address_line1", "Adresse", "{{contact.address_line1}}"),
    var("Contact", "contact.address_line2", "Complément d'adresse", "{{contact.address_line2}}"),
    var("Contact", "contact.postal_code", "Code postal", "{{contact.postal_code}}"),
    var("Contact", "contact.city", "Ville", "{{contact.city}}"),
    var("Contact", "contact.country", "Pays", "{{contact.country}}"),
    var("Contact", "contact.phone", "Téléphone", "{{contact.phone}}"),
    var("Contact", "contact.email", "Email", "{{contact.email}}"),
    var("Contact", "contact.website", "Site web", "{{contact.website}}"),
    var("Légal", "legal.siret", "SIRET", "{{legal.siret}}"),
    var("Légal", "legal.vat_number", "N° TVA intracom.", "{{legal.vat_number}}"),
    var("Légal", "legal.ape_code", "Code APE / NAF", "{{legal.ape_code}}"),
    var("Banque", "invoicing.bank_name", "Banque", "{{invoicing.bank_name}}"),
    var("Banque", "invoicing.bank_iban", "IBAN", "{{invoicing.bank_iban}}"),
    var("Banque", "invoicing.bank_bic", "BIC / SWIFT", "{{invoicing.bank_bic}}"),
    var("Banque", "invoicing.footer_text", "Mentions de pied", "{{invoicing.footer_text}}"),
    var("Document", "doc.title", "Titre (FACTURE…)", "{{doc.title}}"),
    var("Document", "doc.number", "Numéro", "{{doc.number}}"),
    var("Document", "doc.date", "Date", "{{doc.date | date}}"),
    only(&[Facture, Devis], var("Document", "doc.due_date", "Date d'échéance", "{{doc.due_date | date}}")),
    var("Document", "doc.notes", "Notes", "{{doc.notes}}"),
    var("Document", "doc.payment_terms", "Conditions de paiement", "{{doc.payment_terms}}"),
    var("Document", "doc.total_ht", "Total HT", "{{doc.total_ht | price}}"),
    var("Document", "doc.total_tva", "Total TVA", "{{doc.total_tva | price}}"),
    var("Document", "doc.total_ttc", "Total TTC", "{{doc.total_ttc | price}}"),
    only(&[Facture], var("Document", "doc.outstanding", "Reste à payer", "{{doc.outstanding | price}}")),
    var("Client", "client.full_name", "Nom complet", "{{client.full_name}}"),
    var("Client", "client.first_name", "Prénom", "{{client.first_name}}"),
    var("Client", "client.last_name", "Nom", "{{client.last_name}}"),
    var("Client", "client.address", "Adresse", "{{client.address}}"),
    var("Client", "client.phone", "Téléphone", "{{client.phone}}"),
    var("Client", "client.email", "Email", "{{client.email}}"),
    only(COMMERCIAL, var("Véhicule", "vehicle.label", "Désignation", "{{vehicle.label}}")),
    only(COMMERCIAL, var("Véhicule", "vehicle.plate_number", "Immatriculation", "{{vehicle.plate_number}}")),
    only(COMMERCIAL, var("Véhicule", "vehicle.vin_or_serial", "VIN / N° série", "{{vehicle.vin_or_serial}}")),
    only(COMMERCIAL, var("Véhicule", "vehicle.odometer", "Compteur", "{{vehicle.odometer}}")),
    var("Produit", "product.name", "Désignation", "{{product.name}}"),
    var("Produit", "product.sku", "Référence", "{{product.sku}}"),
    var("Produit", "product.brand", "Marque", "{{product.brand}}"),
    var("Produit", "product.category", "Catégorie", "{{product.category}}"),
    var("Produit", "product.unit", "Unité", "{{product.unit}}"),
    var("Produit", "product.description", "Description", "{{product.description}}"),
    var("Produit", "product.notes", "Notes", "{{product.notes}}"),
    var("Produit", "product.price_ht", "Prix HT", "{{product.price_ht | price}}"),
    var("Produit", "product.price_ttc", "Prix TTC", "{{product.price_ttc | price}}"),
    var("Produit", "product.vat_rate", "TVA", "{{product.vat_rate | percent}}"),
    var("Produit", "product.stock", "Stock", "{{product.stock}}"),
];

/// One field of the rows of a list: what a table column can show.
pub struct ListField {
    pub binding: &'static str,
    pub label: &'static str,
    pub format: Format,
}

pub struct ListVariable {
    pub path: &'static str,
    pub label: &'static str,
    pub kinds: &'static [TemplateKind],
    pub fields: &'static [ListField],
}

const fn field(binding: &'static str, label: &'static str, format: Format) -> ListField {
    ListField { binding, label, format }
}

pub const LISTS: &[ListVariable] = &[
    ListVariable {
        path: "items",
        label: "Lignes du document",
        kinds: &[],
        fields: &[
            field("label", "Désignation", Format::Text),
            field("qty", "Quantité", Format::Qty),
            field("unit", "Unité", Format::Text),
            field("price_ht", "PU HT", Format::Price),
            field("vat_rate", "TVA", Format::Percent),
            field("total_ht", "Total HT", Format::Price),
            field("total_ttc", "Total TTC", Format::Price),
        ],
    },
    ListVariable {
        path: "vehicles",
        label: "Véhicules du client",
        kinds: &[],
        fields: &[
            field("label", "Désignation", Format::Text),
            field("plate_number", "Immatriculation", Format::Text),
            field("vin_or_serial", "VIN / n° série", Format::Text),
            field("odometer", "Compteur", Format::Text),
        ],
    },
    ListVariable {
        path: "payments",
        label: "Règlements",
        kinds: &[Facture],
        fields: &[field("date", "Date", Format::Text), field("method", "Moyen", Format::Text), field("amount", "Montant", Format::Price)],
    },
];

fn offered(kinds: &[TemplateKind], kind: TemplateKind) -> bool {
    kinds.is_empty() || kinds.contains(&kind)
}

/// The plain variables a template of this kind can show, by group, in the order they are declared.
pub fn groups_for(kind: TemplateKind) -> Vec<(&'static str, Vec<&'static Variable>)> {
    let mut groups: Vec<(&'static str, Vec<&'static Variable>)> = Vec::new();
    for variable in SCALARS.iter().filter(|variable| offered(variable.kinds, kind)) {
        match groups.iter_mut().find(|(group, _)| *group == variable.group) {
            Some((_, members)) => members.push(variable),
            None => groups.push((variable.group, vec![variable])),
        }
    }
    groups
}

pub fn lists_for(kind: TemplateKind) -> Vec<&'static ListVariable> {
    LISTS.iter().filter(|list| offered(list.kinds, kind)).collect()
}

pub fn list(path: &str) -> Option<&'static ListVariable> {
    LISTS.iter().find(|list| list.path == path)
}

/// What a variable is called in the editor: `client.last_name` reads « Nom (Client) ». A path
/// that is not in the catalogue is named after its last part.
pub fn human_label(path: &str) -> String {
    if let Some(variable) = SCALARS.iter().find(|variable| variable.path == path) {
        return format!("{} ({})", variable.label, variable.group);
    }
    let tail = path.rsplit('.').next().unwrap_or(path);
    let mut out = String::new();
    let mut capital = true;
    for c in tail.chars() {
        let c = if c == '_' { ' ' } else { c };
        out.extend(if capital { c.to_uppercase().collect::<Vec<_>>() } else { vec![c] });
        capital = c == ' ';
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_template_is_offered_the_variables_of_its_kind() {
        let paths = |kind| groups_for(kind).into_iter().flat_map(|(_, members)| members).map(|v| v.path).collect::<Vec<_>>();
        assert!(paths(Facture).contains(&"doc.outstanding"));
        assert!(!paths(Devis).contains(&"doc.outstanding"));
        assert!(paths(Devis).contains(&"doc.due_date"));
        assert!(!paths(BonLivraison).contains(&"doc.due_date"));
        assert!(!paths(TemplateKind::Autre).contains(&"vehicle.label"));
        assert!(paths(TemplateKind::Autre).contains(&"product.name"));

        let groups: Vec<&str> = groups_for(Facture).into_iter().map(|(group, _)| group).collect();
        assert_eq!(groups, ["Boutique", "Contact", "Légal", "Banque", "Document", "Client", "Véhicule", "Produit"]);
        assert_eq!(lists_for(Devis).iter().map(|l| l.path).collect::<Vec<_>>(), ["items", "vehicles"]);
        assert_eq!(lists_for(Facture).len(), 3);
    }

    #[test]
    fn a_variable_is_named_for_people() {
        assert_eq!(human_label("client.last_name"), "Nom (Client)");
        assert_eq!(human_label("doc.total_ttc"), "Total TTC (Document)");
        assert_eq!(human_label("product.low_stock"), "Low Stock");
        assert_eq!(human_label("note"), "Note");
    }

    #[test]
    fn every_token_names_its_own_path() {
        for variable in SCALARS {
            assert_eq!(super::super::interpolate::bare_variable(variable.token).as_deref(), Some(variable.path), "{}", variable.token);
        }
        assert_eq!(list("items").map(|l| l.fields.len()), Some(7));
        assert!(list("inconnue").is_none());
    }
}
