# Réseau local (sans cloud) et mise en ligne

Motodesk fonctionne de trois façons, qui se combinent :

- **Un poste seul**, hors ligne.
- **Un réseau local** : les postes d'un atelier forment un maillage et partagent les mêmes données et les mêmes comptes, **sans aucun serveur**.
- **En ligne** : un cloud relie les sites et sauvegarde les données.

L'écran d'accès a deux onglets, **Connexion** et **Inscription**, et un interrupteur sous la carte, **En ligne ⇄ Local**. En ligne, ce sont les comptes du cloud. En local, ce sont les comptes **du réseau de l'atelier**.

## Parcours

1. **Poste A, Inscription › Local.** On crée le réseau local : un identifiant, un mot de passe et sa confirmation. Ce premier compte est l'administrateur. Le poste A est le premier poste du maillage. Si un réseau de l'atelier est déjà visible, il faut confirmer qu'on en veut un nouveau.
2. **Poste A, Employés.** L'administrateur crée les comptes des autres personnes (identifiant, mot de passe, rôle).
3. **Poste B, Connexion › Local.** Un **administrateur** saisit son identifiant et son mot de passe (celui d'un employé ne suffit pas à ajouter un poste ; les employés se connectent ensuite). Le poste B **cherche sur le réseau un poste de l'atelier** déjà allumé (par mDNS ; ce n'est pas forcément l'administrateur), **prouve qu'il connaît le mot de passe sans l'envoyer**, reçoit de quoi rejoindre le maillage, puis récupère les comptes et les données et s'ajoute au maillage.
4. **Garde-fou.** Un poste qui n'a jamais rejoint le réseau **ne peut pas** se connecter si aucun poste du réseau n'est allumé : il n'a ni comptes ni données à vérifier. Une fois rejoint, il possède sa propre copie des comptes et ouvre une session seul.

## Ce qui est construit

| Brique | Où |
|---|---|
| **Comptes du réseau**, répliqués entre les postes : le plus récent gagne, une suppression est une « pierre tombale » qui se réplique. Un identifiant ne peut pas être réutilisé par un autre compte (le second est mis de côté). Au moins un administrateur actif doit toujours rester. | `local_accounts.rs` |
| **Création de comptes par l'administrateur** : création, rôle, désactivation, suppression, changement de mot de passe. Les hachages ne quittent jamais le poste vers l'écran. | `local_accounts.rs`, `routes/network_accounts.rs`, `routes/NetworkAccounts.svelte` |
| **Preuve de mot de passe sur le réseau** : SRP-6a avec un secret dérivé par Argon2id. Le mot de passe ne circule jamais ; la clé de session chiffre (AES-256-GCM) le paquet d'adhésion (identifiant d'entreprise, secret du maillage, comptes). Le poste qui est rejoint prouve à son tour qu'il détient le compte : un poste imposteur est détecté. | `mesh/join.rs` |
| **Protection contre les essais** : 5 échecs verrouillent l'identifiant une minute ; un identifiant inconnu reçoit une réponse indiscernable d'un identifiant connu jusqu'à la preuve. | `mesh/join.rs` |
| **Découverte sans secret** : un poste vierge garde les postes vus par mDNS (toute entreprise) comme candidats, plus les pairs statiques pour les réseaux qui bloquent le multicast. | `mesh/mod.rs` |
| **Rattrapage par vecteurs de versions** : `POST /mesh/pull` (signé avec le secret du maillage) renvoie ce qui manque, y compris les trous au milieu d'une histoire, par lots ; une modification arrivée avant ce à quoi elle se rattache attend et est réessayée (`mesh/deferred.rs`) ; un nouveau poste reçoit toute l'histoire, un poste qui était éteint seulement ce qui s'est passé. Les comptes s'échangent dans les deux sens (`POST /mesh/accounts`). | `mesh/catchup.rs`, `routes/mesh.rs` |
| **Envoi aux pairs depuis un curseur** : sans cloud, rien n'est jamais « synchronisé », et l'ancien envoi renvoyait toujours les 200 plus anciennes modifications. | `sync_log.rs`, `sync_engine.rs` |
| **Identité de l'entreprise hors ligne** (nom, dénomination, logo) et profil légal et fiscal. | `routes/company.rs` |
| **Mise en ligne** par un administrateur : le cloud crée l'entreprise sous les identifiants que le poste possède déjà (le journal fiscal signé garde des références valides), puis la synchronisation envoie tout. | `routes/auth.rs`, `routes/GoOnline.svelte` (et, côté cloud, l'inscription qui accepte `company_id` et `user_id`) |
| Interrupteur En ligne ⇄ Local et quatre formulaires. | `routes/AccessSwitch.svelte`, `LocalLogin.svelte`, `LocalRegister.svelte` |
| **Arrière-plan** : fermer la fenêtre la **cache** sans arrêter Motodesk (icône près de l'horloge : clic pour rouvrir, menu pour quitter) ; l'application **démarre avec la session Windows**, cachée (`--background`) ; la lancer une seconde fois **montre l'instance en cours** au lieu d'en démarrer une autre (deux copies se disputeraient les mêmes ports et la même base). Activé par défaut pour un poste d'un réseau local, réglable dans *Réglages › Synchronisation*, avec un bouton « Quitter maintenant ». | `background.rs`, `routes/background.rs`, `routes/BackgroundCard.svelte` |

Tous les échanges entre postes sont **chiffrés et authentifiés** (AES-256-GCM sous une clé dérivée du secret du maillage, horodatage, nonce à usage unique), dans les deux sens (`mesh/transport.rs`) ; l'adhésion a son propre scellement. Un poste d'une autre version d'échange est ignoré et signalé à l'écran : mettre à jour tous les postes ensemble. Les tests (`mesh/network_tests.rs`) font tourner deux postes avec un vrai serveur HTTP entre eux : adhésion avec les identifiants d'un employé, rattrapage, mauvais mot de passe, identifiant inconnu, réseau éteint, verrouillage, poste imposteur, requêtes non signées.

## Ce qu'il reste à faire

| Sujet | État |
|---|---|
| **Vrai service Windows**, qui démarre au démarrage de l'ordinateur, avant toute ouverture de session. | **Non fait**, par choix : l'arrière-plan actuel démarre à l'ouverture de la session Windows et vit dans le processus de l'application. Un vrai service demanderait de séparer le serveur de données de la fenêtre. Un poste dont personne n'a ouvert de session Windows n'est donc pas dans le maillage. |
| **Un nouveau poste a son propre registre fiscal** : il ne copie jamais la base d'un autre poste (deux postes qui signeraient la même chaîne la forkeraient). | Vrai par construction (il part d'une base vierge et reçoit des modifications), mais **pas encore éprouvé** par un test de caisse sur deux postes. |
| **Mise en ligne de tout le réseau** : reprendre les comptes dans le cloud avec leurs empreintes Argon2 (mêmes mots de passe) et prévenir les autres postes. | **À faire.** Aujourd'hui seul le poste qui met en ligne reçoit un compte en ligne. |
| **Caisse retirée par une restauration** : ses périodes encore ouvertes ne reçoivent plus de clôture, puisque plus aucun poste ne l'écrit. | Limite connue, à signaler à l'expert-comptable. |

