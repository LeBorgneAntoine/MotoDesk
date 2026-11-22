# Journal des modifications

Le format suit [Keep a Changelog](https://keepachangelog.com/fr/1.1.0/) et le
projet suit le [versionnage sémantique](https://semver.org/lang/fr/).

## [Non publié]

### Ajouté
- Rappel de sauvegarde pour les administrateurs : bandeau « Données non sauvegardées » tant qu'aucune sauvegarde automatique n'est active (elles n'écrivent rien sans mot de passe) ou qu'elles se sont arrêtées.
- Installateur Windows (NSIS, sans droits administrateur) produit par l'intégration continue.
- Journaux : un fichier par jour (14 conservés) dans `%LOCALAPPDATA%\Motodesk\logs`, avec les plantages (panic) et leur origine ; carte « Diagnostic » dans Paramètres › Sauvegardes pour ouvrir le dossier.
- Workflow de publication : un tag `vX.Y.Z` construit l'installateur et prépare une version brouillon.

### Corrigé
- Restauration : le redémarrage qui applique une sauvegarde laissait la nouvelle instance sans service de données (elle héritait du port 17300 de l'ancienne). Les sockets d'écoute ne sont plus héritables, et le démarrage attend la libération des ports.
- Restauration : les clés de signature et le secret du réseau d'une sauvegarde étaient écrits dans le trousseau avant d'être vérifiés : une sauvegarde refusée pouvait remplacer la clé en service. Ils ne le sont plus qu'une fois la restauration certaine.
- Restauration : un fichier endommagé disait « mot de passe incorrect » ; le message cite maintenant les deux causes.
- Restauration : la date de dernière sauvegarde est conservée dans les données restaurées (le rappel de sauvegarde ne croit plus qu'aucune n'a jamais été faite).

