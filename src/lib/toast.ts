import { createSignal } from "solid-js";

/** A toast button runs an action of the registry. */
export interface ToastAction {
  label: string;
  actionId: string;
  args?: unknown;
}

export interface Toast {
  id: number;
  text: string;
  kind: "info" | "error";
  action?: ToastAction;
}

let seq = 0;
export const [toasts, setToasts] = createSignal<Toast[]>([]);

/** Shows a short message. Errors stay longer (FILE-08); a toast with an action stays 10 s. */
export function toast(text: string, kind: Toast["kind"] = "info", action?: ToastAction) {
  const id = ++seq;
  setToasts([...toasts().slice(-3), { id, text, kind, action }]);
  setTimeout(() => dismiss(id), action ? 10000 : kind === "error" ? 7000 : 3500);
}

export function dismiss(id: number) {
  setToasts(toasts().filter((t) => t.id !== id));
}
