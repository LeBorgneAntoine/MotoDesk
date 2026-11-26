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
| Accès typé à l'API locale, sans passer par le réseau | `crates/motodesk-client` | clients, véhicules en stock, catalogue, fournisseurs et mouvements de stock, ventes et règlements, ordres de réparation, comptabilité et contrôles fiscaux, identité de l'entreprise, sauvegardes, dossier partagé (envoi et lecture par morceaux), import CSV (lecture du fichier, correspondance, envoi), tables et console SQL, caisse (session, encaissement, tickets en attente, tiroir, ticket, réglages du poste), assistant (clé du poste, conversations, réponse lue au fil de l'écriture), modèles de documents (bibliothèque, variables, mise en page, PDF), comptes, équipe, postes du réseau, tableau de bord, réglages, synchro |
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
| Paramètres (réglages par catégorie, entreprise et logo, sauvegardes, synchronisation, passage en ligne) | migré |
| Données : tables (aperçu, recherche, export d'une page, vidage), console SQL, import CSV | migré |
| Dossier partagé (dossiers, visibilité, envoi et téléchargement de fichiers, historique) | migré |
| Coquille : bandeaux du réseau local et « Reconnecter ce poste » | migré |
| Employés : comptes du réseau local, clé de récupération, postes du réseau | migré |
| Employés : équipe d'une entreprise en ligne (rôles, invitation, fiche) | migré, vu contre un service de démonstration seulement |
| Caisse (ouverture de session, recherche et lecture de codes, panier, encaissement en espèces, carte, chèque ou à plusieurs, tickets en attente, tiroir, clôture) | migré, sans imprimante, tiroir ni terminal réels |
| Assistant (conversations, échange, canevas, clé et modèle du poste) | migré, sans échange avec un vrai modèle |
| Modèles (bibliothèque, éditeur de mise en page, aperçu, export PDF) | migré |
| Impression des documents (facture ou devis depuis Ventes avec sa trace et son DUPLICATA, clé de récupération) | à faire : le moteur est là, pas encore branché |

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

**L'impression des documents n'est pas encore branchée.** Ce qui dessine une page et l'écrit en
PDF existe (voir « Sur les Modèles ») et sert déjà à l'aperçu d'un modèle. Il reste à l'appeler
depuis Ventes : d'ici là, cet écran n'a ni bouton d'impression ni duplicata, et un devis ne
prend son numéro qu'en étant transformé en facture ou en ordre de réparation (dans Svelte, il le
prend aussi à l'impression). Une facture validée ici s'imprime depuis l'interface Svelte.

Sur l'écran Ventes, la fiche d'un document montre aussi son client, que la fiche Svelte ne
montrait pas. Les totaux affichés pendant la saisie viennent de `motodesk_fiscal::vat`, le calcul
avec lequel le service scelle : ce qui est à l'écran est ce qui sera enregistré.

L'éditeur de lignes d'un document (article, désignation, quantité, prix, remise, TVA, totaux)
est le même pour une vente et pour un ordre de réparation : `desktop-gpui/src/pages/lines.rs`.
Svelte en portait une copie dans chaque formulaire.

Sur L'Atelier, le champ « Technicien » propose l'équipe d'une entreprise en ligne, comme dans
Svelte : sur un réseau local, où l'équipe n'existe pas, il reste vide. Une affectation déjà
enregistrée est conservée et affichée « Technicien assigné ».

Les fenêtres de fichiers du système (« Ouvrir », « Enregistrer sous ») ne se pilotent pas
depuis un essai automatique. En développement seulement (`desktop-gpui/src/backend.rs`),
`MOTODESK_DEV_PICK_FILES=<fichiers>` répond à la place d'une fenêtre « Ouvrir » et
`MOTODESK_DEV_SAVE_DIR=<dossier>` à la place d'une fenêtre « Enregistrer sous ». Ces deux
variables n'existent pas dans une version publiée.

Sur les Modèles :

- **Une page n'est plus une image.** Svelte dessinait la page sur un canevas, en tirait une
  image, et imprimait cette image depuis la fenêtre du navigateur intégré. Ici, un modèle et
  les données d'un document donnent une liste de formes en millimètres
  (`motodesk_client::templates::render`), que l'écran dessine (`PageView`, dans le kit) et
  que le PDF écrit (`templates::pdf`) : ce qui est montré est ce qui est imprimé. Dans le PDF
  le texte reste du texte (il se sélectionne, se cherche), les traits restent nets ; une
  facture d'une page pèse 3 Ko.
- **« Imprimer » passe par un PDF.** Il n'y a plus de navigateur pour ouvrir la fenêtre
  d'impression du système. L'aperçu propose « Enregistrer en PDF » et « Ouvrir pour
  imprimer », qui confie le fichier au lecteur de PDF du poste ; l'impression se lance de
  là. Envoyer la page directement à une imprimante reste possible plus tard, à partir des
  mêmes formes.
- Les documents sont composés en Helvetica, que tout lecteur de PDF possède : aucune police
  n'est embarquée, et les largeurs de ses lettres (`templates::metrics`) suffisent à couper
  et aligner les lignes. À l'écran c'est Arial, de mêmes largeurs. Svelte utilisait Segoe UI :
  les lignes ne tombent pas toujours au même endroit qu'avant.
- Le format d'un modèle est celui de Svelte (`templates::schema`) : un modèle fait là-bas
  s'ouvre ici, et inversement. L'image de la page que Svelte enregistrait avec chaque modèle
  n'est plus ni lue ni écrite : les cartes de la bibliothèque dessinent la page elle-même,
  avec l'identité de l'atelier. Un modèle enregistré ici garde donc, vu depuis Svelte, sa
  vignette d'avant.
- **Écarts voulus avec Svelte** : le cadre en pointillé de la marge ne s'imprime plus (il
  sortait sur chaque document) ; la mention DUPLICATA tient dans la page ; quitter l'éditeur
  avec des modifications non enregistrées le demande ; un élément choisi dans les calques se
  déplace à la souris même sous un élément plus grand ; les nombres se saisissent avec une
  virgule.
- **Simplifications par rapport à Svelte** : le texte d'un élément se saisit dans le panneau
  de droite (un double-clic sur la page y mène), pas sur la page ; les calques et les
  colonnes d'un tableau se réordonnent par boutons, pas en les glissant ; un outil s'ajoute
  d'un clic, pas en le déposant sur la page ; une image à une adresse web n'est pas chargée
  (le logo de l'atelier et les images écrites dans leur adresse le sont).
- L'éditeur : glisser pour déplacer (avec alignement sur les marges, le milieu de la page et
  les autres éléments), huit poignées pour redimensionner, flèches (Maj : 10 mm), Suppr,
  Ctrl+Z / Ctrl+Maj+Z, Ctrl+C / V / D, Ctrl+[ et ], Ctrl+S, Ctrl+P, Ctrl+molette pour le
  zoom. Ces raccourcis valent quand la page a le clavier ; dans un champ, les touches sont au
  texte. La géométrie et l'historique sont à part, avec leurs tests (`editor/geometry.rs`).
- `cargo run -p motodesk-client --example sample_pdf -- <dossier>` écrit les cinq modèles de
  base en PDF, plus une facture de trois pages marquée DUPLICATA, sans ouvrir l'application.
- Vérifié dans l'application sur un dossier de démonstration : installation des modèles de
  base, cartes et panneau, duplication, archivage et filtre des archivés, suppression, filtre
  par type, nouveau modèle, ouverture dans l'éditeur, sélection, déplacement et
  redimensionnement à la souris, flèches, saisie du texte, insertion d'une variable au
  curseur, ajout d'un texte et d'une image, logo de l'atelier, propriétés d'un tableau,
  premier plan, suppression au clavier, annuler, paysage, zoom, enregistrement, aperçu, export
  PDF (relu : logo compris), demande avant de quitter. Les PDF des modèles de base ont été
  relus un à un. Non essayés : « Ouvrir pour imprimer » (ouvre un autre programme) et une
  impression sur papier, les raccourcis avec Ctrl, Ctrl+molette, la copie d'une variable
  dans le presse-papiers, l'édition des colonnes d'un tableau champ par champ, un modèle fait
  sous Svelte ouvert ici (le format est fixé par un test), et la vue d'un employé.

Sur l'Assistant :

- La clé OpenRouter et le modèle sont ceux du poste : la clé est gardée par le service à côté
  de la base, le modèle dans `assistant.store.json`, le fichier de l'interface Svelte
  (`motodesk_client::assistant::PrefsStore`). Sans clé, l'écran est une démonstration : les
  quatre suggestions montrent ce que le canevas sait afficher (tableau, indicateurs, demande
  d'autorisation, document), avec des données inventées, et rien n'est demandé à personne.
- La réponse du modèle arrive par morceaux qui peuvent finir n'importe où, au milieu d'une
  ligne ou d'une lettre accentuée : `assistant::EventLines` les recolle, `ApiExt::api_stream`
  (`desktop-gpui/src/backend.rs`) les amène à l'écran un par un. Ce que chaque événement fait
  à la conversation est dans `pages/assistant/thread.rs`, sans dessin ni appel, avec ses tests.
- Le texte de l'assistant est du markdown dessiné par `Prose` (titres, listes, tableaux,
  citations, code). **Un lien écrit par un modèle n'ouvre que le web ou un écran de
  l'application** (`#/sales`) : un lien vers un fichier ou un autre protocole ne fait rien.
- Le champ de message est `Composer` : il grandit avec le texte, Entrée envoie, Maj+Entrée va
  à la ligne.
- Changer de conversation pendant qu'une réponse s'écrit cesse de l'écouter ; le service la
  termine et la garde. Sous Svelte, la suite de la réponse s'écrivait dans la conversation
  qu'on venait d'ouvrir.
- **Correction par rapport à Svelte** : choisir un modèle de la liste après avoir saisi un
  identifiant à la main prend effet. Svelte gardait l'identifiant saisi, qui l'emportait
  toujours sur la liste, champ masqué compris. Un identifiant saisi est enregistré dès qu'il
  est valide, sans attendre que le champ soit quitté.
- Avec une clé, les suggestions affichent la question qu'elles posent au modèle, et non
  « exemple de tableau ».
- Les conversations de démonstration durent le temps de l'application (Svelte les gardait
  dans le navigateur intégré). Le raccourci Ctrl+N arrive avec les menus de la barre de titre.
