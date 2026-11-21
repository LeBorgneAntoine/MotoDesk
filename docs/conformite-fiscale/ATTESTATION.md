# Attestation individuelle de conformité — modèle à signer

> Ce fichier est un **modèle**. Il n'a aucune valeur tant qu'il n'a pas été
> renseigné, daté et signé sur papier ou par signature électronique par le
> représentant légal de l'entreprise, puis conservé avec les pièces comptables.

## Ce qu'il faut savoir avant de signer

L'article 286 I-3° bis du CGI impose que le logiciel de caisse satisfasse aux
conditions d'inaltérabilité, de sécurisation, de conservation et d'archivage
(ISCA). La preuve de ce respect prend l'une de deux formes : un certificat
délivré par un organisme accrédité (NF525, LNE) **ou** une attestation
individuelle de l'éditeur, conforme au modèle BOI-LETTRE-000242. L'article 125
de la loi de finances pour 2026 a rétabli cette seconde voie le 21 février 2026.

Le BOFiP BOI-TVA-DECLA-30-10-30 définit l'éditeur comme « la personne qui
détient le code source et qui a la maîtrise de la modification des paramètres »
(§ 300) et vise explicitement « un logiciel développé en interne » (§ 45).
Motodesk étant développé et maîtrisé en interne, l'entreprise est à la fois
éditeur et utilisateur : elle s'atteste à elle-même.

Trois conséquences pratiques :

1. **L'attestation nomme une version précise.** Elle doit être réémise à chaque
   version qui touche au code ISCA — c'est-à-dire au crate `motodesk-fiscal`, au
   module `local_server::fiscal`, aux routes `sales` et `pos`, ou aux migrations
   `010_fiscal.sql` et suivantes. La version courante est exposée par la
   constante `SOFTWARE_VERSION` et figure dans chaque entrée du journal.
2. **Un tiers qui modifie les fonctions ISCA invalide l'attestation** et hérite
   de l'obligation (§ 315). Aucune modification du journal fiscal ne doit être
   faite hors du dépôt.
3. **L'attestation ne dispense de rien d'autre.** L'obligation de recevoir des
   factures électroniques par une plateforme agréée à compter du
   1er septembre 2026 reste entière et n'est pas couverte par ce document.

Le détail de ce qui est réellement implémenté, et où, se trouve dans
[ISCA.md](ISCA.md). Ce document est la pièce à produire en cas de contrôle : il
est ce qui rend l'attestation défendable.

---

## Modèle (BOI-LETTRE-000242)

**ATTESTATION INDIVIDUELLE DE CONFORMITÉ**

**d'un logiciel ou système de caisse aux conditions d'inaltérabilité, de
sécurisation, de conservation et d'archivage des données prévues au 3° bis du I
de l'article 286 du code général des impôts**

Je soussigné(e) `<PRÉNOM NOM>`, agissant en qualité de `<QUALITÉ — gérant,
président, …>` de l'entreprise `<DÉNOMINATION SOCIALE>`, dont le siège social
est situé `<ADRESSE COMPLÈTE>`, immatriculée sous le numéro SIREN
`<SIREN>`,

**atteste** que le logiciel de comptabilité, de gestion ou le système de caisse
désigné ci-dessous satisfait aux conditions d'inaltérabilité, de sécurisation,
de conservation et d'archivage des données en vue du contrôle de
l'administration fiscale, prévues au 3° bis du I de l'article 286 du code
général des impôts.

| | |
|---|---|
| Dénomination du logiciel | **Motodesk** |
| Numéro de version | `<VERSION — ex. 0.1.0>` |
| Éditeur | `<DÉNOMINATION SOCIALE>` (logiciel développé en interne) |
| Date de la version | `<AAAA-MM-JJ>` |
| Date de première installation | `<AAAA-MM-JJ>` |
| Périmètre attesté | Fonctionnalité de caisse et d'encaissement : émission des factures, tickets de caisse et avoirs, enregistrement des règlements, sessions de caisse, clôtures et archivage fiscal |

L'attestation ne porte pas sur les fonctionnalités hors périmètre de
l'encaissement (gestion des clients, du stock, de l'atelier et des modèles de
documents), conformément au § 40 du BOI-TVA-DECLA-30-10-30 qui limite
l'obligation à « la seule fonctionnalité de caisse enregistreuse et
encaissement ».

Je m'engage à produire, à la demande de l'administration, la documentation
technique décrivant la mise en œuvre de ces quatre conditions, et à établir une
nouvelle attestation à chaque version modifiant ces fonctions.

Fait à `<VILLE>`, le `<AAAA-MM-JJ>`.

Signature et cachet de l'entreprise :

<br><br><br>

---

## Registre des attestations émises

À compléter à chaque émission. Une attestation périmée qui traîne sans que l'on
sache quelle version elle couvrait ne vaut rien lors d'un contrôle.

| Version | Date de la version | Date de signature | Signataire | Motif de la réémission |
|---------|--------------------|-------------------|------------|------------------------|
| 0.1.0 | | | | Première mise en service du journal fiscal |
