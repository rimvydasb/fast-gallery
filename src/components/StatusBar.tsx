import { Show } from "solid-js";
import { plural } from "../lib/format";
import { folderCount, images, loading, selection, setTileSize, tileSize, MAX_TILE, MIN_TILE } from "../lib/store";

/** GRID-17: number of folders, images, and the selection count. GRID-06: tile size slider. */
export function StatusBar() {
  return (
    <footer class="statusbar">
      <span class="status-text" aria-live="polite">
        {plural(folderCount(), "folder", "folders")}, {plural(images().length, "image", "images")}
        <Show when={selection().size > 0}> — {selection().size.toLocaleString()} selected</Show>
        <Show when={loading()}>
          <span class="spinner" aria-label="Loading" />
        </Show>
      </span>
      <label class="size-slider" title="Tile size (⌘+ / ⌘−)">
        <span class="visually-hidden">Tile size</span>
        <svg viewBox="0 0 16 16" class="icon-small" aria-hidden="true">
          <rect x="4" y="4" width="8" height="8" rx="1.5" fill="currentColor" />
        </svg>
        <input
          type="range"
          min={MIN_TILE}
          max={MAX_TILE}
          step={4}
          value={tileSize()}
          onInput={(e) => setTileSize(Number(e.currentTarget.value))}
        />
        <svg viewBox="0 0 16 16" class="icon-small" aria-hidden="true">
          <rect x="1" y="1" width="14" height="14" rx="2" fill="currentColor" />
        </svg>
      </label>
    </footer>
  );
}
