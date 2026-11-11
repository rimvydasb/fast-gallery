import { For } from "solid-js";
import { dismiss, toasts } from "../lib/toast";

export function Toasts() {
  return (
    <div class="toasts" role="status" aria-live="polite">
      <For each={toasts()}>
        {(t) => (
          <div class="toast" classList={{ error: t.kind === "error" }} role={t.kind === "error" ? "alert" : undefined}>
            <span>{t.text}</span>
            <button class="toast-close" aria-label="Dismiss" onClick={() => dismiss(t.id)}>
              ×
            </button>
          </div>
        )}
      </For>
    </div>
  );
}
