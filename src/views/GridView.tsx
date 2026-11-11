import { createEffect, createMemo, createSignal, For, on, onCleanup, onMount, Show, untrack } from "solid-js";
import { api, Entry } from "../lib/api";
import {
  cursor,
  editMode,
  entries,
  entryPath,
  loadError,
  navSeq,
  openEntry,
  path,
  previews,
  requestPreviews,
  selectOnly,
  selectRange,
  selection,
  selectedEntries,
  setSelection,
  sort,
  tileSize,
  toggleSelect,
  trashPaths,
  clearSelection,
  setEntries,
} from "../lib/store";
import { noteFastScroll, setVisibleRange } from "../lib/thumbQueue";
import { FolderTile, ImageTile } from "../components/Tile";
import { openContextMenu } from "../components/ContextMenu";
import { imageCount } from "../lib/format";

export const GAP = 14;
export const PAD = 18;
export const LABEL = 40;

/** Number of columns of the grid, for keyboard navigation (GRID-14). */
export const [gridCols, setGridCols] = createSignal(1);

let scrollToIndexImpl: (i: number) => void = () => {};
/** Scrolls the grid so that the tile at index i is visible. */
export const scrollToIndex = (i: number) => scrollToIndexImpl(i);

export function GridView() {
  let scroller!: HTMLDivElement;
  const [width, setWidth] = createSignal(800);
  const [height, setHeight] = createSignal(600);
  const [scrollTop, setScrollTop] = createSignal(0);

  const rowH = () => tileSize() + LABEL + GAP;
  const cols = createMemo(() => Math.max(1, Math.floor((width() - 2 * PAD + GAP) / (tileSize() + GAP))));
  const rows = () => Math.ceil(entries().length / cols());
  const offsetX = () => Math.max(PAD, (width() - (cols() * tileSize() + (cols() - 1) * GAP)) / 2);
  createEffect(() => setGridCols(cols()));

  // GRID-04: render only the rows in the viewport and a buffer of 2 rows.
  const range = createMemo(() => {
    const rh = rowH();
    const first = Math.floor(Math.max(0, scrollTop() - PAD) / rh);
    const last = Math.floor(Math.max(0, scrollTop() + height() - PAD) / rh);
    return { first, last };
  });
  const startIdx = createMemo(() => Math.max(0, range().first - 2) * cols());
  const endIdx = createMemo(() => Math.min(entries().length, (range().last + 3) * cols()));
  const visible = createMemo(() => entries().slice(startIdx(), endIdx()));
  createEffect(() => setVisibleRange(range().first * cols(), (range().last + 1) * cols() - 1));

  scrollToIndexImpl = (i: number) => {
    if (i < 0) return;
    const row = Math.floor(i / cols());
    const top = PAD + row * rowH();
    const bottom = top + rowH() - GAP;
    if (top < scroller.scrollTop) scroller.scrollTop = top - PAD;
    else if (bottom > scroller.scrollTop + scroller.clientHeight) scroller.scrollTop = bottom - scroller.clientHeight + PAD;
  };

  onMount(() => {
    const ro = new ResizeObserver(() => {
      setWidth(scroller.clientWidth);
      setHeight(scroller.clientHeight);
    });
    ro.observe(scroller);
    onCleanup(() => ro.disconnect());
  });

  let raf = 0;
  let lastTop = 0;
  let lastT = 0;
  const FAST_SCROLL = 2; // px per ms (2,000 px/s)
  const onScroll = () => {
    const now = performance.now();
    const top = scroller.scrollTop;
    if (now > lastT && Math.abs(top - lastTop) / (now - lastT) > FAST_SCROLL) noteFastScroll();
    lastTop = top;
    lastT = now;
    if (raf) return;
    raf = requestAnimationFrame(() => {
      raf = 0;
      setScrollTop(scroller.scrollTop);
    });
  };

  // New folder: reset the scroll position, or show the selected tile.
  createEffect(
    on(navSeq, () => {
      const c = untrack(cursor);
      scroller.scrollTop = 0;
      setScrollTop(0);
      if (c >= 0) queueMicrotask(() => scrollToIndex(c));
    }),
  );

  // FPV-14: request folder previews, visible folder tiles first.
  createEffect(
    on([navSeq, sort], () => {
      const p = untrack(path);
      if (!p) return;
      const e = untrack(entries);
      const folders: number[] = [];
      for (let i = 0; i < e.length && e[i].d; i++) folders.push(i);
      const { first, last } = untrack(range);
      const c = untrack(cols);
      const lo = first * c;
      const hi = (last + 1) * c;
      const visibleFirst = [...folders.filter((i) => i >= lo && i < hi), ...folders.filter((i) => i < lo || i >= hi)];
      requestPreviews(visibleFirst.map((i) => entryPath(e[i])));
    }),
  );

  // ----- Mouse: selection (GRID-13), open (GRID-15, NAV-07), drag out (FILE-10) -----
  let down: { x: number; y: number; index: number; dragged: boolean; deferSelect: boolean } | null = null;

  const onTileMouseDown = (e: MouseEvent, index: number) => {
    if (e.button !== 0) return;
    e.preventDefault();
    scroller.focus({ preventScroll: true });
    const entry = entries()[index];
    let deferSelect = false;
    if (e.metaKey) toggleSelect(index);
    else if (e.shiftKey) selectRange(index);
    else if (!selection().has(entry.n)) selectOnly(index);
    else deferSelect = true;
    down = { x: e.clientX, y: e.clientY, index, dragged: false, deferSelect };
  };

  const onMouseMove = (e: MouseEvent) => {
    if (!down || down.dragged || (e.buttons & 1) === 0) return;
    if (Math.hypot(e.clientX - down.x, e.clientY - down.y) < 6) return;
    down.dragged = true;
    const paths = selectedEntries()
      .filter((x) => !x.d)
      .map(entryPath);
    if (paths.length) api.startDrag(paths).catch(() => {});
  };

  const onMouseUp = () => {
    if (down && !down.dragged && down.deferSelect) selectOnly(down.index);
    down = null;
  };

  const trashOne = async (entry: Entry) => {
    const done = await trashPaths([entryPath(entry)]);
    if (done.length) {
      const all = entries();
      const i = all.indexOf(entry);
      const remaining = all.filter((x) => x !== entry);
      setEntries(remaining);
      const next = remaining[Math.min(i, remaining.length - 1)];
      if (next) setSelection(new Set([next.n]));
    }
  };

  return (
    <div
      class="grid-scroll"
      ref={scroller}
      tabIndex={0}
      role="listbox"
      aria-multiselectable="true"
      aria-label={`Contents of ${path() ?? ""}`}
      aria-activedescendant={cursor() >= 0 ? `tile-${cursor()}` : undefined}
      onScroll={onScroll}
      onMouseMove={onMouseMove}
      onMouseUp={onMouseUp}
      onMouseDown={(e) => {
        if (e.target === scroller || (e.target as HTMLElement).classList.contains("grid-canvas")) {
          scroller.focus({ preventScroll: true });
          clearSelection();
        }
      }}
      onContextMenu={(e) => {
        e.preventDefault();
        if (e.target === scroller || (e.target as HTMLElement).classList.contains("grid-canvas")) {
          clearSelection();
          openContextMenu(e.clientX, e.clientY);
        }
      }}
    >
      <Show when={!loadError()}>
        <div class="grid-canvas" style={{ height: `${rows() * rowH() + PAD * 2 - GAP}px` }}>
          <For each={visible()}>
            {(entry, i) => {
              const idx = () => startIdx() + i();
              const x = () => offsetX() + (idx() % cols()) * (tileSize() + GAP);
              const y = () => PAD + Math.floor(idx() / cols()) * rowH();
              const p = entryPath(entry);
              const selected = () => selection().has(entry.n);
              return (
                <div
                  id={`tile-${idx()}`}
                  class="tile"
                  classList={{ folder: entry.d, selected: selected(), cursor: cursor() === idx() }}
                  role="option"
                  aria-selected={selected()}
                  aria-label={
                    entry.d
                      ? `Folder ${entry.n}${previews[p] ? `, ${imageCount(previews[p].count, previews[p].more)}` : ""}`
                      : `Image ${entry.n}`
                  }
                  style={{
                    transform: `translate(${x()}px, ${y()}px)`,
                    width: `${tileSize()}px`,
                    height: `${tileSize() + LABEL}px`,
                  }}
                  onMouseDown={(e) => onTileMouseDown(e, idx())}
                  onDblClick={() => openEntry(idx())}
                  onContextMenu={(e) => {
                    e.preventDefault();
                    e.stopPropagation();
                    if (!selected()) selectOnly(idx());
                    openContextMenu(e.clientX, e.clientY);
                  }}
                >
                  {entry.d ? (
                    <FolderTile
                      entry={entry}
                      path={p}
                      index={idx()}
                      selected={selected()}
                      focused={cursor() === idx()}
                      tileSize={tileSize()}
                      editMode={editMode()}
                      preview={previews[p]}
                    />
                  ) : (
                    <ImageTile
                      entry={entry}
                      path={p}
                      index={idx()}
                      selected={selected()}
                      focused={cursor() === idx()}
                      tileSize={tileSize()}
                      editMode={editMode()}
                      onTrash={() => trashOne(entry)}
                    />
                  )}
                </div>
              );
            }}
          </For>
        </div>
        <Show when={entries().length === 0 && path()}>
          <div class="empty-state">This folder has no images or folders.</div>
        </Show>
      </Show>
      <Show when={loadError()}>
        {(err) => (
          <div class="error-state" role="alert">
            <svg viewBox="0 0 24 24" aria-hidden="true" class="error-icon">
              <path
                d="M12 2a5 5 0 0 0-5 5v3H6a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-8a2 2 0 0 0-2-2h-1V7a5 5 0 0 0-5-5zm-3 8V7a3 3 0 1 1 6 0v3z"
                fill="currentColor"
              />
            </svg>
            <p>{err().message}</p>
            <Show when={err().kind === "permission"}>
              <p class="hint">
                macOS protects this folder. To open it, give Fast Gallery Full Disk Access in System Settings,
                then open the folder again.
              </p>
              <button class="btn primary" onClick={() => api.openPrivacySettings()}>
                Open Privacy Settings
              </button>
            </Show>
          </div>
        )}
      </Show>
    </div>
  );
}
