import { createEffect, createSignal, Show } from "solid-js";
import { baseName } from "../lib/api";
import { renameFile, renameRequest, setRenameRequest } from "../lib/store";
import { stemLength, validateFileName } from "../lib/rename";
import { Icon, ICONS } from "./Toolbar";

/** FILE-14: asks for a new name for one file. */
export function RenameDialog() {
  const [name, setName] = createSignal("");
  const [error, setError] = createSignal<string | null>(null);
  const [busy, setBusy] = createSignal(false);
  let input: HTMLInputElement | undefined;

  createEffect(() => {
    const req = renameRequest();
    setError(null);
    setBusy(false);
    if (!req) return;
    const n = baseName(req);
    setName(n);
    queueMicrotask(() => {
      input?.focus();
      input?.setSelectionRange(0, stemLength(n));
    });
  });

  const close = () => setRenameRequest(null);

  const submit = async () => {
    const req = renameRequest();
    if (!req || busy()) return;
    const value = name().trim();
    if (value === baseName(req)) return close();
    const invalid = validateFileName(value);
    if (invalid) return setError(invalid);
    setBusy(true);
    const err = await renameFile(req, value);
    // The dialog may have been closed or opened for another file while the request ran.
    if (renameRequest() !== req) return;
    setBusy(false);
    if (err) setError(err);
  };

  const onKey = (e: KeyboardEvent) => {
    if (e.key === "Escape") {
      e.preventDefault();
      close();
    } else if (e.key === "Enter" && !e.isComposing) {
      e.preventDefault();
      submit();
    }
    e.stopPropagation();
  };

  return (
    <Show when={renameRequest()}>
      {(req) => (
        <div class="dialog-backdrop" onMouseDown={close}>
          <div
            class="dialog"
            role="dialog"
            aria-modal="true"
            aria-labelledby="rename-title"
            onMouseDown={(e) => e.stopPropagation()}
            onKeyDown={onKey}
          >
            <div class="dialog-header">
              <div class="dialog-icon rename">
                <Icon d={ICONS.rename} />
              </div>
              <div class="dialog-heading">
                <h2 id="rename-title">
                  Rename <span class="dialog-name">“{baseName(req())}”</span>
                </h2>
              </div>
            </div>
            <input
              ref={input}
              class="dialog-input"
              type="text"
              aria-label="New name"
              aria-invalid={error() !== null}
              aria-describedby="rename-error"
              spellcheck={false}
              autocomplete="off"
              value={name()}
              onInput={(e) => {
                setName(e.currentTarget.value);
                setError(null);
              }}
            />
            <p id="rename-error" class="dialog-error" role="alert">
              {error()}
            </p>
            <div class="dialog-actions">
              <button id="rename-cancel" class="dialog-btn" onClick={close}>
                Cancel
              </button>
              <button id="rename-confirm" class="dialog-btn primary" disabled={busy()} onClick={submit}>
                Rename
              </button>
            </div>
          </div>
        </div>
      )}
    </Show>
  );
}
