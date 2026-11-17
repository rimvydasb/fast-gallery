import { createEffect, createMemo, createSignal, For, on, onCleanup, onMount, Show, untrack } from "solid-js";
import { api, GroupInfo } from "../lib/api";
import {
  cursor,
  editMode,
  entries,
  entryKey,
  entryPath,
  folderCount,
  groups,
  loadError,
  navSeq,
  previews,
  requestPreviews,
  selectOnly,
  selectRange,
  selectSpan,
  selection,
  selectedEntries,
  sort,
  tileSize,
  toggleSelect,
  clearSelection,
} from "../lib/store";
import { location, path } from "../lib/navigation";
import { providerFor } from "../lib/locations";
import { runAction } from "../lib/actions";
import { noteFastScroll, setVisibleRange } from "../lib/thumbQueue";
import { FolderTile, ImageTile } from "../components/Tile";
import { openContextMenu } from "../components/ContextMenu";
import { groupLabel, imageCount } from "../lib/format";
import { task, taskText } from "../lib/tasks";

/** Gap between tiles, padding around the grid, and label height. */
const GAP = 4;
const PAD = 6;
const LABEL = 22;
/** Height of a group header: the label and the horizontal line. */
export const HEADER_H = 38;

/**
 * A run of consecutive entries that the grid lays out as rows: all entries (no grouping), the
 * folders, or 1 group with a header. Each section starts a new row.
 */
interface Section {
  start: number;
  count: number;
  rows: number;
  /** y of the header, or of the first row if there is no header. */
  top: number;
  rowsTop: number;
  header: GroupInfo | null;
}

/** Number of columns of the grid, for keyboard navigation (GRID-14). */
export const [gridCols, setGridCols] = createSignal(1);

