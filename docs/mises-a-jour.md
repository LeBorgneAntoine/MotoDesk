# Mises à jour

Motodesk prévient quand une nouvelle version existe, montre ce qu'elle apporte, et l'installe quand un administrateur le décide. **Rien ne s'installe tout seul.**

## Ce que voit l'utilisateur

- Un bandeau « Nouvelle version disponible » apparaît pour les administrateurs, avec « Voir les nouveautés » (les notes de la version, c'est-à-dire sa section du `CHANGELOG.md`) et « Plus tard » (le bandeau revient le lendemain, ou tout de suite pour une version plus récente).
- **Réglages › Sauvegardes › Mises à jour** : la version installée, « Rechercher une mise à jour », et un interrupteur pour la recherche automatique (au démarrage, puis toutes les 6 heures).
- Installer : l'application télécharge l'installateur, vérifie sa signature, se ferme, s'installe et se rouvre. Les données restent en place ; une copie de la base est faite avant sa mise à jour (dossier `pre-migration/`, les trois dernières sont gardées).
- Une recherche qui échoue (pas d'Internet) est silencieuse en automatique, et expliquée en manuel.

## Pourquoi on peut s'y fier

1. **Origine** : l'adresse de recherche est fixée dans le programme (`motodesk_client::updates::ENDPOINT`) : les versions publiées de ce dépôt GitHub, en HTTPS. Un test refuse toute autre adresse et tout transport non sécurisé.
2. **Signature** : chaque installateur est signé avec une clé privée que seul le mainteneur détient. La clé publique correspondante est **compilée dans l'application**. Un installateur dont la signature ne se vérifie pas n'est jamais lancé, d'où qu'il vienne : même un serveur compromis, ou un `latest.json` modifié, ne peut pas faire installer autre chose que ce que le mainteneur a signé. (Au pire, un `latest.json` modifié fait échouer la mise à jour ou en affiche de fausses notes, qui sont rendues sans HTML ni script.)
3. **Pas de retour en arrière** : seule une version strictement plus récente que celle qui tourne est proposée.
4. **Brouillon d'abord** : le workflow de publication crée une version *brouillon*, invisible des applications installées. Elle n'est proposée à personne tant que quelqu'un ne l'a pas relue et publiée.
5. **Vérifiable** : le code est public ; chaque version publiée porte aussi `SHA256SUMS.txt`.

Ce que la signature **ne** remplace **pas** : la signature de code Windows (Authenticode). Sans elle, Windows SmartScreen avertit à la **première installation manuelle** (« éditeur inconnu »). Les mises à jour suivantes, lancées par l'application, passent par la signature décrite ici.

## Publier une version

1. Mettre à jour la version dans `desktop-gpui/Cargo.toml` (le workflow vérifie qu'elle concorde avec le tag).
2. Dans `CHANGELOG.md`, renommer `## [Non publié]` en `## [X.Y.Z] - AAAA-MM-JJ` (le workflow refuse une version sans section : c'est ce texte que les utilisateurs liront), puis rouvrir une section `[Non publié]` vide au-dessus.
3. Fusionner, puis pousser le tag : `git tag vX.Y.Z && git push origin vX.Y.Z`.
4. Le workflow *Release* construit l'installateur, le signe, écrit `latest.json` et crée la version **brouillon**.
5. Relire le brouillon sur GitHub, télécharger l'installateur et l'essayer, puis **publier**. C'est cette publication qui rend la mise à jour visible aux applications installées. Un tag avec suffixe (`v1.2.0-rc.1`) est publié comme pré-version et jamais proposé.

## La clé de signature

- Elle se génère une seule fois, sur le poste du mainteneur : `cargo packager signer generate --path "$HOME\.motodesk\motodesk-updater.key"` (un mot de passe est demandé). C'est une clé minisign, au format de celle que produisait l'outillage Tauri : une clé déjà créée ainsi reste valable.
- La **clé publique** (`.key.pub`) va dans `PUBLIC_KEY` de `crates/motodesk-client/src/updates.rs`. Elle n'est pas secrète.
- La **clé privée** et son mot de passe vont dans les secrets du dépôt (`TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` : ils gardent le nom sous lequel ils ont été créés) **et** dans une sauvegarde hors ligne (gestionnaire de mots de passe). Ne jamais la committer.
- **Perdue**, aucune installation existante ne peut plus être mise à jour automatiquement : il faudrait réinstaller à la main une version portant une nouvelle clé publique. **Divulguée**, quelqu'un pourrait signer de fausses mises à jour : même remède, plus vite.

## Limites connues

- Windows uniquement.
- Les postes d'un même réseau local se mettent à jour un par un ; pendant ce temps ils peuvent tourner sur des versions différentes. Mettre à jour tous les postes dans la même journée.
- Une version installée ne se « désinstalle » pas par l'application : pour revenir en arrière, réinstaller l'installateur de la version précédente (les données migrées par la version plus récente peuvent ne plus s'ouvrir : voir la copie `pre-migration/`).
- Les notes affichées sont celles de la dernière version seulement, pas de toutes celles sautées.
