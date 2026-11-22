# Journal des modifications

Le format suit [Keep a Changelog](https://keepachangelog.com/fr/1.1.0/) et le
projet suit le [versionnage sémantique](https://semver.org/lang/fr/).

## [Non publié]

### Ajouté
- Rappel de sauvegarde pour les administrateurs : bandeau « Données non sauvegardées » tant qu'aucune sauvegarde automatique n'est active (elles n'écrivent rien sans mot de passe) ou qu'elles se sont arrêtées.
- Installateur Windows (NSIS, sans droits administrateur) produit par l'intégration continue.
- Journaux : un fichier par jour (14 conservés) dans `%LOCALAPPDATA%\Motodesk\logs`, avec les plantages (panic) et leur origine ; carte « Diagnostic » dans Paramètres › Sauvegardes pour ouvrir le dossier.
- Workflow de publication : un tag `vX.Y.Z` construit l'installateur et prépare une version brouillon.
