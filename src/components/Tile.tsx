import { createSignal, For, onCleanup, Show } from "solid-js";
import { Entry, FolderPreview, parentPath, thumbUrl } from "../lib/api";
import { badgeInfo, badgeLabels } from "../lib/badges";
import { TaskIndicator } from "./TaskIndicator";
import { requestThumb } from "../lib/thumbQueue";
import { imageCount } from "../lib/format";
import { flatFolder } from "../lib/locations";
import { location } from "../lib/navigation";

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
        <Show when={props.entry.b?.length}>
          {/* QUAL-23: the badges of a collection item, for example "Blurry, Underexposed". */}
          <div class="tile-badges" title={badgeLabels(props.entry.b)}>
            <For each={props.entry.b}>
              {(code) => <span class={`tile-badge ${badgeInfo(code).tone}`}>{badgeInfo(code).label}</span>}
            </For>
          </div>
        </Show>
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
      {/* In a collection, the tooltip shows the folder of the file; in a flat location, the folder
          relative to the flattened root (FLAT-11). */}
      <div
        class="tile-label"
        title={props.entry.p ? (flatFolder(location(), props.entry) ?? parentPath(props.entry.p) ?? props.entry.p) : props.entry.n}
      >
        {props.entry.n}
      </div>
    </>
  );
}

function FolderGlyph(props: { class?: string }) {
  return (
    <svg class={props.class} viewBox="0 0 16 16" aria-hidden="true">
      <path d="M1.5 3.5A1 1 0 0 1 2.5 2.5h3.6l1.4 1.5h6a1 1 0 0 1 1 1v7.5a1 1 0 0 1-1 1h-11a1 1 0 0 1-1-1z" fill="currentColor" />
    </svg>
  );
}

/**
 * FPV-01, FPV-11, FPV-12, FPV-13: a box with a thin bezel. The previews fill the box as a mosaic
 * (1 to 4 cells), and the image count is a pill on the box.
 */
export function FolderTile(props: TileProps) {
  const images = () => props.preview?.images ?? [];
  return (
    <>
      <div class="tile-media">
        <div class="mfolder" data-count={images().length}>
          <Show when={images().length > 0} fallback={<FolderGlyph class="mfolder-empty" />}>
            <For each={images()}>
              {(img) => (
                <div class="mfolder-cell">
                  <QueuedThumb path={img.p} mtime={img.m} size={thumbSizeFor(props.tileSize)} priority={props.index} fit="cover" />
                </div>
              )}
            </For>
          </Show>
          <Show when={props.preview && props.preview.count > 0 ? props.preview : null}>
            {(p) => (
              <span class="mfolder-count" title={imageCount(p().count, p().more)}>
                {p().count.toLocaleString()}
                {p().more ? "+" : ""}
              </span>
            )}
          </Show>
        </div>
        <TaskIndicator variant="tile" root={props.path} />
      </div>
      <div class="tile-label" title={props.entry.n}>
        <FolderGlyph class="mfolder-icon" />
        <span class="tile-name">{props.entry.n}</span>
      </div>
    </>
  );
}
