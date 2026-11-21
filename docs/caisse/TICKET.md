# Mentions du ticket de caisse

Un ticket (papier ou texte) porte au minimum :

- nom et adresse du commerçant, SIRET (et n° TVA si assujetti) ;
- date et heure, numéro `RC-AAAA-CODE-NNNNN` ;
- désignation, quantité, prix, remises ;
- totaux HT, TVA par taux, TTC ;
- moyen(s) de paiement ;
- opérateur si connu ;
- **DUPLICATA** dès la deuxième impression.

## AGEC (C. env. L541-15-10)

L'impression n'est plus systématique. La caisse affiche :

> Un ticket de caisse vous est remis sur simple demande.

Elle imprime néanmoins si :

- le client le demande ;
- au moins une ligne est un bien sous garantie légale de conformité ;
- le TTC des prestations de service atteint 25,00 €.

Le logiciel calcule cela dans `desktop/src/lib/pos/agec.ts` (`mustPrintTicket`).
