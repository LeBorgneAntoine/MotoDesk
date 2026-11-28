//! Every component of the kit on one page, in both themes and the three densities.
//!
//! `cargo run -p motodesk-ui --example gallery`
//!
//! This is where a component is looked at before a screen uses it: add it here with every
//! variant and state when you add or change one. `--dark`, `--compact`, `--panel`, `--notices` and
//! `--section <name>` start it in a given state (used to take screenshots).

use std::rc::Rc;

use motodesk_ui::gpui::*;
use motodesk_ui::*;

struct Gallery {
    section: SharedString,
    name: TextModel,
    password: TextModel,
    notes: TextModel,
    search: TextModel,
    colour: TextModel,
    message: TextModel,
    vehicle_type: SelectModel,
    buyer: ComboModel,
    panels: Entity<PanelStack>,
    panel_note: TextModel,
    tab: SharedString,
    checked: bool,
    switched: bool,
    clicks: usize,
    notices_open: bool,
    selected_row: Option<usize>,
    page: usize,
    _subscriptions: Vec<Subscription>,
}

const ROWS: usize = 200;

fn person(row: usize) -> (&'static str, &'static str) {
    const FIRST: [&str; 8] = ["Léa", "Camille", "Noël", "Maëlle", "Jean", "Chloé", "Rémi", "Inès"];
    const LAST: [&str; 7] = ["Martin", "Le Gall", "Dubois", "Garçon", "Bernard", "Kervella", "Moreau"];
    (FIRST[row % FIRST.len()], LAST[(row / 3) % LAST.len()])
}

impl Gallery {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let args: Vec<String> = std::env::args().collect();
        let section = args.iter().position(|a| a == "--section").and_then(|i| args.get(i + 1)).cloned();
        let name = TextModel::builder().placeholder("Nom du client").build(window, cx);
        let on_name = name.on_event(window, cx, |_, _, _, cx| cx.notify());
        let panels = cx.new(PanelStack::new);
        let on_panels = cx.observe(&panels, |_, _, cx| cx.notify());
        // A key pressed anywhere runs the menu entry that advertises it, as in the application.
        // In a text field, copy, paste and undo stay the field's.
        let on_keys = cx.observe_keystrokes(|this, event, window, cx| {
            let Some(pressed) = pressed_shortcut(&event.keystroke, typing_in_a_field(window)) else { return };
            let menus = Self::menus(&cx.entity(), &this.panels);
            window.defer(cx, move |window, cx| {
                MenuGroup::run_shortcut(&menus, &pressed, window, cx);
            });
        });

        let buyer = ComboModel::new("Nom, téléphone…", window, cx);
        let for_query = buyer.clone();
        buyer.on_query(cx, move |query, _, cx| {
            // A real screen asks its service here; the gallery filters a fixed list.
            let query = query.to_lowercase();
            let matches = (0..ROWS)
                .map(|row| (row, person(row)))
                .filter(|(_, (first, last))| format!("{first} {last}").to_lowercase().contains(&query))
                .take(8)
                .map(|(row, (first, last))| {
                    ComboOption::new(row.to_string(), format!("{first} {last}")).detail(format!("CL-{:05}", 1000 + row))
                })
                .collect();
            for_query.set_options(matches, cx);
        });
        let on_buyer = cx.entity().downgrade();
        buyer.on_pick(cx, move |_, _, cx| {
            let _ = on_buyer.update(cx, |_, cx| cx.notify());
        });

