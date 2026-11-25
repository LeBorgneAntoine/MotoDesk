# Migration de l'interface vers GPUI

L'interface de Motodesk passe de Svelte (dans une fenêtre web Tauri) à une interface native
écrite en Rust avec [GPUI](https://gpui.rs). Le service de données ne change pas : API locale,
base SQLite, journal fiscal et synchronisation restent les mêmes. La migration se fait **écran par
écran**, sur une branche à part, et l'interface actuelle reste livrée tant que tous les écrans ne
sont pas passés.

## Ce qui est en place

| Élément | Où | État |
|---|---|---|
| Service de données indépendant de Tauri | `crates/motodesk-server` | fait |
| Accès typé à l'API locale, sans passer par le réseau | `crates/motodesk-client` | clients, véhicules en stock, comptes, équipe, postes du réseau, réglages, synchro |
| Kit d'interface (jetons, thèmes, composants) | `crates/motodesk-ui` | fait pour les écrans migrés |
| Application native | `desktop-gpui` | fenêtre, connexion, coquille |

| Écran | État |
|---|---|
| Connexion et inscription (en ligne et réseau local), clé de récupération | migré |
| Clients & Véhicules | migré |
| Parc VN/VO (liste, fiche, vente avec reprise) | migré |
| Coquille : bandeaux du réseau local et « Reconnecter ce poste » | migré |
| Employés : comptes du réseau local, clé de récupération, postes du réseau | migré |
| Employés : équipe d'une entreprise en ligne (rôles, invitation, fiche) | migré, vu contre un service de démonstration seulement |
| Tableau de bord, Atelier, Inventaire, Ventes, Comptabilité, Modèles, Dossier partagé, Données, Paramètres, Assistant, Caisse | à faire |

Des dernières évolutions de l'interface Svelte (master `f6e767d`), les postes du réseau et la
carte de la clé de récupération sont portés avec l'écran Employés. Restent la récupération
d'accès et la restauration au premier lancement, notées plus bas.

L'équipe en ligne s'adresse au cloud, qui n'est pas dans ce dépôt : ses appels sont fixés par
un test (`crates/motodesk-client/tests/cloud_employees.rs`) et l'écran a été regardé contre un
service de démonstration (voir « Lancer »), jamais contre le vrai cloud. « Inviter » copie
l'invitation dans le presse-papiers : le bouton n'a pas été essayé dans l'application.

Non repris pour l'instant sur les écrans migrés : « Mot de passe perdu ? » et « Restaurer une
sauvegarde » sur la page de connexion, l'impression de la clé de récupération, les ventes et
ordres d'atelier dans la fiche client (ils arrivent avec leurs écrans), les bandeaux d'alerte et
les menus de la barre de titre. Vendre un véhicule du parc crée bien la facture brouillon, mais
l'écran Ventes qui doit l'ouvrir n'est pas encore migré.

## Lancer

Les deux interfaces partagent le dossier de données et les ports 17300 et 17301 : **une seule
tourne à la fois**.

```powershell
cargo run -p motodesk-ui --example gallery          # tous les composants du kit
cargo run -p motodesk-gpui                          # l'application, sur les données du poste
```

Pour regarder les écrans sans toucher aux données du magasin, travaillez sur un dossier à part,
avec un profil isolé (ses secrets vont sous un autre nom dans le trousseau) :

```powershell
cargo run -p motodesk-gpui --example seed -- C:\Temp\motodesk-demo    # 137 clients et 14 véhicules en stock, fictifs
$env:MOTODESK_PROFILE = "demo"; $env:MOTODESK_DATA_DIR = "C:\Temp\motodesk-demo"
cargo run -p motodesk-gpui
```

L'écran Employés d'une entreprise en ligne se regarde sans cloud, en version de développement
seulement : un service de démonstration garde une petite équipe en mémoire, et la variable
`MOTODESK_DEV_TEAM_CLOUD` fait afficher l'équipe en ligne en s'adressant à lui.

```powershell
cargo run -p motodesk-gpui --example team_cloud                       # écoute sur 127.0.0.1:8791
$env:MOTODESK_DEV_TEAM_CLOUD = "http://127.0.0.1:8791"; cargo run -p motodesk-gpui
```

## Les règles du kit

Un écran importe ses contrôles de `motodesk_ui` et de nulle part ailleurs.

- **Pas de couleur écrite à la main** : `cx.ui().colors`.
- **Pas de taille écrite à la main** quand un jeton existe : `cx.ui().m`.
- **Aucun coin arrondi.** Il n'y a pas de réglage pour en remettre.
- **Aucun type de GPUI Kit** nommé dans un écran : les champs, menus et dialogues du kit en
  viennent, mais emballés.

`desktop-gpui/tests/ui_rules.rs` vérifie ces règles à chaque `cargo test`.

Un composant qui manque s'ajoute au kit, avec sa démonstration dans la galerie
(`crates/motodesk-ui/examples/gallery.rs`), pas dans un écran.

## Migrer un écran

1. **API** : porter le module de `desktop/src/lib/api/<écran>.ts` dans `crates/motodesk-client`
   (types et appels), avec un test contre les vrais gestionnaires
   (`crates/motodesk-client/tests/against_the_server.rs`).
2. **Kit** : ajouter les composants qui manquent, et leur démonstration dans la galerie.
3. **Écran** : l'écrire dans `desktop-gpui/src/pages/`, le brancher dans `shell.rs`.
4. **Tests** : porter en Rust les tests vitest de la logique de l'écran.
5. **Comparer** avec l'écran Svelte, côte à côte, sur les mêmes données.
6. Un commit par écran : `feat(gpui): <écran>`.

Ordre prévu : Tableau de bord, Inventaire, Ventes, Atelier, Comptabilité,
Paramètres, Import, Caisse, Assistant, Modèles et impression. Viennent ensuite l'icône de la zone
de notification, le démarrage avec Windows, les mises à jour et l'installateur, puis le retrait de
Svelte et de Tauri.

## Comment un écran parle au service

Le service tourne sur des fils Tokio dans le même processus ; l'interface, sur le fil principal de
GPUI. Un écran ne bloque jamais :

```rust
cx.api(
    move |client| async move { clients::list(&client, &query).await },   // sur les fils du service
    |this, result, cx| { /* de retour sur l'écran */ },
);
```

L'appel va droit au gestionnaire Axum, par les mêmes intergiciels que sur le port local, sans
socket. Le port 17300 reste ouvert pour l'interface Tauri.
