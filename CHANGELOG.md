# Journal des modifications

Le format suit [Keep a Changelog](https://keepachangelog.com/fr/1.1.0/) et le
projet suit le [versionnage sémantique](https://semver.org/lang/fr/).

## [Non publié]

### Modifié
- **Nouvelle interface, native.** L'application n'est plus une page web dans une fenêtre : son interface est écrite en Rust (GPUI), plus rapide à ouvrir et à l'usage. Les écrans, les données, le journal fiscal et la synchronisation sont les mêmes ; les réglages de chaque poste (caisse, assistant) sont repris.
- Imprimer un document ou la clé de récupération ouvre désormais un PDF dans le lecteur du poste, d'où l'impression se lance. La trace au journal et la mention DUPLICATA sont inchangées.
- Les menus de chaque écran sont dans la barre de titre, avec leurs raccourcis.
- Deux réglages d'apparence disparaissent : les coins arrondis (tout est carré) et le flou.

### Ajouté
- Réseau local : liste des postes et « Retirer ce poste » pour un poste perdu ou volé. La clé du réseau change, l'administrateur choisit un nouveau mot de passe, et chaque autre poste est reconnecté par un administrateur (ses données sont conservées). Un poste qui a perdu sa clé se reconnecte de la même façon.
- Sur un poste neuf, « Restaurer une sauvegarde » dès l'écran d'accès, sans créer d'abord un réseau.
- Réseau local : une clé de récupération est donnée à la création du réseau. Elle permet, depuis « Mot de passe perdu ? », de redonner l'accès à un compte ; elle est remplacée après usage et peut être renouvelée par un administrateur.
- Mises à jour : un bandeau « Nouvelle version disponible » (administrateurs) avec les notes de la version, installation en un clic, recherche manuelle ou automatique dans Réglages › Sauvegardes. Les installateurs sont signés ; l'application refuse tout installateur dont la signature ne correspond pas à sa clé publique. Voir `docs/mises-a-jour.md`.
- Licence : code source consultable, tous droits réservés (`LICENSE`).
- Audit des dépendances : un contrôle des failles connues (`cargo audit`, `npm audit`) à chaque changement de dépendances et chaque semaine. Les deux exceptions acceptées sont justifiées dans `.cargo/audit.toml`.
- Dossier partagé : un espace de fichiers sensibles (comptabilité, contrats, pièces d'identité) chiffrés sur le disque, copiés sur chaque poste du réseau local, avec un accès choisi dossier par dossier (administrateurs ou tous les employés) et un historique des accès. Voir `docs/dossier-partage.md`.
- Rappel de sauvegarde pour les administrateurs : bandeau « Données non sauvegardées » tant qu'aucune sauvegarde automatique n'est active (elles n'écrivent rien sans mot de passe) ou qu'elles se sont arrêtées.
- Installateur Windows (NSIS, sans droits administrateur) produit par l'intégration continue.
- Journaux : un fichier par jour (14 conservés) dans `%LOCALAPPDATA%\Motodesk\logs`, avec les plantages (panic) et leur origine ; carte « Diagnostic » dans Paramètres › Sauvegardes pour ouvrir le dossier.
- Workflow de publication : un tag `vX.Y.Z` construit l'installateur et prépare une version brouillon.

### Corrigé
- Réseau local : les échanges entre postes sont désormais chiffrés (ils n'étaient que signés : clients, ventes et empreintes de mots de passe circulaient en clair). Tous les postes doivent être mis à jour ensemble.
- Réseau local : seul un administrateur peut ajouter un poste au réseau (le compte de n'importe quel employé suffisait).
- Réseau local : une coupure pendant la création du réseau ou l'adhésion d'un poste ne laisse plus le poste ni le réseau à moitié configurés ; un enregistrement de compte qui échoue ne le fait plus disparaître.
- Réseau local : un identifiant accentué (« Élodie ») fonctionne quelle que soit la casse saisie, y compris depuis un nouveau poste.
- Réseau local : ouvrir de nombreuses tentatives d'adhésion en même temps ne contourne plus le verrouillage après cinq essais.
- Réseau local : se déconnecter n'efface plus le secret du réseau (le poste cessait d'en faire partie au redémarrage suivant).
- Réseau local : « Synchroniser » (Paramètres › Synchronisation) ne déconnecte plus la personne connectée.
- Réseau local : désactiver ou supprimer un compte, ou changer son mot de passe, ferme ses sessions ouvertes ; un changement de rôle s'applique sans se reconnecter.
- Restaurer une sauvegarde ne reconnecte plus la personne qui était connectée au moment de la sauvegarde.
- Réseau local : une vente en caisse ou un avoir déduit maintenant le stock sur les autres postes ; une fiche reçue avant ce à quoi elle se rattache (véhicule avant son client) n'est plus perdue ; un poste resté éteint récupère aussi les modifications plus anciennes que la dernière reçue.
- Réseau local : chaque poste détient désormais une copie du journal fiscal des autres (l'ouverture de caisse n'était jamais envoyée, et tout le reste était refusé).
- Restauration sans cloud : le poste repart avec une nouvelle caisse et une nouvelle série de numéros, pour ne jamais réattribuer un numéro de facture émis après la sauvegarde.
- Dépendances : mise à jour des bibliothèques réseau et TLS (rustls, quinn, url…) et de la lecture des anciennes sauvegardes ZIP, ainsi que des outils de développement (vite 6, vitest 4) : plus aucune faille connue côté application, ni côté outils.
- Restauration : le redémarrage qui applique une sauvegarde laissait la nouvelle instance sans service de données (elle héritait du port 17300 de l'ancienne). Les sockets d'écoute ne sont plus héritables, et le démarrage attend la libération des ports.
- Restauration : les clés de signature et le secret du réseau d'une sauvegarde étaient écrits dans le trousseau avant d'être vérifiés : une sauvegarde refusée pouvait remplacer la clé en service. Ils ne le sont plus qu'une fois la restauration certaine.
- Restauration : un fichier endommagé disait « mot de passe incorrect » ; le message cite maintenant les deux causes.
- Restauration : la date de dernière sauvegarde est conservée dans les données restaurées (le rappel de sauvegarde ne croit plus qu'aucune n'a jamais été faite).