## Décisions et limites

| Sujet | Décision |
|---|---|
| Poste perdu ou volé | **Retirer ce poste** (Employés) : nouveau secret du maillage, nouveau mot de passe pour l'administrateur, clés du dossier partagé réemballées. Les autres postes se **reconnectent** un par un avec un compte administrateur (même échange que l'adhésion) et gardent leurs données. Ce que le poste retiré contient déjà reste lisible par qui le détient : la protection du compte Windows et le chiffrement du disque sont ce qui le garde. Ses copies des empreintes de mots de passe peuvent être attaquées hors ligne : les changer tous. |
| Deux retraits sans contact entre les postes | Deux secrets différents du même âge : reconnecter les postes sur l'un des deux tranche. |
| Restaurer une sauvegarde sur un poste du réseau | Le poste repart comme un **nouvel appareil** : nouvelle caisse, nouvelle série de numéros, nouveau journal. Ce que l'ancienne caisse a fait après la sauvegarde revient des autres postes. Restaurer n'annule donc rien sur le réseau : les autres postes renvoient les modifications faites depuis. |
| Mot de passe sur le réseau | Jamais en clair : SRP-6a. La bibliothèque `srp` indique elle-même n'avoir fait l'objet d'aucun audit indépendant : à relire avant une mise en production. Un mot de passe faible reste devinable au rythme permis par le verrouillage. |
| Un poste vierge sans pair joignable | **Refusé**, par conception. |
| Récupération d'un mot de passe oublié | Un administrateur le réinitialise. Si aucun administrateur ne peut se connecter : la **clé de récupération**, donnée une seule fois à la création du réseau (125 bits, seule son empreinte est gardée et répliquée), redonne un mot de passe et le rôle d'administrateur à un compte, sur n'importe quel poste du réseau ; elle est alors remplacée. Clé et mot de passe perdus tous deux : pas de porte dérobée. La clé n'ouvre pas les sauvegardes. |
| Une installation déjà liée à un cloud devient-elle un réseau local ? | **Non** : ce serait mélanger deux comptabilités. |
| Retour en arrière après la mise en ligne | **Non**, volontairement. |
| Rattacher un réseau local à une entreprise **déjà existante** sur le cloud | **Non couvert** (fusion de deux historiques et de deux chaînes fiscales). |
| Cloud aux inscriptions fermées | La mise en ligne y est refusée tant que l'opérateur ne les ouvre pas ; un code de création serait la suite logique. |
| Deux ateliers sur le même réseau | Chacun a son empreinte d'entreprise ; l'adhésion passe par la preuve du mot de passe, qui échoue sur l'autre atelier. |
| Réseau partagé avec le Wi-Fi des clients | À éviter : la découverte et les échanges supposent un réseau de confiance. |
