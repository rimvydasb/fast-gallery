import { createEffect, Show } from "solid-js";
import { runAction } from "../lib/actions";
import { scanCapInfo, setScanCapInfo } from "../lib/badPhotos";
import { plural } from "../lib/format";
import { Icon } from "./Toolbar";

const SCAN_ICON = "M3 7V4.5A1.5 1.5 0 0 1 4.5 3H7M13 3h2.5A1.5 1.5 0 0 1 17 4.5V7M17 13v2.5a1.5 1.5 0 0 1-1.5 1.5H13M7 17H4.5A1.5 1.5 0 0 1 3 15.5V13M4 10h12";

/** QUAL-13: tells the user that the scan stopped at the scan cap. A modal in the window, not alert(). */
export function ScanCapDialog() {
  let primary: HTMLButtonElement | undefined;

  createEffect(() => {
    if (scanCapInfo()) queueMicrotask(() => primary?.focus());
  });

  const close = () => setScanCapInfo(null);
  const open = () => {
    const i = scanCapInfo();
    close();
    if (i) runAction("nav.openCollection", i.collectionId);
  };

  const onKey = (e: KeyboardEvent) => {
    if (e.key === "Escape") {
      e.preventDefault();
      close();
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (scanCapInfo()?.showOpen) open();
      else close();
    }
    e.stopPropagation();
  };

  return (
    <Show when={scanCapInfo()}>
      {(info) => (
        <div class="dialog-backdrop" onMouseDown={close}>
          <div
            class="dialog"
            role="alertdialog"
            aria-modal="true"
            aria-labelledby="scancap-title"
            aria-describedby="scancap-text"
            onMouseDown={(e) => e.stopPropagation()}
            onKeyDown={onKey}
          >
            <div class="dialog-header">
              <div class="dialog-icon scan">
                <Icon d={SCAN_ICON} />
              </div>
              <div class="dialog-heading">
                <h2 id="scancap-title">Scan Stopped at {info().cap.toLocaleString()} Photos</h2>
              </div>
            </div>
            <p id="scancap-text" class="dialog-sub">
              The folder <span class="dialog-name">“{info().rootName}”</span> has more photos. Found{" "}
              {plural(info().found, "bad photo", "bad photos")} in the first {info().cap.toLocaleString()} photos. Scan a
              subfolder, or raise the scan limit in Settings.
            </p>
            <div class="dialog-actions">
              <Show
                when={info().showOpen}
                fallback={
                  <button class="dialog-btn primary" ref={primary} onClick={close}>
                    OK
                  </button>
                }
              >
                <button class="dialog-btn" onClick={close}>
                  OK
                </button>
                <button class="dialog-btn primary" ref={primary} onClick={open}>
                  Open Bad Photos
                </button>
              </Show>
            </div>
          </div>
        </div>
      )}
    </Show>
  );
}
