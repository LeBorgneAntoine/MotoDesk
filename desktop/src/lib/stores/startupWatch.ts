/**
 * Decide, while the application starts, whether the data service came up. Pure over the checks
 * it is given so the rules can be tested without a window.
 */
import type { StartupFailure } from "$lib/api/startup";

export interface StartupChecks {
  /** The service answers. */
  healthy: () => Promise<boolean>;
  /** What the shell recorded, if the service failed. */
  recorded: () => Promise<StartupFailure | null>;
  sleep: (ms: number) => Promise<void>;
}

/** How long a start may take before it is called stuck. */
export const START_BUDGET_MS = 30_000;
const INTERVAL_MS = 400;

export const STUCK: StartupFailure = {
  kind: "unknown",
  title: "Motodesk ne répond pas",
  message:
    "Le service de données ne répond pas après 30 secondes. Relancez Motodesk ; si cela se répète, " +
    "le dossier des journaux contient le détail à transmettre à l'assistance.",
};

/** `null` once the service answers; otherwise why it will not. */
export async function watchStartup(checks: StartupChecks, budgetMs = START_BUDGET_MS): Promise<StartupFailure | null> {
  for (let waited = 0; waited < budgetMs; waited += INTERVAL_MS) {
    if (await checks.healthy()) return null;
    const failure = await checks.recorded().catch(() => null);
    if (failure) return failure;
    await checks.sleep(INTERVAL_MS);
  }
  return STUCK;
}
