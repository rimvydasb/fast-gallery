import { createSignal, For, onCleanup, Show } from "solid-js";
import { Entry, FolderPreview, thumbUrl } from "../lib/api";
import { requestThumb } from "../lib/thumbQueue";
import { plural } from "../lib/format";

/** THM-02: 512 px thumbnails for tiles larger than 256 px or on Retina displays. */
export function thumbSizeFor(tile: number): 256 | 512 {
  return tile > 256 || window.devicePixelRatio > 1 ? 512 : 256;
}

/**
 * An <img> whose request goes through the thumbnail queue (GRID-09, PERF-12).
 * Shows nothing until the thumbnail is ready (the tile keeps its final size, GRID-08) and a
 * placeholder if the decode fails (FMT-07).
 */
export function QueuedThumb(props: { path: string; mtime: number; size: 256 | 512; priority: number; class?: string; fit?: "contain" | "cover" }) {
  const [src, setSrc] = createSignal<string>();
  const [state, setState] = createSignal<"wait" | "ok" | "error">("wait");
  const url = thumbUrl(props.path, props.size, props.mtime);
  const ticket = requestThumb(props.priority, () => setSrc(url));
  onCleanup(() => ticket.cancel());
  return (
    <Show when={state() !== "error"} fallback={<div class="thumb-error" aria-hidden="true">?</div>}>
      <img
        class={props.class}
        classList={{ loaded: state() === "ok", [`fit-${props.fit ?? "contain"}`]: true }}
        src={src()}
        alt=""
        decoding="async"
        draggable={false}
        onLoad={() => {
          setState("ok");
          ticket.done();
        }}
        onError={() => {
          if (!src()) return;
          setState("error");
          ticket.done();
        }}
      />
    </Show>
  );
}

interface TileProps {
  entry: Entry;
  path: string;
  index: number;
  selected: boolean;
  focused: boolean;
  tileSize: number;
  editMode: boolean;
  preview?: FolderPreview;
  onTrash?: () => void;
}

export function ImageTile(props: TileProps) {
  return (
    <>
      <div class="tile-media">
        <QueuedThumb path={props.path} mtime={props.entry.m} size={thumbSizeFor(props.tileSize)} priority={props.index} />
        <Show when={props.editMode}>
          <button
            class="tile-trash"
            title="Move to Trash (⌘⌫)"
            aria-label={`Move ${props.entry.n} to Trash`}
            tabIndex={-1}
            onMouseDown={(e) => e.stopPropagation()}
            onClick={(e) => {
              e.stopPropagation();
              props.onTrash?.();
            }}
          >
            <svg viewBox="0 0 16 16" aria-hidden="true">
              <path d="M5.5 2h5l.5 1H14v1.5H2V3h3zM3.5 5.5h9l-.7 8.5H4.2z" fill="currentColor" />
            </svg>
          </button>
        </Show>
      </div>
      <div class="tile-label" title={props.entry.n}>
        {props.entry.n}
      </div>
    </>
  );
}

/** FPV-01, FPV-11, FPV-12, FPV-13: up to 4 previews in a 2×2 layout on a folder shape. */
export function FolderTile(props: TileProps) {
  const size = () => (props.tileSize > 256 && window.devicePixelRatio > 1 ? 512 : 256) as 256 | 512;
  return (
    <>
      <div class="tile-media">
        <div class="folder-shape">
          <div class="folder-tab" />
          <div class="folder-body">
            <div class="folder-grid" data-count={props.preview?.images.length ?? 0}>
              <For each={props.preview?.images ?? []}>
                {(img) => (
                  <div class="folder-cell">
                    <QueuedThumb path={img.p} mtime={img.m} size={size()} priority={props.index} fit="cover" />
                  </div>
                )}
              </For>
            </div>
          </div>
        </div>
      </div>
      <div class="tile-label" title={props.entry.n}>
        <span class="tile-name">{props.entry.n}</span>
        <Show when={props.preview}>
          {(p) => <span class="tile-count">{plural(p().count, "image", "images")}</span>}
        </Show>
      </div>
    </>
  );
}
