# Dossier partagé

Un espace pour les fichiers sensibles de l'atelier (comptabilité, contrats, pièces d'identité, bilans) : **chiffrés sur le disque**, **copiés sur chaque poste du réseau local**, avec un **accès choisi dossier par dossier**.

Ce document décrit le fonctionnement et les garanties, et dit clairement ce qui n'est pas protégé.

## Ce que voit l'utilisateur

L'entrée « Dossier partagé » du menu ouvre un gestionnaire de fichiers : dossiers imbriqués (8 niveaux au plus), glisser-déposer ou sélecteur pour ajouter des fichiers (100 Mo au plus chacun), téléchargement d'un clic.

| | Administrateurs | Employés |
|---|---|---|
| Voir un dossier | tous | seulement ceux ouverts à « Tous les employés » |
| Ajouter et ouvrir des fichiers | oui | oui, dans les dossiers qu'ils voient |
| Créer, modifier, supprimer des dossiers | oui | non |
| Renommer, supprimer des fichiers | oui | non (ils peuvent annuler leur propre envoi en cours) |
| Historique des accès | oui | non |

Un dossier est atteignable seulement si **lui et tous ses parents** sont visibles pour l'utilisateur. Un dossier qu'il ne peut pas voir se comporte comme un dossier qui n'existe pas (réponse « introuvable », pas « interdit »).

L'historique garde qui a ouvert, ajouté ou supprimé quoi. Il est tenu sur chaque poste séparément.

## Chiffrement

- Chaque fichier a **sa propre clé** (32 octets aléatoires).
- Cette clé est elle-même chiffrée (AES-256-GCM) par une **clé de l'atelier** dérivée (HKDF-SHA256) du secret du réseau que tous les postes de l'atelier possèdent déjà. Ce secret est dans le trousseau Windows et dans les sauvegardes chiffrées : il n'y a **aucun mot de passe de plus à saisir ni à perdre**.
- Le fichier est découpé en morceaux de 1 Mo, chacun chiffré en AES-256-GCM avec un nonce aléatoire. Les données authentifiées de chaque morceau nomment le contenu, sa position, le nombre de morceaux et la taille du fichier : un morceau ne peut pas être déplacé, rejoué ni supprimé sans que le déchiffrement échoue.
- Les fichiers sont stockés dans la base de données, donc **inclus dans les sauvegardes**.
- Conséquence du chiffrement par clé enveloppée : changer un jour la clé de l'atelier (par exemple après la perte d'un poste) revient à **re-chiffrer de petites clés**, pas à re-chiffrer les fichiers. Ce changement de clé n'est pas encore construit.

## Copie entre les postes

Chaque poste du réseau tire ce qu'il lui manque des autres, par le même canal signé que les comptes du réseau (secret du réseau, requêtes horodatées, jamais rejouables). En trois temps :

1. **Résumé** : une empreinte de ce que le poste détient. Si celle du pair est identique, c'est fini (le cas courant : une seule petite requête).
2. **Lignes** : les dossiers et fichiers qui manquent ou sont plus anciens. **La modification la plus récente l'emporte** ; à égalité, une suppression l'emporte, puis la plus grande empreinte, de sorte que tous les postes choisissent la même version. Une suppression voyage comme une pierre tombale et efface le contenu chiffré sur les autres postes.
3. **Morceaux** : le contenu chiffré des fichiers connus mais pas encore complets. Chaque morceau est déchiffré (avec la clé du fichier) avant d'être gardé : un morceau altéré est refusé. Un fichier n'apparaît et n'est ouvrable **qu'une fois tous ses morceaux arrivés**. Une copie interrompue reprend où elle s'était arrêtée.

Une modification locale prévient les autres postes pour qu'ils n'attendent pas leur prochain tour (45 secondes au plus). Un poste hors du réseau local, ou avec le secret d'un autre atelier, est refusé.

## Limites connues

- **Les noms de fichiers et de dossiers ne sont pas chiffrés.** Un « Salaires.xlsx » se voit dans la base de données. Le contenu, lui, est chiffré.
- **Pas de corbeille ni de versions** : une suppression est définitive, partout.
- **Pas de déplacement de fichier dans l'écran** (l'API le permet).
- **Changement de clé de l'atelier non construit** (voir plus haut).
- **Copie entre postes : réseau local uniquement.** Une installation en ligne garde les fichiers sur son poste ; la synchronisation avec le système en ligne viendra ensuite, en réutilisant le même format (lignes à pierres tombales et morceaux chiffrés).
- Deux postes qui créent en même temps deux dossiers de même nom donnent deux dossiers de même nom (la règle d'unicité est vérifiée à la création, pas imposée entre postes).
- Un employé qui a accès au disque d'un poste et à son trousseau lit ce que ce poste peut lire : les règles d'accès sont appliquées par l'application, le chiffrement protège contre le vol ou la copie du disque et des sauvegardes.

## Où c'est dans le code

- `desktop/src-tauri/src/local_server/shared_folder/crypto.rs` : chiffrement.
- `.../shared_folder/mod.rs` : dossiers, accès, envoi par morceaux, lecture, historique.
- `.../shared_folder/replication.rs` : copie entre postes. Routes `/mesh/shared/*` dans `routes/mesh.rs`.
- `.../routes/shared.rs` : API locale `/api/shared/*`.
- `desktop/src/routes/SharedFolder.svelte` : l'écran.
