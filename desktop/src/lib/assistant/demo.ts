import type { CanvasArtifact, Suggestion } from "./types";

export const SUGGESTIONS: Suggestion[] = [
  {
    id: "clients",
    label: "Chercher un client",
    description: "Exemple de tableau",
    prompt: "Trouve le client Dupont",
  },
  {
    id: "stock",
    label: "Stock bas",
    description: "Indicateurs de démo",
    prompt: "Quels articles sont en stock bas ?",
  },
  {
    id: "devis",
    label: "Créer un devis",
    description: "Demande d'autorisation",
    prompt: "Prépare un devis pour Dupont Jean — révision 600 km",
  },
  {
    id: "mois",
    label: "Résumé du mois",
    description: "Note sur le canevas",
    prompt: "Fais-moi un résumé d'activité du mois",
  },
];

export const STUB_REPLY =
  "L'assistant n'est pas encore connecté à un modèle. Configurez une clé OpenRouter dans Réglages › Assistant. En attendant, les suggestions ci-dessus montrent le canevas.";

export function demoTurn(id: string): { assistant: string; artifacts: CanvasArtifact[] } {
  const now = () => crypto.randomUUID();
  if (id === "clients") {
    return {
      assistant:
        "Voici les fiches qui correspondent à « Dupont » (données de démonstration).",
      artifacts: [
        {
          id: now(),
          title: "Clients — Dupont",
          kind: "table",
          columns: [
            { key: "name", label: "Nom" },
            { key: "plate", label: "Immatriculation" },
            { key: "phone", label: "Téléphone" },
          ],
          rows: [
            { name: "Dupont Jean", plate: "AB-123-CD", phone: "06 12 34 56 78" },
            { name: "Dupont Marie", plate: "EF-456-GH", phone: "06 98 76 54 32" },
          ],
        },
      ],
    };
  }
  if (id === "stock") {
    return {
      assistant: "Trois références sous le seuil d'alerte (démonstration).",
      artifacts: [
        {
          id: now(),
          title: "Stock bas",
          kind: "kpis",
          items: [
            { label: "Références sous seuil", value: "3", hint: "casques et consommables" },
            { label: "Valeur HT", value: "1 240 €" },
            { label: "Ruptures", value: "1", hint: "filtre à huile 10W40" },
          ],
        },
      ],
    };
  }
  if (id === "devis") {
    return {
      assistant:
        "Je peux préparer un brouillon de devis. Confirmez l'autorisation sur le canevas — aucune écriture n'est faite en phase 0.",
      artifacts: [
        {
          id: now(),
          title: "Autorisation — devis",
          kind: "permission",
          tool: "sales.create_quote",
          summary: "Créer un devis pour Dupont Jean, révision 600 km (démonstration).",
          status: "pending",
        },
      ],
    };
  }
  return {
    assistant: "Voici une synthèse fictive pour illustrer le canevas.",
    artifacts: [
      {
        id: now(),
        title: "Activité du mois",
        kind: "markdown",
        body: `# Activité du mois

**Août 2026** — synthèse de démonstration. Les montants sont TTC.

## Atelier

- 42 passages atelier
- 3 devis en attente
- CA TTC **18 400 €**

## Par canal

| Canal | Passages | CA TTC |
| --- | ---: | ---: |
| Comptoir | 28 | 11 200 € |
| Atelier | 14 | 7 200 € |
| **Total** | **42** | **18 400 €** |

Les chiffres viendront des outils de lecture à la phase 2. Voir le module [Ventes](#/sales) et la fiche \`clients\`.

> Aucune écriture fiscale n'est générée par cette note.
`,
      },
    ],
  };
}
