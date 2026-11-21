export type ToastTone = "info" | "success" | "error";

export interface Toast {
  id: number;
  message: string;
  tone: ToastTone;
}

class ToastStore {
  items = $state<Toast[]>([]);
  private nextId = 1;

  show(message: string, tone: ToastTone = "info", ms = 4200): void {
    const id = this.nextId++;
    this.items = [...this.items, { id, message, tone }];
    window.setTimeout(() => this.dismiss(id), ms);
  }

  success(message: string): void {
    this.show(message, "success");
  }

  error(message: string): void {
    this.show(message, "error", 7000);
  }

  dismiss(id: number): void {
    this.items = this.items.filter((t) => t.id !== id);
  }
}

export const toast = new ToastStore();
