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
| Accès typé à l'API locale, sans passer par le réseau | `crates/motodesk-client` | clients, véhicules en stock, catalogue, fournisseurs et mouvements de stock, ventes et règlements, ordres de réparation, comptabilité et contrôles fiscaux, identité de l'entreprise, sauvegardes, comptes, équipe, postes du réseau, tableau de bord, réglages, synchro |
| Kit d'interface (jetons, thèmes, composants) | `crates/motodesk-ui` | fait pour les écrans migrés |
| Application native | `desktop-gpui` | fenêtre, connexion, coquille |

| Écran | État |
|---|---|
| Connexion et inscription (en ligne et réseau local), clé de récupération | migré |
| Tableau de bord (indicateurs, modules, activité récente) : l'écran d'accueil | migré |
| Clients & Véhicules | migré |
| Parc VN/VO (liste, fiche, vente avec reprise) | migré |
| Inventaire (catalogue, catégories, fiche produit, variantes, réception fournisseur) | migré |
| Ventes (devis, factures, validation, avoirs, règlements) | migré, sans l'impression |
| L'Atelier (tableau des ordres de réparation, fiche, facturation) | migré |
| Comptabilité (journaux des ventes et de caisse, TVA, balance âgée, FEC, caisse et intégrité, clôtures, archives, journal fiscal) | migré |
| Paramètres (réglages par catégorie, entreprise et logo, sauvegardes, synchronisation, passage en ligne) | migré, sans les réglages de la Caisse et de l'Assistant |
| Coquille : bandeaux du réseau local et « Reconnecter ce poste » | migré |
| Employés : comptes du réseau local, clé de récupération, postes du réseau | migré |
| Employés : équipe d'une entreprise en ligne (rôles, invitation, fiche) | migré, vu contre un service de démonstration seulement |
| Modèles, Dossier partagé, Données, Assistant, Caisse | à faire |

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
les menus de la barre de titre. Avec ces menus manquent ce qu'ils ouvraient depuis le tableau de
bord : les raccourcis (Ctrl+N, F5...) et les fenêtres d'aide (documentation, raccourcis, à
propos). Sur
l'Inventaire, les entrées de ces menus (nouveau produit, réception, nouvelle catégorie) ont
toutes leur bouton à l'écran ; F5 actualise.

**L'impression n'est pas portée** : elle arrive avec les modèles de documents. D'ici là,
l'écran Ventes n'a ni bouton d'impression ni duplicata, et un devis ne prend son numéro qu'en
étant transformé en facture ou en ordre de réparation (dans Svelte, il le prend aussi à
l'impression). Une facture validée ici s'imprime depuis l'interface Svelte.

Sur l'écran Ventes, la fiche d'un document montre aussi son client, que la fiche Svelte ne
montrait pas. Les totaux affichés pendant la saisie viennent de `motodesk_fiscal::vat`, le calcul
avec lequel le service scelle : ce qui est à l'écran est ce qui sera enregistré.

L'éditeur de lignes d'un document (article, désignation, quantité, prix, remise, TVA, totaux)
est le même pour une vente et pour un ordre de réparation : `desktop-gpui/src/pages/lines.rs`.
Svelte en portait une copie dans chaque formulaire.

Sur L'Atelier, le champ « Technicien » propose l'équipe d'une entreprise en ligne, comme dans
Svelte : sur un réseau local, où l'équipe n'existe pas, il reste vide. Une affectation déjà
enregistrée est conservée et affichée « Technicien assigné ».

Sur les Paramètres :

- Les réglages sont décrits dans `desktop-gpui/src/pages/settings/registry.rs` (un réglage, une
  entrée). Deux réglages de Svelte n'y sont pas : les coins (tout est carré) et le flou (cette
  interface n'en dessine pas).
- Un nombre hors de ses bornes ou une couleur mal écrite est signalé sous son champ et bloque
  « Enregistrer » ; Svelte l'enregistrait tel quel. L'apparence (thème, accent, mode compact)
  change à l'enregistrement, pas pendant la saisie.
- Les catégories Caisse et Assistant affichent qu'elles arrivent avec leur écran. La carte des
  mises à jour et celle de l'arrière-plan (rester actif fenêtre fermée, démarrer avec Windows)
  arrivent avec la zone de notification et les mises à jour.
- Sur un réseau local, vider la raison sociale la retire vraiment (Svelte envoyait `null`, que
  le service lit comme « ne rien changer »).
- **Défaut du service trouvé en chemin, présent aussi sous Svelte** : « Synchroniser », dans
  Paramètres › Synchronisation, renvoie la session au service ; pour une session de réseau
  local, le service remplace alors sa propre fiche de session et déconnecte l'utilisateur.
  Le client natif ne renvoie plus une session locale (`auth::sync_session_to_local`, avec son
  test) ; le service et l'écran Svelte restent à corriger.
- Non essayés dans l'application : le choix d'un fichier de logo et « Ouvrir le dossier des
  journaux » (fenêtres du système), le mot de passe des sauvegardes automatiques (trousseau du
  système), une restauration menée jusqu'au redémarrage, l'envoi de « Passer en ligne » (étape
  définitive, vers le vrai cloud), Maj + clic sur « Synchroniser », l'écriture de l'identité
  d'une entreprise en ligne, et les vues d'un compte non administrateur. L'envoi et la lecture
  du logo, l'écriture d'une sauvegarde et le refus d'un mauvais mot de passe sont couverts par
  les tests du client.

Sur la Comptabilité, quelques écarts avec l'écran Svelte. Une clôture montre sa période par son
nom (« octobre 2026 », « 24 novembre 2026 ») plutôt que par ses deux dates, et la confirmation
nomme la période avant de la sceller. Les quatre boutons de clôture sont dans le corps de la
carte, pas dans son en-tête. Le FEC s'enregistre par la fenêtre « Enregistrer sous » du système
(`SaveExt::save_file` dans `desktop-gpui/src/backend.rs`) : ce chemin, la copie du FEC et celle
du chemin d'une archive n'ont pas été essayés dans l'application, pour ne toucher ni au
presse-papiers ni aux fichiers de la machine de développement. Le contrôle d'une archive est
couvert par le test du client, pas vu à l'écran : le jeu de démonstration n'a pas d'année close.
Le rappel de la coquille qui ouvre les clôtures (`section=closures`) arrive avec les bandeaux
d'alerte ; l'écran lit déjà ce paramètre.

Deux écarts voulus avec l'écran Svelte de l'Inventaire : la réception fournisseur s'ouvre sans
produit choisi (Svelte présélectionnait le premier de la liste, au risque de recevoir sur le
mauvais article), et la référence proposée pour une variante (« BRMB-PLQ-001- ») n'est pas
enregistrée telle quelle si personne ne la complète : le service la déduit alors du modèle. 

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
cargo run -p motodesk-gpui --example seed -- C:\Temp\motodesk-demo    # 137 clients, 14 véhicules en stock, 6 produits et un profil d'entreprise, fictifs
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

Ordre prévu : Import, Caisse, Assistant, Modèles et impression. Viennent ensuite l'icône de la zone
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
