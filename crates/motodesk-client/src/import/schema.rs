//! What can be imported, field by field (port of `desktop/src/lib/import/schema.ts`): the
//! columns a file can fill, their type (which drives how a value is read and checked), and
//! the fields that can tell an existing row from a new one.
//!
//! Field keys are the ones the service reads (`local_server/routes/import`).

/// The table a reference points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefTarget {
    Clients,
    Products,
    Categories,
    Vehicles,
    Sales,
}

impl RefTarget {
    /// The name of the table, as the mapping step shows it.
    pub fn table(self) -> &'static str {
        match self {
            RefTarget::Clients => "clients",
            RefTarget::Products => "products",
            RefTarget::Categories => "categories",
            RefTarget::Vehicles => "vehicles",
            RefTarget::Sales => "sales",
        }
    }

    /// The columns of that table a value can be looked up by, as `(key, label)`. The first is
    /// the default; `id` is the row's own identifier.
    pub fn lookups(self) -> &'static [(&'static str, &'static str)] {
        const ID: (&str, &str) = ("id", "Identifiant (ID)");
        match self {
            RefTarget::Clients => &[
                ("email_or_phone", "E-mail ou téléphone"),
                ID,
                ("email", "E-mail"),
                ("phone", "Téléphone"),
                ("last_name", "Nom de famille"),
            ],
            RefTarget::Products => &[("sku", "Référence (SKU)"), ID, ("name", "Nom")],
            RefTarget::Categories => &[("name", "Nom"), ID],
            RefTarget::Vehicles => &[("vin_or_serial", "N° de série / VIN"), ID, ("plate_number", "Immatriculation")],
            RefTarget::Sales => &[("reference", "Référence de la vente"), ID],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FieldType {
    Text,
    Integer,
    /// Typed in euros, stored in cents.
    Money,
    /// Typed as a quantity, stored in thousandths.
    Quantity,
    /// Typed in percent, stored in basis points.
    Rate,
    Flag,
    /// One of these `(stored value, label)`.
    Choice(&'static [(&'static str, &'static str)]),
    /// A row of another table, found by one of its columns.
    Reference(RefTarget),
}

impl FieldType {
    /// The few words the mapping step shows under a field.
    pub fn label(self) -> &'static str {
        match self {
            FieldType::Text => "texte",
            FieldType::Integer => "entier",
            FieldType::Money => "montant (€)",
            FieldType::Quantity => "quantité",
            FieldType::Rate => "taux (%)",
            FieldType::Flag => "oui / non",
            FieldType::Choice(_) => "liste",
            FieldType::Reference(_) => "référence",
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct Field {
    pub key: &'static str,
    pub label: &'static str,
    pub kind: FieldType,
    pub required: bool,
    pub unique: bool,
    /// A line of help for the mapping step; empty when the label says it all.
    pub hint: &'static str,
}

impl Field {
    /// The table this field points at, when it is a reference.
    pub fn reference(&self) -> Option<RefTarget> {
        match self.kind {
            FieldType::Reference(target) => Some(target),
            _ => None,
        }
    }

    /// The column a reference is looked up by when nothing says otherwise.
    pub fn default_lookup(&self) -> &'static str {
        self.reference().and_then(|target| target.lookups().first()).map_or("id", |(key, _)| key)
    }
}

const fn field(key: &'static str, label: &'static str, kind: FieldType) -> Field {
    Field { key, label, kind, required: false, unique: false, hint: "" }
}

const EUROS: &str = "En euros — converti en centimes.";

const fn money(key: &'static str, label: &'static str) -> Field {
    Field { hint: EUROS, ..field(key, label, FieldType::Money) }
}

#[derive(Debug, PartialEq)]
pub struct Entity {
    pub id: &'static str,
    pub label: &'static str,
    /// One line for the step where the destination is chosen.
    pub description: &'static str,
    pub fields: &'static [Field],
    /// The fields that can tell an existing row from a new one. None: rows are only ever added.
    pub match_keys: &'static [&'static str],
}

impl Entity {
    pub fn field(&self, key: &str) -> Option<&'static Field> {
        self.fields.iter().find(|field| field.key == key)
    }

    /// The label of a field, or its key when the entity has no such field.
    pub fn label_of<'a>(&self, key: &'a str) -> &'a str
    where
        'static: 'a,
    {
        self.field(key).map_or(key, |field| field.label)
    }
}

