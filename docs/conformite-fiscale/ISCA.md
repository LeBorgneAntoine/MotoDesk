# Conformité ISCA — documentation technique

Ce document décrit comment Motodesk satisfait aux quatre conditions posées par
l'article 286 I-3° bis du CGI — **i**naltérabilité, **s**écurisation,
**c**onservation, **a**rchivage — et où, dans le code, chacune est mise en
œuvre. Il est la pièce à produire en cas de contrôle, en accompagnement de
l'[attestation](ATTESTATION.md).

Les références « § n » renvoient aux paragraphes du BOFiP
BOI-TVA-DECLA-30-10-30.

## Périmètre

Le § 40 limite l'obligation, pour un logiciel multifonctions, à « la seule
fonctionnalité de caisse enregistreuse et encaissement ». Sont donc dans le
périmètre attesté :

- l'émission des factures, des tickets de caisse et des avoirs (`routes/sales.rs`, `routes/pos.rs`) ;
- l'enregistrement des règlements (mêmes modules) ;
- les sessions de caisse, le fond de tiroir et les ouvertures hors vente (`session_opened`, `session_closed`, `drawer_opened`) ;
- les clôtures et les totaux (`fiscal/closures.rs`) ;
- l'archivage (`fiscal/archive.rs`).

Sont hors périmètre : clients, véhicules, stock, atelier, modèles de documents,
synchronisation des données non fiscales.

## Architecture en deux étages

Les données de travail (`sales`, `sale_lines`) restent des entités synchronisées
et fusionnées champ à champ, ce qui est structurellement l'inverse de
l'inaltérabilité. Elles ne sont donc pas la source de vérité fiscale.

```
Étage de travail (modifiable)          Étage fiscal (append-only, chaîné)
  sales / sale_lines  ──validate──►  fiscal_journal  ──►  fiscal_closures
  brouillons, devis                    seq + prev_hash        J / M / A
                                       + signature Ed25519    + total perpétuel
                                            │
                                            └──►  fiscal_archives
```

La validation est l'unique passage d'un étage à l'autre, et elle est
irréversible. Après elle, la ligne de travail porte `fiscal_seq` et `locked_at`
et n'est plus jamais modifiée : toute correction ultérieure est un **avoir**,
c'est-à-dire une nouvelle entrée en moins, conformément au § 90 qui exige des
corrections « par des opérations de plus et de moins et non par modification
directe des données d'origine ».

## Inaltérabilité

| Mécanisme | Emplacement |
|---|---|
| Chaînage SHA-256 de chaque entrée sur la précédente | `crates/motodesk-fiscal/src/chain.rs` |
| Sérialisation JSON canonique avant hachage, rejetant les flottants | `crates/motodesk-fiscal/src/canonical.rs` |
| Déclencheurs SQL refusant `UPDATE` et `DELETE` | `migrations/010_fiscal.sql` : `fiscal_journal_no_update`, `fiscal_journal_no_delete`, `fiscal_closures_no_update`, `fiscal_closures_no_delete`, `fiscal_archives_no_delete` |
| Verrouillage de la ligne de travail à la validation | `routes/sales.rs`, fonction `validate` |
| Refus de fusionner une vente déjà scellée | `crates/motodesk-sync/src/merge/mod.rs` |
| Réplication du journal en insertion seule | `EntityType::FiscalEntry`, `fiscal::replicate` |

Le journal ne possède ni `deleted_at`, ni `row_version`, ni `field_meta` : les
colonnes qui rendent une entité fusionnable n'existent pas.

Deux propriétés méritent d'être signalées à un contrôleur :

- **L'horloge ne peut pas faire reculer la chaîne.** Si l'horloge système
  recule, l'entrée est horodatée à l'instant de l'entrée précédente et l'écart
  est consigné dans la charge utile sous `clock_skew_reported_at`. Le client
  n'est pas bloqué au comptoir et l'anomalie reste visible (`fiscal/mod.rs`,
  fonction `append`).
- **L'entrée et la ligne métier sont écrites dans la même transaction.** Une
  coupure ne peut pas laisser une facture sans trace fiscale, ni l'inverse.

## Sécurisation

Chaque entrée est signée en Ed25519 par la clé propre à la caisse
(`crates/motodesk-fiscal/src/signature.rs`). La clé est conservée dans le
trousseau du système d'exploitation ; à défaut de trousseau utilisable, elle
tombe dans la table `fiscal_key_fallback` et le mode de conservation retenu est
inscrit en clair dans l'entrée d'ouverture de caisse, puis affiché dans
l'écran *Comptabilité › Caisse & intégrité* (`fiscal/keys.rs`).

La vérification parcourt chaque chaîne, recalcule les empreintes et contrôle les
signatures : `fiscal::verify_all`, exposée par `GET /api/fiscal/verify` et par
le bouton « Vérifier l'intégrité » de l'écran *Caisse & intégrité*. Le rapport
nomme la première rupture et son numéro de séquence, en français.

**Numérotation multi-caisses.** Chaque appareil possède sa propre série et sa
propre chaîne, de la forme `FA-2026-C1-00042` (`crates/motodesk-fiscal/src/numbering.rs`).
Deux caisses hors ligne ne peuvent donc pas attribuer le même numéro, et chaque
série reste ininterrompue et chronologique, ce qu'exige l'article 242 nonies A
de l'annexe II au CGI. Le numéro est alloué à la validation et jamais avant :
un brouillon abandonné ne laisse pas de trou.

