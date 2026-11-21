# Architecture de la caisse

## Deux étages, un seul journal

Le panier reste en mémoire jusqu'à `POST /api/pos/checkout`. Cet appel, dans **une** transaction SQLite :

1. crée la vente et ses lignes ;
2. scelle un ticket `RC-…` (ou une facture `FA-…`) dans `fiscal_journal` ;
3. enregistre le ou les règlements ;
4. décrémente le stock des articles (pas des services).

Un crash au milieu n'écrit rien. Un ticket non soldé est refusé avant toute écriture fiscale.

Les factures du module *Ventes* empruntent le même `seal_in_tx`. La caisse n'invente pas une deuxième vérité.

## Rôle machine

`pos.store.json` (plugin Tauri Store), jamais `app_settings`. Ces dernières sont répliquées en mesh (`EntityType::AppSetting`) : un booléen « caisse » y ferait basculer tout l'atelier.

## TPE

`local_server/pos/terminal.rs` expose `charge(mode, amount, force)` :

| Mode | Comportement |
|---|---|
| `simulator` | `force=accept\|refuse\|timeout` |
| `manual` | le caissier confirme après un TPE autonome |

La carte est débitée **avant** checkout. Un refus ne crée pas de ticket. Un pilote Caisse-AP se branche derrière le même contrat, après achat de la spécification.

## ESC/POS

`local_server/pos/escpos.rs` construit le tampon (init, texte, coupe, `ESC p` tiroir). L'écriture vers le port est best-effort : l'échec d'imprimante n'annule pas la vente.

## Sessions

Table `pos_sessions` + événements `session_opened` / `session_closed` / `drawer_opened`. La clôture de session calcule l'attendu (fond + espèces de la session) et l'écart. Elle n'est **pas** la clôture J : l'option *Z* appelle `closures::close(Day)` ensuite.