const PRODUCT_UNITS: &[(&str, &str)] = &[
    ("piece", "Pièce"),
    ("liter", "Litre"),
    ("meter", "Mètre"),
    ("kg", "Kilogramme"),
    ("hour", "Heure"),
    ("service", "Service"),
];

const VEHICLE_TYPES: &[(&str, &str)] = &[("moto", "Moto"), ("bicycle", "Vélo"), ("electric", "Électrique")];

const SALE_STATUSES: &[(&str, &str)] = &[
    ("draft", "Brouillon"),
    ("quote", "Devis"),
    ("invoice", "Facture"),
    ("paid", "Payée"),
    ("cancelled", "Annulée"),
];

const CLIENTS: Entity = Entity {
    id: "clients",
    label: "Clients",
    description: "Fiches clients (nom de famille requis).",
    match_keys: &["email", "phone", "last_name"],
    fields: &[
        Field {
            hint: "Optionnel. S'il est fourni, il est conservé — c'est ce qui relie les véhicules et les ventes ensuite.",
            ..field("id", "Identifiant (ID)", FieldType::Text)
        },
        field("first_name", "Prénom", FieldType::Text),
        Field { required: true, ..field("last_name", "Nom", FieldType::Text) },
        field("phone", "Téléphone", FieldType::Text),
        field("email", "E-mail", FieldType::Text),
        field("address", "Adresse", FieldType::Text),
        field("notes", "Notes", FieldType::Text),
    ],
};

const CATEGORIES: Entity = Entity {
    id: "categories",
    label: "Catégories",
    description: "Catégories de produits (nom unique).",
    match_keys: &["name"],
    fields: &[
        Field {
            hint: "Optionnel. S'il est fourni (UUID), il est conservé — c'est ce qui relie les produits importés ensuite.",
            ..field("id", "Identifiant (ID)", FieldType::Text)
        },
        Field { required: true, unique: true, ..field("name", "Nom", FieldType::Text) },
        Field { hint: "Ordre d'affichage.", ..field("position", "Position", FieldType::Integer) },
    ],
};

const PRODUCTS: Entity = Entity {
    id: "products",
    label: "Produits",
    description: "Articles et pièces (référence SKU unique, prix en euros).",
    match_keys: &["sku", "name"],
    fields: &[
        Field { required: true, ..field("name", "Nom", FieldType::Text) },
        Field {
            hint: "Rattachée à la table des catégories. Colonne `category` = nom ; `category_id` = identifiant.",
            ..field("category", "Catégorie", FieldType::Reference(RefTarget::Categories))
        },
        Field { unique: true, ..field("sku", "Référence (SKU)", FieldType::Text) },
        field("brand", "Marque", FieldType::Text),
        field("description", "Description", FieldType::Text),
        field("unit", "Unité", FieldType::Choice(PRODUCT_UNITS)),
        field("is_service", "Service", FieldType::Flag),
        money("purchase_price_ht_cents", "Prix d'achat HT"),
        money("selling_price_ttc_cents", "Prix de vente TTC"),
        Field {
            hint: "20 ou 5,5 ; une valeur déjà en points de base (2000) est reconnue.",
            ..field("vat_rate_bp", "TVA (%)", FieldType::Rate)
        },
        Field { hint: "2,5 ou 12 ; les décimales sont conservées.", ..field("stock_milli", "Stock", FieldType::Quantity) },
        field("low_stock_threshold", "Seuil stock bas", FieldType::Integer),
        field("is_active", "Actif", FieldType::Flag),
    ],
};