- **Aucun échange avec OpenRouter n'a été fait**, ni par les tests ni dans l'application :
  cela enverrait une requête à un service extérieur. L'adresse d'OpenRouter est fixée dans le
  service. En développement seulement, `MOTODESK_DEV_ASSISTANT_REPLY=<fichier markdown>` fait
  écrire ce fichier à l'écran comme le ferait un modèle, sans rien enregistrer.
- Vérifié dans l'application sur un dossier de démonstration : les quatre suggestions et
  leurs canevas, l'autorisation accordée, un message libre, la liste des documents, un lien
  vers l'écran Ventes, le retour sur l'écran (conversations retrouvées), la recherche, la
  suppression (démonstration et conversation enregistrée), l'historique replié et déplié,
  l'enregistrement et l'effacement d'une clé d'essai (dans le dossier de démonstration, hors
  trousseau), le choix du modèle, l'ouverture de conversations enregistrées, et une réponse
  écrite au fil de l'eau par le fichier de développement. Non essayés : « Tester la
  connexion » et un vrai message (OpenRouter), le refus d'une clé par OpenRouter, Maj+Entrée,
  le trousseau du système, et la vue d'un employé non administrateur.

Sur la Caisse :

- Un poste devient une caisse dans Paramètres › Caisse (administrateurs) : le réglage reste
  sur l'ordinateur, dans le même fichier `pos.store.json` que l'interface Svelte
  (`motodesk_client::pos::PosConfigStore`), et la caisse s'ouvre aussitôt, puis à chaque
  démarrage. « Bureau » ramène un administrateur à la coquille ; « Ouvrir la caisse
  maintenant » y retourne.
