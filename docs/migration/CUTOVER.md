# Cutover Motogest → Motodesk

Motogest (Harmony) reste **l'archive légale** des années passées. Motodesk est le
système opérationnel et fiscal **à partir du jour de bascule**. Les numéros de
pièces Motogest ne continuent pas dans Motodesk : la chaîne ISCA et les compteurs
repartent à zéro.

## Ce qui est importé vs ce qui est scellé

| Donnée | Assistant CSV in-app | Journal fiscal Motodesk |
|--------|------------------------|-------------------|
| Clients, véhicules | Oui, sans `legacy_ref` | — |
| Catalogue | Produits seulement | — |
| Ventes, règlements, ordres d'atelier | Non | **Non** — hors chaîne |
| Totaux perpétuels / Z / FEC | — | Uniquement les ventes **validées dans Motodesk** |

Un comptable qui cherche le CA 2019–2024 doit ouvrir les archives Motogest (ou
les exports Harmony), pas Comptabilité dans Motodesk.

## Import

*Données › Import* : un assistant par fichier CSV, validation ligne à ligne. Il convient à un
démarrage avec les clients, les véhicules et le catalogue ; il ne rejoue pas un historique de
documents. Après un import de produits : **inventaire physique** (les quantités sont à saisir
via Inventaire › Réception ou un ajustement).

## Fiscalité

- Premier document **validé** dans Motodesk = premier maillon ISCA (`fiscal_journal`).
- L'historique importé n'est **jamais** scellé a posteriori (ce n'est pas une
  continuité de caisse enregistreuse).
- Lettre type au cabinet : Motogest conserve les journaux jusqu'à J−1 ; Motodesk
  produit FEC / clôtures / archives ZIP à partir de J.

Attestation et règles : [docs/conformite-fiscale/ISCA.md](../conformite-fiscale/ISCA.md).

## Nouveau PC (réinstall)

1. Sur l'ancien poste, administrateur : Réglages › **Sauvegardes** › exporter
   avec un mot de passe (≥ 10 caractères, à conserver hors de l'atelier : sans
   lui le fichier est illisible). Le fichier `.mgbak` est entièrement chiffré ;
   il contient SQLite, `archives/` et les secrets fiscaux / mesh.
2. Copier le fichier dans le dossier backups du nouvel appareil (indiqué à
   l'écran).
3. Restaurer (confirmer), puis **Redémarrer maintenant**. Les données
   remplacées sont conservées à côté de la base (`local.db.before-restore-…`).
4. Se connecter. La caisse est locale à la machine : réactiver Réglages ›
   Caisse si ce PC encaisse.

Sans sauvegarde : une réinstall vide ne ramène **pas** les archives fiscales ni les
clés du trousseau.

La sauvegarde automatique (même écran) utilise un mot de passe que vous
choisissez une fois (conservé dans le trousseau du système) ; sans lui, elle
ne s'exécute pas. Les 8 dernières sont gardées. Les anciens `.zip` restent
restaurables avec leur mot de passe d'origine.

## Jour J (checklist magasin)

1. Arrêter les ventes Motogest ; exporter / archivage Motogest selon le cabinet.
2. Import par l'assistant **ou** boutique neuve sans import.
3. Inventaire, profil légal (SIRET, TVA), activer la caisse sur le(s) till(s).
4. Premier devis / ticket Motodesk ; vérifier Comptabilité › intégrité.
5. Conserver Motogest en lecture seule (ou sauvegarde hors ligne) le temps
   d'une saison comptable.