        let this = Self {
            section: section.unwrap_or_else(|| "controls".into()).into(),
            name,
            password: TextModel::builder().placeholder("Mot de passe").password().build(window, cx),
            notes: TextModel::builder().placeholder("Notes").multiline(3).build(window, cx),
            search: TextModel::builder().placeholder("Rechercher un client…").build(window, cx),
            colour: TextModel::builder().placeholder("#1976d2").value("#1976d2").build(window, cx),
            message: TextModel::builder().placeholder("Demander à l'assistant…").composer(6).build(window, cx),
            vehicle_type: SelectModel::new(
                vec![
                    SelectOption::new("moto", "Moto"),
                    SelectOption::new("bicycle", "Vélo"),
                    SelectOption::new("electric", "Électrique"),
                ],
                Some("moto"),
                window,
                cx,
            ),
            buyer,
            panels,
            panel_note: TextModel::builder().placeholder("Téléphone").build(window, cx),
            tab: "details".into(),
            checked: true,
            switched: false,
            clicks: 0,
            notices_open: args.iter().any(|a| a == "--notices"),
            selected_row: Some(2),
            page: 0,
            _subscriptions: vec![on_name, on_panels, on_keys],
        };
        if args.iter().any(|a| a == "--panel") {
            Self::open_demo_panels(&this.panels, cx);
        }
        this
    }

    /// One notice of each kind: with a thing to do, with a way out, one that can be removed.
    fn notices() -> Vec<Notice> {
        let say = |text: &'static str| move |window: &mut Window, cx: &mut App| toast::info(text, window, cx);
        vec![
            Notice::new("gap", Status::Danger, "Base plus ancienne que le cloud")
                .body("Aucun document ne peut être émis tant que les modifications ne sont pas récupérées.")
                .action(NoticeAction::new("Récupérer", say("Récupération")).primary())
                .action(NoticeAction::new("Continuer sans récupérer", say("Ignoré"))),
            Notice::new("backup", Status::Warning, "Sauvegarde non copiée")
                .body("La dernière sauvegarde n'a pas été copiée hors de ce poste.")
                .action(NoticeAction::new("Ouvrir les sauvegardes", say("Sauvegardes")).primary().busy(true))
                .action(NoticeAction::new("Demain", say("Demain"))),
            Notice::new("done", Status::Success, "Export terminé").body("Le fichier FEC est prêt.").on_dismiss(say("Retiré")),
            Notice::new("till", Status::Info, "Caisse").body("Cet ordinateur n'est pas encore une caisse."),
        ]
    }

    /// The menus of the title bar, with every kind of entry: an icon, keys that run the entry
    /// from anywhere (`shortcut`), keys that are only named because the screen handles them
    /// itself (`hint`), an entry that cannot be used now, a separator.
    fn menus(gallery: &Entity<Self>, panels: &Entity<PanelStack>) -> Vec<MenuGroup> {
        let show = |section: &'static str| {
            let gallery = gallery.clone();
            move |_: &mut Window, cx: &mut App| {
                gallery.update(cx, |this, cx| {
                    this.section = section.into();
                    cx.notify();
                })
            }
        };
        let (to_open, to_close) = (panels.clone(), panels.clone());
        vec![
            MenuGroup::new(
                "Fichier",
                vec![
                    MenuEntry::new("Nouveau client", |window, cx| toast::success("« Nouveau client », depuis le menu ou Ctrl+N.", window, cx))
                        .icon(IconName::Plus)
                        .shortcut("Ctrl+N"),
                    MenuEntry::new("Imprimer…", |_, _| {}).icon(IconName::Printer).shortcut("Ctrl+P").disabled(true),
                    MenuEntry::separator(),
                    MenuEntry::new("Actualiser", |window, cx| toast::info("Actualisé.", window, cx)).icon(IconName::RefreshCw).shortcut("F5"),
                ],
            ),
            MenuGroup::new(
                "Affichage",
                vec![
                    MenuEntry::new("Contrôles", show("controls")).shortcut("Ctrl+1"),
                    MenuEntry::new("Contenu", show("content")).shortcut("Ctrl+2"),
                    MenuEntry::new("Table et panneaux", show("table")).shortcut("Ctrl+3"),
                    MenuEntry::separator(),
                    MenuEntry::new("Ouvrir les panneaux", move |_, cx| Self::open_demo_panels(&to_open, cx)).icon(IconName::Layers),
                    // Escape is the panel's own key: the menu names it, the panel handles it.
                    MenuEntry::new("Fermer le panneau", move |window, cx| request_close(&to_close, window, cx)).hint("Échap"),
                ],
            ),
        ]
    }

    fn open_demo_panels(panels: &Entity<PanelStack>, cx: &mut App) {
        panels.update(cx, |stack, cx| {
            stack.open(
                PanelLayer::new("client", "1", "Léa Martin").subtitle("Cliente depuis 2021").icon(IconName::CircleUser),
            );
            stack.open(
                PanelLayer::new("vehicle", "7", "Honda CB 500 F (2019)")
                    .subtitle("AB-123-CD")
                    .icon(IconName::Bike)
                    .width(PanelWidth::Sm),
            );
            cx.notify();
        });
    }

    fn block(title: &'static str, cx: &App) -> Div {
        col().gap(cx.ui().m.space_3).child(Heading::new(title).caps())
    }

    fn controls(&mut self, cx: &mut Context<Self>) -> Div {
        let theme = cx.ui().clone();
        let (c, m) = (theme.colors, theme.m);
        let count = cx.listener(|this, _: &ClickEvent, _, cx| {
            this.clicks += 1;
            cx.notify();
        });
        let entity = cx.entity();
        let (check, switch, tab) = (entity.clone(), entity.clone(), entity.clone());

        col()
            .gap(m.space_7)
            .child(
                Self::block("Button", cx)
                    .child(
                        row()
                            .gap(m.space_2)
                            .flex_wrap()
                            .child(Button::new("b1").primary().label("Enregistrer").on_click(count))
                            .child(Button::new("b2").label("Annuler"))
                            .child(Button::new("b3").ghost().icon(IconName::RefreshCw).label("Actualiser"))
                            .child(Button::new("b4").danger().icon(IconName::Trash).label("Supprimer"))
                            .child(Button::new("b5").link().label("Ouvrir les réglages"))
                            .child(Button::new("b6").primary().loading(true).label("Envoi"))
                            .child(Button::new("b7").disabled(true).label("Indisponible"))
                            .child(Button::new("b8").primary().disabled(true).label("Indisponible")),
                    )
                    .child(
                        row()
                            .gap(m.space_2)
                            .child(Button::new("s1").small().primary().label("Petit"))
                            .child(Button::new("s2").small().icon(IconName::Plus).label("Nouveau"))
                            .child(Button::new("s3").small().ghost().label("Plus tard").tooltip("Demander à nouveau demain"))
                            .child(IconButton::new("i1", IconName::Pencil).title("Modifier"))
                            .child(IconButton::new("i2", IconName::Trash).title("Supprimer").danger())
                            .child(IconButton::new("i3", IconName::Funnel).title("Filtrer").active(true))
                            .child(IconButton::new("i4", IconName::Copy).title("Copier").disabled(true))
                            .child(div().fs(m.fs_sm).text_color(c.text_muted).child(format!("{} clic(s)", self.clicks))),
                    ),
            )
            .child(
                Self::block("Input, Textarea, Select, SearchField, Combobox", cx)
                    .child(
                        row()
                            .gap(m.space_3)
                            .items_start()
                            .child(div().w(px(240.)).child(Input::new(&self.name).icon(IconName::User).clearable()))
                            .child(div().w(px(240.)).child(Input::new(&self.password).reveal_toggle()))
                            .child(div().w(px(180.)).child(Input::new(&self.name).invalid(true)))
                            .child(div().w(px(180.)).child(Input::new(&self.name).disabled(true)))
                            .child(div().w(px(180.)).child(Select::new(&self.vehicle_type))),
                    )
                    .child(
                        row()
                            .gap(m.space_3)
                            .items_start()
                            .child(div().w(px(492.)).child(Textarea::new(&self.notes)))
                            .child(SearchField::new(&self.search))
                            .child(div().w(px(260.)).child(Combobox::new(&self.buyer))),
                    )
                    .child(div().fs(m.fs_sm).text_color(c.text_muted).child(format!(
                        "Saisi : « {} » · type : {} · acheteur : {}",
                        self.name.text(cx),
                        self.vehicle_type.value(cx).unwrap_or_default(),
                        self.buyer.value(cx).unwrap_or_else(|| "aucun".into())
                    ))),
            )
            .child(
                Self::block("FormField, Checkbox, Switch", cx).child(
                    row()
                        .gap(m.space_6)
                        .items_start()
                        .child(div().w(px(240.)).child(FormField::new("Nom").required().child(Input::new(&self.name))))
                        .child(
                            div().w(px(240.)).child(
                                FormField::new("Téléphone").hint("Pour prévenir quand la moto est prête").child(Input::new(&self.search)),
                            ),
                        )
                        .child(
                            div().w(px(240.)).child(
                                FormField::new("Email").error(Some("Adresse invalide")).child(Input::new(&self.name).invalid(true)),
                            ),
                        )
                        .child(
                            col()
                                .gap(m.space_3)
                                .child(Checkbox::new("c1", self.checked).label("Bloquer le crédit").hint("Refuse toute nouvelle vente à ce client.").on_change(move |on, _, cx| {
                                    check.update(cx, |this, cx| {
                                        this.checked = on;
                                        cx.notify();
                                    })
                                }))
                                .child(Switch::new("w1", self.switched).label("Affichage compact").hint("Réduit l'espacement entre les éléments.").on_change(move |on, _, cx| {
                                    switch.update(cx, |this, cx| {
                                        this.switched = on;
                                        cx.notify();
                                    })
                                })),
                        ),
                ),
            )
            .child(
                Self::block("ColorField, Picture", cx).child(
                    row()
                        .gap(m.space_6)
                        .child(ColorField::new("accent", &self.colour).presets(&[0x1976d2, 0x00897b, 0x2e7d32, 0x546e7a]))
                        .child(div().w(px(160.)).child(ProgressBar::new(42)))
                        .child(Picture::new(None))
                        .child(Picture::new(picture_from_bytes(
                            br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><circle cx="12" cy="12" r="10" fill="#1976d2"/></svg>"##.to_vec(),
                        ))),
                ),
            )
            .child(
                Self::block("Stepper, DropZone", cx).child(
                    col()
                        .gap(m.space_4)
                        .child(
                            Stepper::new("steps", 2)
                                .step("Fichier", "CSV source")
                                .step("Destination", "Table et mode")
                                .step("Correspondance", "Colonnes → champs")
                                .step("Aperçu", "Contrôle avant import")
                                .step("Résultat", "Bilan")
                                .on_select(|_, _, _| {}),
                        )
                        .child(
                            DropZone::new("drop", "Glissez-déposez un fichier CSV ici")
                                .line("Formats acceptés : .csv · séparateur détecté automatiquement")
                                .action(Button::new("drop-pick").primary().icon(IconName::Upload).label("Choisir un fichier")),
                        ),
                ),
            )
            .child(
                Self::block("PageView", cx).child(
                    PageView::new("page", 105.0, 60.0, 3.0).grid(5.0).shapes(vec![
                        PageShape::Rect { x: 0.0, y: 0.0, w: 105.0, h: 12.0, fill: Some(0x1e3a5f), stroke: None, dashed: false },
                        PageShape::Text { x: 6.0, y: 3.0, text: "Garage Martin Moto".into(), size: 12.0, bold: true, color: 0xffffff },
                        PageShape::Badge { x: 6.0, y: 18.0, w: 34.0, h: 4.4, label: "Nom complet (Client)".into(), size: 9.0, bold: false },
                        PageShape::Rect { x: 60.0, y: 16.0, w: 39.0, h: 20.0, fill: Some(0xeef2f6), stroke: Some((0xd0d5db, 0.25)), dashed: false },
                        PageShape::Text { x: 63.0, y: 19.0, text: "Total TTC".into(), size: 9.0, bold: false, color: 0x5c6166 },
                        PageShape::Text { x: 80.0, y: 26.0, text: "378,48 €".into(), size: 11.0, bold: true, color: 0x1a1a1a },
                        PageShape::Line { x1: 6.0, y1: 42.0, x2: 99.0, y2: 42.0, color: 0x1a1a1a, width: 0.1 },
                        PageShape::Placeholder { x: 6.0, y: 26.0, w: 24.0, h: 12.0 },
                    ]),
                ),
            )
            .child(
                Self::block("Composer, Prose", cx).child(
                    row()
                        .gap(m.space_6)
                        .items_start()
                        .child(div().w(px(360.)).child(Composer::new("message", &self.message).on_send(|_, _| {})))
                        .child(div().w(px(420.)).child(Prose::new("prose", "## Révision des 600 km\n\nÀ prévoir : **vidange**, filtre et `tension de chaîne`.\n\n| Poste | Durée |\n| --- | ---: |\n| Vidange | 0,5 h |\n| Contrôles | 1 h |\n\n> Voir le module [Ventes](#/sales)."))),
                ),
            )
            .child(
                Self::block("PosKey, PosItem, PosCartLine", cx).child(
                    row()
                        .gap(m.space_6)
                        .items_start()
                        .child(
                            row()
                                .w(px(320.))
                                .gap(m.space_2)
                                .child(PosKey::new("pos-cash", "Espèces").icon(IconName::Banknote).primary().on_click(|_, _, _| {}))
                                .child(PosKey::new("pos-card", "Carte").icon(IconName::CreditCard).on_click(|_, _, _| {}))
                                .child(PosKey::new("pos-cheque", "Chèque").icon(IconName::FileText).disabled(true)),
                        )
                        .child(
                            col()
                                .w(px(320.))
                                .child(
                                    PosItem::new("pos-item", "Huile moteur 10W40", "14,99 €")
                                        .icon(IconName::Package)
                                        .meta("Motul · MOT-5100-1L")
                                        .badge("Stock 3", Status::Warning)
                                        .on_click(|_, _, _| {}),
                                )
                                .child(PosItem::new("pos-item-out", "Bougie CR8E", "6,50 €").icon(IconName::Package).badge("Rupture", Status::Danger).disabled(true))
                                .child(PosItem::new("pos-item-compact", "Kit chaîne 520", "89,00 €").icon(IconName::Package).compact().on_click(|_, _, _| {})),
                        )
                        .child(
                            col().w(px(380.)).child(
                                PosCartLine::new("pos-line", "Plaquettes de frein avant", "2", "89,82 €")
                                    .icon(IconName::Package)
                                    .meta("2 × 49,90 € TTC · −10 %")
                                    .on_minus(|_, _, _| {})
                                    .on_plus(|_, _, _| {})
                                    .on_discount(|_, _, _| {})
                                    .on_remove(|_, _, _| {}),
                            ),
                        ),
                ),
            )
            .child(
                Self::block("Tabs", cx).child(
                    Tabs::new(
                        "tabs",
                        vec![
                            TabItem::new("details", "Détails").icon(IconName::FileText),
                            TabItem::new("vehicles", "Véhicules").badge("3"),
                            TabItem::new("sales", "Ventes").badge("12"),
                        ],
                        self.tab.clone(),
                    )
                    .on_change(move |id, _, cx| {
                        let id = id.clone();
                        tab.update(cx, |this, cx| {
                            this.tab = id;
                            cx.notify();
                        })
                    }),
                ),
            )
    }

    fn content(&mut self, cx: &mut Context<Self>) -> Div {
        let theme = cx.ui().clone();
        let (c, m) = (theme.colors, theme.m);
        col()
            .gap(m.space_7)
            .child(
                Self::block("Heading, Tag, Icon", cx)
                    .child(
                        row()
                            .gap(m.space_5)
                            .items_baseline()
                            .child(Heading::new("Tableau de bord").size(HeadingSize::Xl3))
                            .child(Heading::new("Clients").size(HeadingSize::Xl2))
                            .child(Heading::new("Coordonnées").size(HeadingSize::Lg))
                            .child(Heading::new("Section").tone(Tone::Muted))
                            .child(Heading::new("Étiquette").caps()),
                    )
                    .child(
                        row()
                            .gap(m.space_2)
                            .child(Tag::new("Brouillon"))
                            .child(Tag::new("Archivé").status(Status::Muted))
                            .child(Tag::new("En cours").status(Status::Accent))
                            .child(Tag::new("Devis").status(Status::Info))
                            .child(Tag::new("Payée").status(Status::Success).icon(IconName::Check))
                            .child(Tag::new("En retard").status(Status::Warning))
                            .child(Tag::new("Bloqué").status(Status::Danger))
                            .child(Tag::new("3 véhicules").status(Status::Accent).medium()),
                    )
                    .child(
                        row()
                            .gap(m.space_4)
                            .child(Icon::new(IconName::Users).sm())
                            .child(Icon::new(IconName::Wrench))
                            .child(Icon::new(IconName::Package).lg())
                            .child(Icon::new(IconName::Receipt).xl())
                            .child(Icon::new(IconName::CircleAlert).lg().color(c.danger))
                            .child(Icon::new(IconName::CircleCheck).lg().color(c.success))
                            .child(Spinner::new().label("Chargement…")),
                    ),
            )
            .child(
                Self::block("Alert", cx)
                    .child(
                        Alert::warning().title("Profil fiscal incomplet").child("Le SIRET manque : les factures ne peuvent pas être validées.").child(
                            row().gap(m.space_3).mt(m.space_1).child(Button::new("a1").link().small().label("Ouvrir les réglages")),
                        ),
                    )
                    .child(Alert::info().child("La caisse n'est pas encore configurée."))
                    .child(Alert::success().title("Sauvegarde terminée").child("Fichier écrit il y a 2 minutes."))
                    .child(Alert::danger().title("Synchronisation refusée").child("3 changements ont été refusés par le cloud.")),
            )
            .child(
                Self::block("Card, SectionCard, InfoList, ListRow", cx).child(
                    row()
                        .gap(m.space_4)
                        .items_start()
                        .child(
                            div().w(px(360.)).child(
                                SectionCard::new()
                                    .title("Coordonnées")
                                    .subtitle("Visible sur les factures")
                                    .icon(IconName::IdCard)
                                    .action(IconButton::new("sc-edit", IconName::Pencil).title("Modifier"))
                                    .footer(Button::new("sc-cancel").small().label("Annuler"))
                                    .footer(Button::new("sc-save").small().primary().label("Enregistrer"))
                                    .child(InfoList::new(vec![
                                        InfoItem::new("Téléphone", Some("06 12 34 56 78")).icon(IconName::Phone),
                                        InfoItem::new("Email", Some("lea.martin@example.fr")).icon(IconName::Mail),
                                        InfoItem::new("N° de série", Some("JH2PC40A0KM000123")).mono(),
                                        InfoItem::new("Adresse", None::<&str>),
                                        InfoItem::new("Notes", Some("Préfère être appelée après 18 h. Pneus à surveiller au prochain passage.")).block(),
                                    ])),
                            ),
                        )
                        .child(
                            div().w(px(320.)).child(
                                Card::new().bordered().title("Véhicules").subtitle("2 au nom de ce client").padding(CardPadding::None).child(
                                    col()
                                        .child(ListRow::new("lr1", "Honda CB 500 F (2019)").meta("AB-123-CD · 12 500 km").icon(IconName::Bike).selected(true))
                                        .child(ListRow::new("lr2", "Peugeot Kisbee 50").meta("Série VGA…0042").icon(IconName::Bike).count("3", Some(IconName::Wrench)))
                                        .child(ListRow::new("lr3", "Camille Le Gall").meta("Mécanicienne").avatar("CL").trailing(Tag::new("Admin").status(Status::Accent))),
                                ),
                            ),
                        )
                        .child(
                            div().w(px(220.)).bg(c.bg_sidebar).border_1().border_color(c.border).child(
                                col()
                                    .child(ListRow::new("d1", "Toutes les catégories").dense().count("312", None).selected(true))
                                    .child(ListRow::new("d2", "Pneus").dense().count("48", None))
                                    .child(ListRow::new("d3", "Huiles et lubrifiants").dense().count("27", None)),
                            ),
                        ),
                ),
            )
            .child(
                Self::block("KpiTile", cx).child(
                    row()
                        .gap(m.space_3)
                        .child(KpiTile::new("Membres", "12").icon(IconName::UsersRound))
                        .child(KpiTile::new("Administrateurs", "2").icon(IconName::Shield).status(Status::Accent))
                        .child(KpiTile::new("Actifs", "11").icon(IconName::CircleCheck).status(Status::Success))
                        .child(KpiTile::new("Chiffre du jour", "1 284,50").unit("€").hint("7 ventes").icon(IconName::Receipt)),
                ),
            )
            .child(
                Self::block("KpiTile: a trend, a figure on its way", cx).child(
                    row()
                        .gap(m.space_3)
                        .child(KpiTile::new("Clients actifs", "137").icon(IconName::Users).status(Status::Accent).trend("+3", Trend::Up))
                        .child(KpiTile::new("Stock disponible", "812").icon(IconName::Package).trend("-12", Trend::Down))
                        .child(KpiTile::new("CA du mois", "").icon(IconName::Banknote).status(Status::Success).loading(true)),
                ),
            )
            .child(
                Self::block("Preformatted", cx).child(div().w(px(560.)).child(Preformatted::new(
                    "JournalCode|JournalLib|EcritureNum|EcritureDate|CompteNum|CompteLib|Debit|Credit\nVT|Ventes|FA-2026-CF60F-00001|20261105|411000|Clients|120,00|0,00\nVT|Ventes|FA-2026-CF60F-00001|20261105|706000|Prestations|0,00|100,00",
                ))),
            )
            .child(
                Self::block("TileButton", cx).child(
                    row()
                        .w(px(760.))
                        .border_1()
                        .border_color(c.border)
                        .bg(c.bg_surface)
                        .child(div().flex_1().min_w_0().child(
                            TileButton::new("tile-clients", "Clients & Véhicules")
                                .icon(IconName::Users)
                                .description("Recherche clients et parc véhicules.")
                                .on_click(|_, _, _| {}),
                        ))
                        .child(div().flex_1().min_w_0().child(
                            TileButton::new("tile-stock", "Inventaire")
                                .icon(IconName::Package)
                                .description("Inventaire, réceptions et mouvements.")
                                .divided(true)
                                .on_click(|_, _, _| {}),
                        ))
                        .child(div().flex_1().min_w_0().child(
                            TileButton::new("tile-soon", "Caisse")
                                .icon(IconName::ShoppingCart)
                                .badge("Bientôt")
                                .divided(true)
                                .disabled(true),
                        )),
                ),
            )
            .child(
                Self::block("DataTable, as tall as its rows, with a muted row", cx).child(
                    div().w(px(560.)).border_1().border_color(c.border).child(
                        DataTable::new(
                            "auto-table",
                            vec![Column::new("Compte").flex(2.), Column::new("Rôle").fixed(px(160.))],
                            3,
                            |row, column, _, _| match (row, column) {
                                (0, 0) => "Camille".into_any_element(),
                                (1, 0) => "Léa Martin".into_any_element(),
                                (_, 0) => "Noël (désactivé)".into_any_element(),
                                (0, _) => Tag::new("Admin").status(Status::Accent).into_any_element(),
                                _ => Tag::new("Employé").status(Status::Muted).into_any_element(),
                            },
                        )
                        .auto_height()
                        .muted_rows(|row| row == 2),
                    ),
                ),
            )
            .child(
                Self::block("StateView", cx).child(
                    row()
                        .gap(m.space_4)
                        .children([
                            StateView::loading().title("Chargement des clients").compact(),
                            StateView::empty().icon(IconName::Users).title("Aucun client").body("Créez votre premier client pour commencer.").compact(),
                            StateView::error().title("Chargement impossible").body("Le service de données ne répond pas.").action("Réessayer", |_, _, _| {}).compact(),
                        ]
                        .map(|view| div().w(px(280.)).h(px(220.)).bg(c.bg_surface).border_1().border_color(c.border).child(view))),
                ),
            )
    }

    fn table(&mut self, cx: &mut Context<Self>) -> Div {
        let theme = cx.ui().clone();
        let (c, m) = (theme.colors, theme.m);
        let (on_row, on_page) = (cx.entity(), cx.entity());
        let muted = c.text_muted;

        let table = DataTable::new(
            "demo-table",
            vec![
                Column::new("Nom").flex(2.),
                Column::new("Téléphone").flex(1.5),
                Column::new("Référence").fixed(px(140.)).mono(),
                Column::new("Véhicules").fixed(px(110.)).right(),
                Column::new("Statut").fixed(px(120.)),
            ],
            ROWS,
            move |row, column, _, _| {
                let (first, last) = person(row);
                match column {
                    0 => format!("{last} {first}").into_any_element(),
                    1 => div().text_color(muted).child(format!("06 {:02} {:02} {:02} {:02}", row % 97, (row * 7) % 97, (row * 13) % 97, (row * 31) % 97)).into_any_element(),
                    2 => format!("CL-{:05}", 1000 + row).into_any_element(),
                    3 => format!("{}", row % 4).into_any_element(),
                    _ => if row % 5 == 0 { Tag::new("Bloqué").status(Status::Danger) } else { Tag::new("Actif").status(Status::Success) }.into_any_element(),
                }
            },
        )
        .selected(self.selected_row)
        .on_row_click(move |row, _, cx| {
            let row = *row;
            on_row.update(cx, |this, cx| {
                this.selected_row = Some(row);
                cx.notify();
            })
        })
        .row_menu(|row, _, _| {
            vec![
                MenuEntry::new(format!("Ouvrir la ligne {}", row + 1), |_, _| {}).icon(IconName::Eye),
                MenuEntry::new("Modifier", |_, _| {}).icon(IconName::Pencil),
                MenuEntry::separator(),
                MenuEntry::new("Supprimer", |_, _| {}).icon(IconName::Trash),
            ]
        });

        col().gap(m.space_3).child(Heading::new("DataTable, Pagination (200 lignes, clic droit pour le menu)").caps()).child(
            col().h(px(420.)).border_1().border_color(c.border).child(div().flex_1().min_h_0().child(table)).child(
                Pagination::new(self.page, 50, ROWS).noun("clients").on_page(move |page, _, cx| {
                    on_page.update(cx, |this, cx| {
                        this.page = page;
                        cx.notify();
                    })
                }),
            ),
        )
    }

    fn overlays(&mut self, cx: &mut Context<Self>) -> Div {
        let m = cx.ui().m;
        let (panels, to_open) = (self.panels.clone(), self.panels.clone());
        col().gap(m.space_3).child(Heading::new("Panel, Confirm, Modal, toast").caps()).child(
            row()
                .gap(m.space_2)
                .child(Button::new("o1").icon(IconName::PanelRight).label("Ouvrir deux panneaux").on_click(move |_, _, cx| {
                    Self::open_demo_panels(&to_open, cx);
                }))
                .child(Button::new("o2").label("Confirmation").on_click(|_, window, cx| {
                    Confirm::new("Supprimer ce client ?", "Léa Martin et ses 2 véhicules seront supprimés. Cette action est définitive.")
                        .confirm_label("Supprimer")
                        .danger()
                        .open(window, cx, |window, cx| toast::success("Client supprimé.", window, cx));
                }))
                .child(Button::new("o3").label("Fenêtre modale").on_click(|_, window, cx| {
                    Modal::new("Nouvelle catégorie").open(
                        window,
                        cx,
                        |_, _| element("Le corps d'une fenêtre modale : un court formulaire ou un message."),
                        |_, _| {
                            vec![
                                element(Button::new("m-cancel").label("Annuler").on_click(|_, window, cx| close_modal(window, cx))),
                                element(Button::new("m-ok").primary().label("Créer").on_click(|_, window, cx| close_modal(window, cx))),
                            ]
                        },
                    );
                }))
                .child(Button::new("o4").label("Toast info").on_click(|_, window, cx| toast::info("Synchronisation en cours…", window, cx)))
                .child(Button::new("o5").label("Toast succès").on_click(|_, window, cx| toast::success("Client enregistré.", window, cx)))
                .child(Button::new("o6").label("Toast erreur").on_click(|_, window, cx| {
                    toast::error("Enregistrement impossible : le service de données ne répond pas.", window, cx)
                }))
                .child(Button::new("o7").ghost().label("Marquer le panneau modifié").on_click(move |_, _, cx| {
                    panels.update(cx, |stack, cx| {
                        if let Some(id) = stack.top().map(|l| l.id.clone()) {
                            stack.set_dirty(&id, true);
                            cx.notify();
                        }
                    })
                })),
        )
    }

    fn panel_layer(&self, layer: &PanelLayer, cx: &App) -> Panel {
        let m = cx.ui().m;
        let panel = Panel::new(&self.panels, layer.id.clone());
        if layer.kind.as_ref() == "client" {
            panel
                .badge("Actif", Status::Success)
                .action(IconButton::new("p-edit", IconName::Pencil).title("Modifier").size(IconButtonSize::Md))
                .child(
                    PanelSection::new("Coordonnées").icon(IconName::IdCard).child(InfoList::new(vec![
                        InfoItem::new("Téléphone", Some("06 12 34 56 78")),
                        InfoItem::new("Email", Some("lea.martin@example.fr")),
                        InfoItem::new("Adresse", Some("12 rue des Motos, 35000 Rennes")),
                    ])),
                )
                .child(
                    PanelSection::new("Stock").icon(IconName::Package).child(InfoList::new(vec![
                        InfoItem::new("Quantité", Some("12 pièce")),
                        InfoItem::new("Seuil d'alerte", None::<&str>).placeholder("Aucun seuil"),
                    ])),
                )
                .child(
                    PanelSection::new("Attributs véhicule")
                        .subtitle("Une section qui se replie : ici ouverte")
                        .collapsible(false, |_, _, _| {})
                        .child(InfoList::new(vec![InfoItem::new("Année", Some("2021"))])),
                )
                .child(
                    PanelSection::new("Divers")
                        .subtitle("ici repliée")
                        .collapsible(true, |_, _, _| {})
                        .child("Ce texte ne se voit pas."),
                )
                .child(PanelSection::new("Véhicules").subtitle("2").flush().child(
                    col().child(ListRow::new("pv1", "Honda CB 500 F (2019)").meta("AB-123-CD").icon(IconName::Bike)).child(
                        ListRow::new("pv2", "Peugeot Kisbee 50").meta("Série VGA…0042").icon(IconName::Bike),
                    ),
                ))
        } else {
            panel
                .footer(Button::new("p-cancel").label("Annuler"))
                .footer(Button::new("p-save").primary().label("Enregistrer"))
                .child(col().gap(m.space_4).child(FormField::new("Téléphone").child(Input::new(&self.panel_note))))
        }
    }
}