- L'encaissement tient en une seule fenêtre à étapes (mode de paiement, espèces, carte,
  chèque, vente enregistrée) au lieu d'une fenêtre par étape. Pendant un paiement en plusieurs
  fois, elle montre le reste à payer et ce qui a déjà été reçu.
- La monnaie à rendre s'affiche à la dernière étape, avec « Imprimer » et « Sans ticket ».
  **Sans imprimante réglée, « Imprimer » montre le texte du ticket à l'écran** ; Svelte
  annonçait l'impression sans rien montrer.
- **Si le service refuse la vente une fois le paiement saisi**, les règlements sont conservés
  et la fenêtre propose « Enregistrer la vente » à nouveau ou « Annuler les règlements » ;
  Svelte les gardait sans le dire.
- Un code tapé ou lu en entier (référence exacte) ajoute son article après une courte pause,
  sans Entrée ; Entrée prend le premier article de la liste. Une réponse arrivée après
  qu'Entrée a déjà servi est ignorée, pour ne pas compter l'article deux fois. Les boutons du
  panier rendent le clavier au champ de recherche, pour que la douchette continue d'y écrire.
- En mode facture, l'en-tête du panier dit « Facture » et « Encaisser » attend un client.
- La clôture refuse un montant compté vide (ce serait enregistrer tout le tiroir manquant) et
  n'annonce en vert qu'un tiroir conforme ; un écart s'affiche en avertissement.