let gridMoveImpl: (i: number, dir: -1 | 1) => number = (i, dir) => i + dir;
/** The index 1 row above (dir = -1) or below the tile at index i; rows do not cross a group as index ± columns. */
export const gridMove = (i: number, dir: -1 | 1) => gridMoveImpl(i, dir);

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
  const offsetX = () => Math.max(PAD, (width() - (cols() * tileSize() + (cols() - 1) * GAP)) / 2);
  createEffect(() => setGridCols(cols()));

  const layout = createMemo(() => {
    const n = entries().length;
    const c = cols();
    const rh = rowH();
    const gs = groups();
    const fc = folderCount();
    const secs: Section[] = [];
    let y = PAD;
    const push = (start: number, count: number, header: GroupInfo | null) => {
      if (count <= 0) return;
      const rows = Math.ceil(count / c);
      const rowsTop = y + (header ? HEADER_H : 0);
      secs.push({ start, count, rows, top: y, rowsTop, header });
      y = rowsTop + rows * rh;
    };
    // The groups cover the images exactly; if not (a listing in transition), show no groups.
    const grouped = gs.length > 0 && gs.reduce((a, g) => a + g.count, 0) === n - fc;
    if (!grouped) push(0, n, null);
    else {
      push(0, fc, null);
      let start = fc;
      for (const g of gs) {
        push(start, g.count, g);
        start += g.count;
      }
    }
    return { secs, height: y + PAD - GAP };
  });

  /** The section of the entry at index i. */
  const sectionAt = (i: number): Section | undefined => {
    const secs = layout().secs;
    let lo = 0;
    let hi = secs.length - 1;
    while (lo < hi) {
      const mid = (lo + hi + 1) >> 1;
      if (secs[mid].start <= i) lo = mid;
      else hi = mid - 1;
    }
    return secs[lo];
  };

  const pos = (i: number) => {
    const s = sectionAt(i);
    if (!s) return { x: 0, y: 0 };
    const local = i - s.start;
    const c = cols();
    return { x: offsetX() + (local % c) * (tileSize() + GAP), y: s.rowsTop + Math.floor(local / c) * rowH() };
  };

  /** The entries of the rows between y = t0 and y = t1: [first index, end index). */
  const span = (t0: number, t1: number): [number, number] => {
    const c = cols();
    const rh = rowH();
    let first = -1;
    let end = 0;
    for (const s of layout().secs) {
      if (s.rowsTop + s.rows * rh <= t0) continue;
      if (s.top >= t1) break;
      const r0 = Math.max(0, Math.floor((t0 - s.rowsTop) / rh));
      const r1 = Math.min(s.rows - 1, Math.floor((t1 - s.rowsTop) / rh));
      if (r1 < r0) continue;
      if (first < 0) first = s.start + r0 * c;
      end = Math.min(s.start + s.count, s.start + (r1 + 1) * c);
    }
    return first < 0 ? [0, 0] : [first, end];
  };

  // GRID-04: render only the rows in the viewport and a buffer of 2 rows.
  const inView = createMemo(() => span(scrollTop(), scrollTop() + height()), undefined, {
    equals: (a, b) => a[0] === b[0] && a[1] === b[1],
  });
  const buffered = createMemo(() => span(scrollTop() - 2 * rowH(), scrollTop() + height() + 2 * rowH()), undefined, {
    equals: (a, b) => a[0] === b[0] && a[1] === b[1],
  });
  const startIdx = () => buffered()[0];
  const visible = createMemo(() => entries().slice(buffered()[0], buffered()[1]));
  const visibleHeaders = createMemo(() => {
    const t0 = scrollTop() - 2 * rowH();
    const t1 = scrollTop() + height() + 2 * rowH();
    return layout().secs.filter((s) => s.header && s.top + HEADER_H > t0 && s.top < t1);
  });
  createEffect(() => setVisibleRange(inView()[0], inView()[1] - 1));

  scrollToIndexImpl = (i: number) => {
    if (i < 0) return;
    const s = sectionAt(i);
    if (!s) return;
    const row = Math.floor((i - s.start) / cols());
    // The first row of a group scrolls its header into view too.
    const top = row === 0 ? s.top : s.rowsTop + row * rowH();
    const bottom = s.rowsTop + (row + 1) * rowH() - GAP;
    if (top < scroller.scrollTop) scroller.scrollTop = top - PAD;
    else if (bottom > scroller.scrollTop + scroller.clientHeight) scroller.scrollTop = bottom - scroller.clientHeight + PAD;
  };

  gridMoveImpl = (i: number, dir: -1 | 1) => {
    const secs = layout().secs;
    const s = sectionAt(i);
    if (i < 0 || !s) return i + dir;
    const c = cols();
    const col = (i - s.start) % c;
    const row = Math.floor((i - s.start) / c);
    const k = secs.indexOf(s);
    if (dir > 0) {
      if (row + 1 < s.rows) return Math.min(s.start + (row + 1) * c + col, s.start + s.count - 1);
      const next = secs[k + 1];
      return next ? Math.min(next.start + col, next.start + next.count - 1) : i;
    }
    if (row > 0) return s.start + (row - 1) * c + col;
    const prev = secs[k - 1];
    return prev ? Math.min(prev.start + (prev.rows - 1) * c + col, prev.start + prev.count - 1) : i;
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
      const [lo, hi] = untrack(inView);
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
    else if (!selection().has(entryKey(entry))) selectOnly(index);
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

  const emptyText = () => {
    const l = location();
    return l ? providerFor(l).emptyText(l) : "";
  };

  return (
    <div
      class="grid-scroll"
      ref={scroller}
      tabIndex={0}
      role="listbox"
      aria-multiselectable="true"
      aria-label={`Contents of ${path() ?? document.title}`}
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
        <div class="grid-canvas" style={{ height: `${layout().height}px` }}>
          <For each={visibleHeaders()}>
            {(s) => (
              <div
                class="group-header"
                style={{
                  transform: `translate(${offsetX()}px, ${s.top}px)`,
                  width: `${cols() * tileSize() + (cols() - 1) * GAP}px`,
                  height: `${HEADER_H - 6}px`,
                }}
              >
                <button
                  class="group-title"
                  title="Select the photos of this group"
                  tabIndex={-1}
                  onClick={() => {
                    scroller.focus({ preventScroll: true });
                    selectSpan(s.start, s.start + s.count - 1);
                  }}
                >
                  {groupLabel(s.header!)}
                </button>
                <span class="group-count">{s.count.toLocaleString()}</span>
                <span class="group-line" />
                <Show when={s.header!.kind === "missing"}>
                  <Show
                    when={task()?.kind === "analyzePhotos" ? task() : null}
                    fallback={
                      <button class="group-action" tabIndex={-1} onClick={() => runAction("task.analyzePhotos")}>
                        Analyze
                      </button>
                    }
                  >
                    {(t) => <span class="group-note">{taskText(t())}</span>}
                  </Show>
                </Show>
              </div>
            )}
          </For>
          <For each={visible()}>
            {(entry, i) => {
              const idx = () => startIdx() + i();
              const at = createMemo(() => pos(idx()));
              const p = entryPath(entry);
              const selected = () => selection().has(entryKey(entry));
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
                    transform: `translate(${at().x}px, ${at().y}px)`,
                    width: `${tileSize()}px`,
                    height: `${tileSize() + LABEL}px`,
                  }}
                  onMouseDown={(e) => onTileMouseDown(e, idx())}
                  onDblClick={() => runAction("nav.openEntry", idx())}
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
                      onTrash={() => runAction("file.trashEntry", entry)}
                    />
                  )}
                </div>
              );
            }}
          </For>
        </div>
        <Show when={entries().length === 0 && location()}>
          <div class="empty-state">{emptyText()}</div>
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
