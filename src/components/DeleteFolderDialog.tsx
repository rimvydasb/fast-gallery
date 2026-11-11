import { createEffect, createSignal, For, onCleanup, Show } from "solid-js";
import { api, baseName, FolderPreview } from "../lib/api";
import { folderToDelete, setFolderToDelete, sort, trashFolder } from "../lib/store";
import { imageCount, plural } from "../lib/format";
import { QueuedThumb } from "./Tile";

const MAX_THUMBS = 4;

/** Confirmation for moving a whole folder to the Trash: image count and a preview of up to 4 images. */
export function DeleteFolderDialog() {
  const [preview, setPreview] = createSignal<FolderPreview | null>(null);
  let confirmBtn: HTMLButtonElement | undefined;

  createEffect(() => {
    const folder = folderToDelete();
    setPreview(null);
    if (!folder) return;
    let stale = false;
    onCleanup(() => (stale = true));
    api
      .getFolderPreviews([folder], sort())
      .then((r) => {
        if (!stale) setPreview(r[0] ?? null);
      })
      .catch(() => {});
    queueMicrotask(() => confirmBtn?.focus());
  });

  const close = () => setFolderToDelete(null);
  const confirm = () => {
    const f = folderToDelete();
    if (f) trashFolder(f);
  };

  const onKey = (e: KeyboardEvent) => {
    if (e.key === "Escape") {
      e.preventDefault();
      close();
    } else if (e.key === "Enter" && document.activeElement?.id !== "dlg-cancel") {
      e.preventDefault();
      confirm();
    }
    e.stopPropagation();
  };

  return (
    <Show when={folderToDelete()}>
      {(folder) => {
        const shown = () => preview()?.images.slice(0, MAX_THUMBS) ?? [];
        const more = () => Math.max(0, (preview()?.count ?? 0) - shown().length);
        return (
          <div class="dialog-backdrop" onMouseDown={close}>
            <div
              class="dialog"
              role="alertdialog"
              aria-modal="true"
              aria-labelledby="dlg-title"
              onMouseDown={(e) => e.stopPropagation()}
              onKeyDown={onKey}
            >
              <h2 id="dlg-title">Move “{baseName(folder())}” to the Trash?</h2>
              <p class="dialog-sub">
                <Show when={preview()} fallback="Counting images…">
                  {(p) => <>{imageCount(p().count, p().more)} in this folder.</>}
                </Show>{" "}
                The whole folder and everything in it will be moved. Undo with ⌘Z.
              </p>
              <Show when={shown().length > 0}>
                <div class="dialog-previews">
                  <For each={shown()}>
                    {(img) => (
                      <div class="dialog-thumb">
                        <QueuedThumb path={img.p} mtime={img.m} size={256} priority={0} fit="cover" />
                      </div>
                    )}
                  </For>
                  <Show when={more() > 0}>
                    <div class="dialog-thumb dialog-more" title={`${plural(more(), "more image", "more images")}${preview()?.more ? " or more" : ""}`}>
                      +{more().toLocaleString()}{preview()?.more ? "+" : ""}
                    </div>
                  </Show>
                </div>
              </Show>
              <div class="dialog-actions">
                <button id="dlg-cancel" onClick={close}>
                  Cancel
                </button>
                <button class="danger" ref={confirmBtn} onClick={confirm}>
                  Move to Trash
                </button>
              </div>
            </div>
          </div>
        );
      }}
    </Show>
  );
}
