import { For } from "solid-js";
import { dismiss, toasts } from "../lib/toast";
import { runActionById } from "../lib/actions";

export function Toasts() {
  return (
    <div class="toasts" role="status" aria-live="polite">
      <For each={toasts()}>
        {(t) => (
          <div class="toast" classList={{ error: t.kind === "error" }} role={t.kind === "error" ? "alert" : undefined}>
            <span>{t.text}</span>
            {t.action && (
              <button
                class="toast-action"
                onClick={() => {
                  dismiss(t.id);
                  runActionById(t.action!.actionId, t.action!.args);
                }}
              >
                {t.action.label}
              </button>
            )}
            <button class="toast-close" aria-label="Dismiss" onClick={() => dismiss(t.id)}>
              ×
            </button>
          </div>
        )}
      </For>
    </div>
  );
}
