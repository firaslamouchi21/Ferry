export type ToastTone = "info" | "error";

export interface Toast {
  id: number;
  text: string;
  tone: ToastTone;
}

type Listener = (toasts: Toast[]) => void;

let toasts: Toast[] = [];
const listeners = new Set<Listener>();
let nextId = 1;

function emit(): void {
  for (const listener of listeners) listener(toasts);
}

export function pushToast(text: string, tone: ToastTone = "info"): void {
  const id = nextId++;
  toasts = [...toasts, { id, text, tone }];
  emit();
  const ttl = tone === "error" ? 8000 : 5000;
  setTimeout(() => {
    toasts = toasts.filter((toast) => toast.id !== id);
    emit();
  }, ttl);
}

export function subscribeToasts(listener: Listener): () => void {
  listeners.add(listener);
  listener(toasts);
  return () => {
    listeners.delete(listener);
  };
}
