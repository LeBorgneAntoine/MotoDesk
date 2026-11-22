/** French is the default locale. Keys are stable; values are user-facing copy. */

export const fr = {
  boot: { loading: "Chargement…" },
  titlebar: {
    search: "Rechercher",
    searchPlaceholder: "Rechercher clients, véhicules, plaques…",
    signOut: "Se déconnecter",
    userFallback: "Utilisateur",
    minimize: "Réduire",
    maximize: "Agrandir",
    restore: "Restaurer",
    close: "Fermer",
  },
  nav: {
    dashboard: "Tableau de bord",
    clients: "Clients & Véhicules",
    atelier: "L'Atelier",
    parc: "Parc VN/VO",
    stock: "Inventaire",
    sales: "Ventes",
    billing: "Comptabilité",
    templates: "Modèles",
    shared: "Dossier partagé",
    employees: "Employés",
    data: "Données",
    settings: "Paramètres",
    assistant: "Assistant",
    primary: "Navigation principale",
    comingSoon: "Bientôt",
  },
  pos: {
    office: "Bureau",
    floatHint: "Comptez le fond de caisse avant d'encaisser. Le montant est journalisé.",
    returnTill: "Caisse",
  },
  common: {
    retry: "Réessayer",
    cancel: "Annuler",
    save: "Enregistrer",
    delete: "Supprimer",
    loading: "Chargement…",
    empty: "Aucune donnée",
    error: "Une erreur est survenue",
    close: "Fermer",
    new: "Nouveau",
  },
  errors: {
    network: "Connexion au serveur impossible.",
    loadClients: "Impossible de charger les clients.",
    loadStock: "Impossible de charger l'inventaire.",
    loadTemplates: "Impossible de charger les modèles.",
    loadDashboard: "Impossible de charger le tableau de bord.",
    deleteFailed: "La suppression a échoué.",
  },
  clients: {
    none: "Aucun client pour le moment",
    noneSearch: "Aucun résultat",
  },
} as const;

type NestedKey<T> = T extends object
  ? {
      [K in keyof T]: K extends string
        ? T[K] extends object
          ? `${K}.${NestedKey<T[K]>}`
          : K
        : never;
    }[keyof T]
  : never;

export type MsgKey = NestedKey<typeof fr>;

function lookup(dict: unknown, path: string): string | undefined {
  let cur: unknown = dict;
  for (const part of path.split(".")) {
    if (typeof cur !== "object" || cur === null || !(part in cur)) return undefined;
    cur = (cur as Record<string, unknown>)[part];
  }
  return typeof cur === "string" ? cur : undefined;
}

/** Translate a catalog key. Falls back to the key if missing. */
export function t(key: MsgKey | string): string {
  return lookup(fr, key) ?? key;
}

export const locale = "fr";