- Le ticket est obligatoire à partir de 25 € de prestations (`must_print_ticket`). Le service
  n'a pas d'indicateur de garantie : ce cas de Svelte (`mentions.ts`) n'est pas repris.
- La caisse occupe tout l'écran. En développement seulement, `MOTODESK_DEV_WINDOWED=1` la
  garde dans sa fenêtre.
- Vérifié dans l'application sur un dossier de démonstration à part, en fenêtre : activation
  du poste, ouverture de session, recherche, code lu, article en rupture refusé, quantités,
  remise, retrait d'une ligne, encaissement en espèces (monnaie rendue), par carte
  (simulateur), par chèque, en deux fois, ticket affiché, mise en attente et reprise,
  ouverture du tiroir (journalisée), facture avec client, aller-retour au bureau, clôture de
  session avec et sans écart, clôture de journée, puis vente refusée le même jour (règlements
  conservés). Non essayés : une vraie imprimante, un vrai tiroir, un vrai terminal, le plein
  écran lui-même, les articles favoris, et la vue d'un employé non administrateur.
- **Constat sur le service, identique sous Svelte** : une fois la journée clôturée, une
  session de caisse s'ouvre encore et la carte est encaissée sur le terminal avant que le
  service ne refuse la vente. Avec un vrai terminal, le client serait débité d'une vente qui
  ne peut pas être enregistrée ce jour-là. Le refus devrait venir à l'ouverture de la session.

Sur le Dossier partagé :

- Un fichier fait jusqu'à 100 Mo : il est lu sur le disque morceau par morceau à l'envoi, et
  écrit sur le disque à mesure qu'il arrive au téléchargement (`motodesk_client::shared`,
  `Client::download`). Il n'est jamais tenu entier en mémoire, ce que faisait Svelte au
  téléchargement.
- Télécharger demande où enregistrer le fichier (fenêtre du système) ; Svelte l'envoyait dans
  les téléchargements du navigateur intégré.
- Les fichiers choisis ensemble partent l'un après l'autre et continuent de partir si l'écran
  est quitté ; ceux ajoutés pendant cet envoi attendent, et sont perdus si l'écran est quitté
  avant leur tour.
- Les champs d'un nom déjà rempli (renommer, modifier un dossier) s'ouvrent avec le nom
  sélectionné : `TextModel::focus_selected`.
- Vérifié dans l'application sur un dossier de démonstration à part : création d'un dossier,
  envoi de deux fichiers, téléchargement (fichier identique à l'octet près), renommage,
  suppression, changement de visibilité, historique. Non essayés : le glisser-déposer, les
  vraies fenêtres du système, le menu du clic droit, la vue d'un employé, et la copie entre
  deux postes.

Sur les onglets « Simple » et « Avancé » de l'écran Données :

- Une table de la base a bien plus de colonnes que la fenêtre n'est large : `DataTable::scroll_x`
  donne à chaque colonne une largeur lisible et fait défiler le tableau de côté, avec sa barre.
- Le vidage d'une table tient en une fenêtre à deux temps (ce que cela retire, puis le nom de
  la table à retaper) au lieu de deux fenêtres.
