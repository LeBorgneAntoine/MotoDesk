const TITLE_MAX = 48;

export function titleFromPrompt(prompt: string): string {
  const compact = prompt.replace(/\s+/g, " ").trim();
  if (!compact) return "Nouvelle conversation";
  if (compact.length <= TITLE_MAX) return compact;
  return `${compact.slice(0, TITLE_MAX - 1).trimEnd()}…`;
}

export function filterConversations<T extends { title: string }>(
  conversations: T[],
  query: string,
): T[] {
  const q = query.trim().toLowerCase();
  if (!q) return conversations;
  return conversations.filter((c) => c.title.toLowerCase().includes(q));
}

export function formatRelative(iso: string, nowMs = Date.now()): string {
  const then = new Date(iso).getTime();
  if (!Number.isFinite(then)) return "";
  const delta = Math.max(0, nowMs - then);
  const minute = 60_000;
  const hour = 60 * minute;
  const day = 24 * hour;
  if (delta < minute) return "à l'instant";
  if (delta < hour) {
    const n = Math.floor(delta / minute);
    return n === 1 ? "il y a 1 min" : `il y a ${n} min`;
  }
  if (delta < day) {
    const n = Math.floor(delta / hour);
    return n === 1 ? "il y a 1 h" : `il y a ${n} h`;
  }
  if (delta < 2 * day) return "hier";
  return new Intl.DateTimeFormat("fr-FR", { day: "numeric", month: "short" }).format(
    new Date(then),
  );
}
