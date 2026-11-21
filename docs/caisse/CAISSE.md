# Caisse enregistreuse

Mode till de Motodesk : un appareil désigné devient un comptoir tactile, branché sur le même journal fiscal que les factures du bureau.

## Activer l'appareil

1. Réglages › **Caisse** (administrateur).
2. Cocher **Utiliser cet appareil comme caisse**.
3. Le réglage est local à la machine (`pos.store.json`). Il ne se synchronise pas vers les autres postes.
4. Au prochain démarrage (ou via *Ouvrir la caisse maintenant*), l'encaissement s'ouvre en plein écran.

**Bureau** (barre du haut) ramène le shell atelier sans désactiver le rôle. Pour arrêter définitivement, décocher le réglage.

## Journée type

1. **Ouvrir la session** — saisir le fond de caisse.
2. Scanner un code-barres (douchette clavier) ou toucher un article.
3. Encaisser : espèces (rendu, tiroir), carte (simulateur ou confirmation manuelle), chèque, ou un mélange.
4. Le ticket n'est **pas** imprimé tout seul (AGEC). Le proposer ; il s'imprime aussi pour une prestation ≥ 25 € TTC ou un bien sous garantie légale.
5. **Mettre en attente** pour servir quelqu'un d'autre ; rappeler depuis *Attente*.
6. **Clôturer** — compter le tiroir à l'aveugle. L'écart s'affiche ensuite. Option : clôturer aussi la journée (Z), la même clôture journalière que dans Comptabilité. Une journée clôturée est définitive : plus aucune vente ni aucun règlement ne peut y être enregistré, la caisse reprend le lendemain. La clôture n'est possible que session fermée ; les clôtures mensuelle et annuelle restent réservées aux périodes terminées.

Une erreur après validation se corrige par un **avoir**, jamais en modifiant le ticket.

## Périphériques

- Imprimante 80 mm : indiquer le port (`COM3`) ou le chemin RAW. Sans cible, le ticket est quand même scellé.
- Tiroir : impulsion ESC/POS sur un encaissement espèces, si l'imprimante est configurée.
- TPE : *simulateur* pour les tests, ou *manuel* si le terminal est autonome. Le protocole Caisse (Concert) n'est pas livré : il est licencié par l'Association du Paiement.