impl Render for Gallery {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.ui().clone();
        let (c, m) = (theme.colors, theme.m);
        let nav = cx.entity();

        let body = match self.section.as_ref() {
            "content" => self.content(cx),
            "table" => col().gap(m.space_7).child(self.table(cx)).child(self.overlays(cx)),
            _ => self.controls(cx),
        };
        let layers: Vec<PanelLayer> = self.panels.read(cx).layers().to_vec();
        let title = match self.section.as_ref() {
            "content" => "Contenu",
            "table" => "Table et panneaux",
            _ => "Contrôles",
        };

        col()
            .size_full()
            .bg(c.bg_app)
            .text_color(c.text)
            .font_family(FONT_UI)
            .fs(m.fs_md)
            .child(
                TitleBar::new()
                    .child(Icon::new(IconName::Bike).color(c.accent))
                    .child(div().font_weight(FontWeight::SEMIBOLD).child("Motodesk"))
                    .child(div().text_color(c.text_muted).child("Galerie du kit"))
                    .child(div().ml(m.space_2).child(MenuStrip::new(Self::menus(&nav, &self.panels))))
                    .trailing(NotificationBell::new(Self::notices()).open(self.notices_open).on_open_change({
                        let gallery = nav.clone();
                        move |open, _, cx| {
                            gallery.update(cx, |this, cx| {
                                this.notices_open = open;
                                cx.notify();
                            })
                        }
                    }))
                    .trailing(Button::new("mode").small().ghost().icon(if theme.is_dark() { IconName::Sun } else { IconName::Moon }).label(if theme.is_dark() { "Clair" } else { "Sombre" }).on_click(|_, _, cx| {
                        UiTheme::update(cx, |t| t.mode = if t.mode == Mode::Dark { Mode::Light } else { Mode::Dark })
                    }))
                    .trailing(Button::new("density").small().ghost().label(format!("{:?}", theme.density)).on_click(|_, _, cx| {
                        UiTheme::update(cx, |t| {
                            t.density = match t.density {
                                Density::Regular => Density::Compact,
                                Density::Compact => Density::Pos,
                                Density::Pos => Density::Regular,
                            }
                        })
                    }))
                    .trailing(Button::new("accent").small().ghost().label("Accent").on_click(|_, _, cx| {
                        UiTheme::update(cx, |t| t.accent = if t.accent == DEFAULT_ACCENT { 0xc2410c } else { DEFAULT_ACCENT })
                    })),
            )
            .child(
                row()
                    .flex_1()
                    .min_h_0()
                    .items_stretch()
                    .child(
                        Sidebar::new(
                            vec![
                                SidebarItem::new("controls", "Contrôles", IconName::Square),
                                SidebarItem::new("content", "Contenu", IconName::LayoutDashboard),
                                SidebarItem::new("table", "Table et panneaux", IconName::Table).badge("3"),
                                SidebarItem::new("off", "Indisponible", IconName::Lock).disabled(true),
                            ],
                            self.section.clone(),
                        )
                        .footer(vec![SidebarItem::new("settings", "Réglages", IconName::Settings)])
                        .on_select(move |id, _, cx| {
                            let id = id.clone();
                            nav.update(cx, |this, cx| {
                                this.section = id;
                                cx.notify();
                            })
                        }),
                    )
                    .child(
                        col()
                            .relative()
                            .flex_1()
                            .min_w_0()
                            .child(
                                PageToolbar::new(title)
                                    .icon(IconName::Layers)
                                    .meta("kit")
                                    .subtitle("Tous les composants, tels qu'un écran les utilise")
                                    .trailing(SearchField::new(&self.search).width(px(240.)))
                                    .trailing(Button::new("tb-new").primary().icon(IconName::Plus).label("Nouveau")),
                            )
                            .child(div().id("gallery-scroll").flex_1().min_h_0().overflow_y_scroll().child(div().p(m.space_6).child(body)))
                            .child(PanelHost::new(&self.panels).children(layers.iter().map(|layer| self.panel_layer(layer, cx)))),
                    ),
            )
            .child(
                StatusBar::new()
                    .left(StatusBarButton::new("st-sync", "Synchronisé").dot(c.state_ok).title("Dernière synchronisation à l'instant"))
                    .left(StatusBarButton::new("st-lan", "2 postes").icon(IconName::Users))
                    .right(StatusBarButton::new("st-user", "Léa Martin").icon(IconName::CircleUser).on_click(|_, _, _| {}))
                    .right(StatusBarButton::new("st-version", "0.1.0")),
            )
    }
}

fn main() {
    let args: Rc<Vec<String>> = Rc::new(std::env::args().collect());
    motodesk_ui::application().run(move |cx| {
        motodesk_ui::init(cx);
        if args.iter().any(|a| a == "--dark") {
            UiTheme::update(cx, |t| t.mode = Mode::Dark);
        }
        if args.iter().any(|a| a == "--compact") {
            UiTheme::update(cx, |t| t.density = Density::Compact);
        }
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size(px(1280.), px(820.)), cx))),
            window_min_size: Some(size(px(960.), px(600.))),
            ..TitleBar::window_options()
        };
        motodesk_ui::gpui::open_window(options, cx, |window, cx| cx.new(|cx| Gallery::new(window, cx)))
            .expect("the gallery window opens");
        cx.activate(true);
    });
}
