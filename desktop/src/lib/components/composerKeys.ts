/** Enter sends; Shift+Enter inserts a newline. Any other key is ignored. */
export type ComposerEnterAction = "send" | "newline" | "none";

export function composerEnterAction(e: {
  key: string;
  shiftKey: boolean;
}): ComposerEnterAction {
  if (e.key !== "Enter") return "none";
  return e.shiftKey ? "newline" : "send";
}
