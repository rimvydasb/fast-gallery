import { createSignal } from "solid-js";

export interface Toast {
  id: number;
  text: string;
  kind: "info" | "error";
}

let seq = 0;
export const [toasts, setToasts] = createSignal<Toast[]>([]);

/** Shows a short message. Errors stay longer (FILE-08). */
export function toast(text: string, kind: Toast["kind"] = "info") {
  const id = ++seq;
  setToasts([...toasts().slice(-3), { id, text, kind }]);
  setTimeout(() => dismiss(id), kind === "error" ? 7000 : 3500);
}

export function dismiss(id: number) {
  setToasts(toasts().filter((t) => t.id !== id));
}