- La sortie de la console SQL montre l'entrée la plus récente en premier ; Ctrl+Entrée lance
  l'aperçu.
- **Le service compte toute la table même quand une recherche est active** : sous Svelte, le
  pied de page affiche alors « 1–12 sur 137 » et propose une page suivante qui n'existe pas.
  Ici il affiche « Lignes trouvées 1–12 · 137 dans la table » et ne propose la suite que si la
  page est pleine.
- **Défaut du service, visible aussi sous Svelte** : une cellule vide (NULL) revient du service
  sous la forme `0`. Le gestionnaire affiche donc « 0 » pour un téléphone, une note ou une date
  de suppression absents, à l'écran comme dans l'export. Cela ne se corrige que dans le service
  (`cell_to_value` dans `local_server/routes/data`).
- Vérifié dans l'application sur un dossier de démonstration à part : lecture et recherche,
  défilement de côté, vidage d'une table, aperçu et exécution d'une écriture SQL, refus d'un
  `DROP`. Non essayé : l'export d'une page (fenêtre « Enregistrer sous » du système).

Sur l'import CSV (écran Données, onglet « Import CSV ») :

- Les règles sont dans `motodesk_client::import` : `schema` (ce qui s'importe, champ par
  champ), `csv` (lecture du fichier : séparateur trouvé seul, UTF-8 ou Windows-1252, guillemets)
  et `transforms` (colonne → champ, lecture des montants, taux, quantités, oui/non). Le fichier
  est lu et ses lignes préparées hors du fil de l'interface.
- **Correction par rapport à Svelte** : les en-têtes qui portent exactement le nom d'un champ
  sont attribués d'abord. Avec un fichier « Nom;Email », Svelte donnait la colonne « Nom » au
  prénom (« prenom » contient « nom ») et laissait le nom, obligatoire, sans colonne.
- Une ligne refusée par le service pour une valeur déjà prise s'affiche « Référence (SKU) :
  cette valeur est déjà utilisée par une autre fiche » et non la phrase de la base de données.
- Le rapport d'erreurs est proposé dès qu'une ligne n'est pas entrée, y compris celles écartées
  avant l'envoi. Il est écrit avec le séparateur du fichier d'origine et marqué UTF-8, pour
  s'ouvrir en colonnes dans Excel ; Svelte l'écrivait toujours avec des virgules.
- « Ignorer les doublons », imposé pour les lignes de vente, n'est plus conservé en revenant à
  un autre type de données.
- L'aperçu répartit ses colonnes sur la largeur, sans défilement horizontal.
- Non essayés dans l'application : le choix d'un fichier par la fenêtre du système, le
  glisser-déposer, l'enregistrement du rapport d'erreurs, les modes « Mettre à jour » et
  « Ajouter ou mettre à jour » (ce dernier est couvert par le test du client), et l'import de
  véhicules, de ventes et de lignes de vente. Clients et produits ont été importés pour de bon
  dans un dossier de démonstration.
- Constat sur le service, identique sous Svelte : la recherche d'un client existant par e-mail
  tient compte des majuscules, alors que l'assistant signale les doublons sans en tenir compte.
  Deux lignes à la même adresse écrite différemment créent deux fiches.

Sur les Paramètres :

- Les réglages sont décrits dans `desktop-gpui/src/pages/settings/registry.rs` (un réglage, une
  entrée). Deux réglages de Svelte n'y sont pas : les coins (tout est carré) et le flou (cette
  interface n'en dessine pas).
- Un nombre hors de ses bornes ou une couleur mal écrite est signalé sous son champ et bloque
  « Enregistrer » ; Svelte l'enregistrait tel quel. L'apparence (thème, accent, mode compact)
  change à l'enregistrement, pas pendant la saisie.
- La carte des mises à jour et celle de l'arrière-plan (rester actif fenêtre fermée, démarrer
  avec Windows) arrivent avec la zone de notification et les mises à jour.
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

Ordre prévu : impression des documents (Ventes, clé de récupération). Viennent ensuite l'icône de la zone
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