## Conservation

- **Clôtures journalière, mensuelle et annuelle**, « cumulatives et
  impératives » au sens du § 170, dans `fiscal/closures.rs`. Une clôture
  mensuelle exige que tous les jours du mois soient clôturés, et une clôture
  annuelle que tous les mois le soient.
- **Total du grand total de la période** et **total perpétuel**, ce dernier
  n'étant jamais remis à zéro. Les deux sont recalculés à partir de la somme des
  entrées du journal au moment de la clôture, et non lus dans un compteur : une
  clôture doit pouvoir être refaite à l'identique.
- **Compteurs à zéro à la mise en service** (§ 273). Le chiffre d'affaires
  repris de l'ancien logiciel Motogest est archivé à part et n'est pas versé au
  total perpétuel.
- **Aucune purge.** Le § 220 interdit d'effacer les clôtures ; le déclencheur
  `fiscal_closures_no_delete` rend l'opération impossible en base.
- Les clôtures sont elles-mêmes scellées dans le journal, donc couvertes par le
  chaînage.

## Archivage

`fiscal/archive.rs` produit, au plus une fois par exercice (§ 220), une archive
ZIP contenant :

| Fichier | Contenu |
|---|---|
| `journal.csv` | Le journal fiscal permanent, une ligne par événement, dans l'ordre de la séquence |
| `clotures.csv` | Clôtures J / M / A avec les totaux |
| `documents.csv` | En-têtes des factures et avoirs validés |
| `lignes.csv` | Détail ligne à ligne |
| `reglements.csv` | Règlements encaissés |
| `manifeste.json` | Empreinte SHA-256 de chaque fichier et clé publique de la caisse |
| `NOTICE.txt` | Notice en français exigée par le § 230, expliquant comment recontrôler l'archive |

Le CSV est un format ouvert lisible dans n'importe quel tableur dans dix ans, ce
que le § 230 attend. La production de l'archive est elle-même tracée dans le
journal (§ 240) et enregistrée dans `fiscal_archives`. Une archive ne peut être
créée que si l'exercice a été clôturé annuellement, et une seule archive existe
par exercice.

`archive::verify` recalcule l'empreinte du fichier et la compare à celle
enregistrée ; l'écran *Comptabilité › Archives* expose le contrôle.

## Restitution à l'administration

Au-delà des quatre conditions, le module *Comptabilité* fournit ce qu'un
contrôle ou l'expert-comptable réclame, en lecture seule et calculé depuis le
journal :

- journal des ventes et journal de caisse ;
- récapitulatif de TVA par taux, alimentant les cases de la CA3 ;
- balance âgée des règlements attendus ;
- **export FEC** (`fiscal/fec.rs`), dix-huit colonnes séparées par des barres
  verticales, nommé `{SIREN}FEC{AAAAMMJJ}.txt`, conforme à l'arrêté du
  29 juillet 2013 pris pour l'article L47 A I du LPF. Chaque pièce y est
  équilibrée au centime, ce que vérifie le test
  `the_fec_balances_voucher_by_voucher`.

## Mentions légales des factures

La validation d'une facture est refusée tant que le profil de l'entreprise est
incomplet (`fiscal/mentions.rs`). Sont exigés la dénomination sociale,
l'adresse, le SIRET à quatorze chiffres, la forme juridique, le code APE, puis
selon le cas la ville du RCS et le capital social pour une société
(C. com. art. R123-237 et R123-238), le numéro de TVA intracommunautaire pour un
assujetti ou la mention d'exonération de l'article 293 B du CGI, et enfin les
conditions de pénalités de retard assorties de l'indemnité forfaitaire de
recouvrement de 40 € (C. com. art. L441-9, CGI ann. II art. 242 nonies A).
Chaque manque est restitué avec le motif qui le rend obligatoire, et l'écran
*Réglages* affiche la liste.

Toute réimpression d'un document déjà émis porte la mention **DUPLICATA** en
filigrane et fait l'objet d'une entrée `document_reprinted` au journal ; la
première impression est distinguée sous `document_printed`
(`lib/templates/print.ts`, `routes/sales.rs`).

## Ce que ce document ne couvre pas

- **Facturation électronique.** L'obligation de recevoir des factures par une
  plateforme agréée s'applique au 1er septembre 2026, l'émission et
  l'e-reporting au 1er septembre 2027. Le format Factur-X n'est pas encore
  produit ; l'instantané figé à la validation en contient déjà tous les champs.
- **La comptabilité générale.** Motodesk n'est pas le livre de référence ; le
  logiciel de l'expert-comptable l'est. L'export FEC est le point de passage.

## Tenue à jour

Toute modification de l'un des éléments listés ci-dessous impose de réémettre
l'attestation et de mettre ce document à jour :

- `crates/motodesk-fiscal/`
- `crates/motodesk-server/src/local_server/fiscal/`
- `crates/motodesk-server/src/local_server/routes/sales.rs`
- `crates/motodesk-server/src/local_server/routes/pos.rs`
- `crates/motodesk-server/src/local_server/pos/`
- `crates/motodesk-server/migrations/010_fiscal.sql` et migrations fiscales
  ultérieures (`011_pos.sql` pour les sessions)
- les garde-fous d'immutabilité de `crates/motodesk-sync/src/merge/mod.rs`