const VEHICLES: Entity = Entity {
    id: "vehicles",
    label: "Véhicules",
    description: "Véhicules (n° de série unique, propriétaire rattaché par e-mail/téléphone).",
    match_keys: &["vin_or_serial"],
    fields: &[
        Field {
            hint: "Rattaché à la table des clients. Colonne `owner` = e-mail ou téléphone ; `client_id` = identifiant.",
            ..field("owner", "Propriétaire", FieldType::Reference(RefTarget::Clients))
        },
        field("vehicle_type", "Type", FieldType::Choice(VEHICLE_TYPES)),
        Field { required: true, ..field("make", "Marque", FieldType::Text) },
        Field { required: true, ..field("model", "Modèle", FieldType::Text) },
        field("year", "Année", FieldType::Integer),
        field("cylinder_capacity", "Cylindrée (cc)", FieldType::Integer),
        Field { unique: true, ..field("vin_or_serial", "N° de série / VIN", FieldType::Text) },
        field("plate_number", "Immatriculation", FieldType::Text),
        money("purchase_price_ht_cents", "Prix d'achat HT"),
        money("selling_price_ttc_cents", "Prix de vente TTC"),
        field("notes", "Notes", FieldType::Text),
    ],
};

const SALES: Entity = Entity {
    id: "sales",
    label: "Ventes",
    description: "Tickets / factures (en-têtes). Reliez le client et le véhicule à leur fiche.",
    match_keys: &["reference"],
    fields: &[
        Field {
            unique: true,
            hint: "N° de ticket / facture. Sert de clé pour rattacher les lignes.",
            ..field("reference", "Référence", FieldType::Text)
        },
        Field {
            hint: "Rattaché à la table des clients. Colonne `client` = e-mail ou téléphone (comme le propriétaire d'un véhicule) ; `client_id` = identifiant.",
            ..field("client", "Client", FieldType::Reference(RefTarget::Clients))
        },
        Field {
            hint: "Rattaché à la table des véhicules. Colonne `vehicle` = n° de série ou immatriculation ; `vehicle_id` = identifiant.",
            ..field("vehicle", "Véhicule", FieldType::Reference(RefTarget::Vehicles))
        },
        field("status", "Statut", FieldType::Choice(SALE_STATUSES)),
        Field { hint: "Date ou date/heure de la vente.", ..field("sale_date", "Date", FieldType::Text) },
        money("total_ht_cents", "Total HT"),
        money("total_ttc_cents", "Total TTC"),
        field("payment_method", "Paiement", FieldType::Text),
        field("notes", "Notes", FieldType::Text),
    ],
};

const SALE_LINES: Entity = Entity {
    id: "sale_lines",
    label: "Lignes de vente",
    description: "Articles vendus. Reliez chaque ligne à sa vente et à son produit (par index/référence).",
    match_keys: &[],
    fields: &[
        Field {
            required: true,
            hint: "Rattachée à la table des ventes. Colonne `sale` = référence (`684@2004-12-24`) ; `sale_id` = identifiant.",
            ..field("sale", "Vente", FieldType::Reference(RefTarget::Sales))
        },
        Field {
            hint: "Rattaché à la table des produits. Colonne `product` = SKU ; `product_id` = identifiant.",
            ..field("product", "Produit", FieldType::Reference(RefTarget::Products))
        },
        Field { hint: "Libellé affiché sur la ligne.", ..field("label", "Désignation", FieldType::Text) },
        Field { hint: "Décimales acceptées (ex. 2,5).", ..field("quantity_milli", "Quantité", FieldType::Quantity) },
        money("unit_price_ht_cents", "Prix unitaire HT"),
        money("unit_price_ttc_cents", "Prix unitaire TTC"),
        Field { hint: "20 ou 5,5 ; points de base (2000) reconnus.", ..field("vat_rate_bp", "TVA (%)", FieldType::Rate) },
        Field { hint: "5 ou 12,5 ; points de base (500) reconnus.", ..field("discount_bp", "Remise (%)", FieldType::Rate) },
        money("line_total_ttc_cents", "Total ligne TTC"),
        Field { hint: "Ordre d'affichage.", ..field("position", "Position", FieldType::Integer) },
    ],
};

