/** UI sync indicator + signal for views to reload after a manual resync. */
class SyncUiStore {
  phase = $state<"idle" | "syncing" | "error">("idle");
  lastMessage = $state<string | null>(null);
  revision = $state(0);

  startSync(): void {
    this.phase = "syncing";
    this.lastMessage = null;
  }

  finishSync(message: string): void {
    this.phase = "idle";
    this.lastMessage = message;
    this.revision += 1;
    if (typeof window !== "undefined") {
      window.dispatchEvent(new CustomEvent("motodesk:resync"));
    }
  }

  failSync(message: string): void {
    this.phase = "error";
    this.lastMessage = message;
  }
}

export const syncUi = new SyncUiStore();
