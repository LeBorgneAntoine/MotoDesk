/** Rules of the screen that hands over a recovery key (the key itself is made by the server). */

/** Whether what was typed is the last group of the key, however it was typed. */
export function lastGroupMatches(key: string, typed: string): boolean {
  const clean = (s: string) =>
    s
      .toUpperCase()
      .replace(/[^0-9A-Z]/g, "")
      .replace(/O/g, "0")
      .replace(/[IL]/g, "1");
  const last = key.split("-").at(-1) ?? "";
  return last.length > 0 && clean(typed) === clean(last);
}

/** A page holding nothing but the key and what it is for. */
export function recoveryKeyPage(key: string, date: Date): string {
  const day = new Intl.DateTimeFormat("fr-FR", { dateStyle: "long" }).format(date);
  return `<!doctype html>
<html lang="fr"><head><meta charset="utf-8" /><title>Clé de récupération Motodesk</title>
<style>body{font-family:sans-serif;margin:20mm}code{display:block;font-size:20pt;margin:10mm 0;letter-spacing:1px}</style>
</head><body>
<h1>Motodesk : clé de récupération</h1>
<p>Créée le ${day}.</p>
<code>${key.replace(/[^0-9A-Z-]/g, "")}</code>
<p>Elle permet de reprendre la main sur les comptes de l'atelier si un mot de passe est perdu.
Rangez ce papier en lieu sûr, loin des postes. Elle ne sert qu'une fois : la suivante la remplace.
Elle n'ouvre pas les sauvegardes, qui ont leur propre mot de passe.</p>
</body></html>`;
}