/// Every kind of data a file can bring in, in the order the wizard offers them.
pub static ENTITIES: [Entity; 6] = [CLIENTS, CATEGORIES, PRODUCTS, VEHICLES, SALES, SALE_LINES];

pub fn entity(id: &str) -> Option<&'static Entity> {
    ENTITIES.iter().find(|entity| entity.id == id)
}

/// What happens to a row that matches one already there, and to one that does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    #[default]
    Insert,
    Update,
    Upsert,
}

impl Mode {
    pub const ALL: [Mode; 3] = [Mode::Insert, Mode::Update, Mode::Upsert];

    pub fn id(self) -> &'static str {
        match self {
            Mode::Insert => "insert",
            Mode::Update => "update",
            Mode::Upsert => "upsert",
        }
    }

    pub fn from_id(id: &str) -> Option<Mode> {
        Self::ALL.into_iter().find(|mode| mode.id() == id)
    }

    pub fn label(self) -> &'static str {
        match self {
            Mode::Insert => "Ajouter uniquement",
            Mode::Update => "Mettre à jour uniquement",
            Mode::Upsert => "Ajouter ou mettre à jour",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Mode::Insert => "Crée de nouvelles lignes ; ignore les correspondances existantes.",
            Mode::Update => "Met à jour les lignes existantes ; ignore les nouvelles.",
            Mode::Upsert => "Met à jour si une correspondance existe, sinon crée la ligne.",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_entity_is_found_by_its_id_and_its_fields_are_named_once() {
        for listed in &ENTITIES {
            assert_eq!(entity(listed.id).map(|e| e.label), Some(listed.label));
            let mut keys: Vec<&str> = listed.fields.iter().map(|f| f.key).collect();
            keys.sort_unstable();
            keys.dedup();
            assert_eq!(keys.len(), listed.fields.len(), "{}", listed.id);
        }
        assert!(entity("invoices").is_none());
    }

    #[test]
    fn a_match_key_is_a_field_of_its_entity() {
        for listed in &ENTITIES {
            for key in listed.match_keys {
                assert!(listed.field(key).is_some(), "{} has no field {key}", listed.id);
            }
        }
        // Sale lines are only ever added.
        assert!(entity("sale_lines").unwrap().match_keys.is_empty());
    }

    #[test]
    fn a_reference_is_looked_up_by_the_natural_key_of_its_table_unless_told_otherwise() {
        let owner = entity("vehicles").unwrap().field("owner").unwrap();
        assert_eq!(owner.reference(), Some(RefTarget::Clients));
        assert_eq!(owner.default_lookup(), "email_or_phone");
        assert_eq!(entity("sale_lines").unwrap().field("product").unwrap().default_lookup(), "sku");
        // Not a reference: nothing to look up.
        assert_eq!(entity("clients").unwrap().field("email").unwrap().default_lookup(), "id");
        // Every table can be looked up by its identifier.
        for target in [RefTarget::Clients, RefTarget::Products, RefTarget::Categories, RefTarget::Vehicles, RefTarget::Sales] {
            assert!(target.lookups().iter().any(|(key, _)| *key == "id"), "{}", target.table());
        }
    }

    #[test]
    fn a_mode_is_sent_by_its_id() {
        assert_eq!(serde_json::to_string(&Mode::Upsert).unwrap(), "\"upsert\"");
        assert_eq!(Mode::from_id("update"), Some(Mode::Update));
        assert_eq!(Mode::from_id("replace"), None);
    }
}
