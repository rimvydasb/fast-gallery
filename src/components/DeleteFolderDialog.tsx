import { createEffect, createSignal, For, onCleanup, Show } from "solid-js";
import { api, baseName, FolderPreview } from "../lib/api";
import { deleteRequest, setDeleteRequest, sort, trashItems } from "../lib/store";
import { imageCount, plural } from "../lib/format";
import { QueuedThumb } from "./Tile";
import { Icon } from "./Toolbar";

const TRASH_ICON = "M4 6h12M8 6V4.5h4V6M5.5 6l.8 10h7.4l.8-10M8.5 9v4.5M11.5 9v4.5";

const MAX_THUMBS = 4;

/** Confirmation for moving a whole folder to the Trash: image count and a preview of up to 4 images. */
export function DeleteFolderDialog() {
  const [preview, setPreview] = createSignal<FolderPreview | null>(null);
  let confirmBtn: HTMLButtonElement | undefined;

  createEffect(() => {
    const req = deleteRequest();
    setPreview(null);
    // A preview only makes sense for one folder on its own.
    if (!req || req.folders.length !== 1 || req.files.length > 0) return;
    const folder = req.folders[0];
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

  const close = () => setDeleteRequest(null);
  const confirm = () => {
    const r = deleteRequest();
    if (r) trashItems(r);
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
    <Show when={deleteRequest()}>
      {(req) => {
        const single = () => req().folders.length === 1 && req().files.length === 0;
        const total = () => req().folders.length + req().files.length;
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
              <div class="dialog-header">
                <div class="dialog-icon">
                  <Icon d={TRASH_ICON} />
                </div>
                <div class="dialog-heading">
                  <h2 id="dlg-title">
                    <Show
                      when={single()}
                      fallback={
                        <>
                          Move <span class="dialog-name">{plural(total(), "item", "items")}</span> to the Trash?
                        </>
                      }
                    >
                      Move <span class="dialog-name">“{baseName(req().folders[0])}”</span> to the Trash?
                    </Show>
                  </h2>
                  <p class="dialog-count">
                    <Show
                      when={single()}
                      fallback={[
                        req().folders.length > 0 ? plural(req().folders.length, "folder", "folders") : "",
                        req().files.length > 0 ? plural(req().files.length, "image", "images") : "",
                      ]
                        .filter(Boolean)
                        .join(" and ")}
                    >
                      <Show when={preview()} fallback="Counting images…">
                        {(p) => <>{imageCount(p().count, p().more)} in this folder</>}
                      </Show>
                    </Show>
                  </p>
                </div>
              </div>
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
              <p class="dialog-sub">
                {single() ? "The folder and everything in it" : req().folders.length > 0 ? "The items and everything in the folders" : "The items"} will be moved to the Trash. You can undo this with{" "}
                <kbd>⌘Z</kbd>.
              </p>
              <div class="dialog-actions">
                <button id="dlg-cancel" class="dialog-btn" onClick={close}>
                  Cancel
                </button>
                <button class="dialog-btn danger" ref={confirmBtn} onClick={confirm}>
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
