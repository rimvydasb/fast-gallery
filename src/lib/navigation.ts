// Navigation of one window: the current Location, back and forward history, and navigate (D5).
// This module contains no kind-specific code; the providers in locations.ts do.
import { batch, createSignal } from "solid-js";
import { api, baseName, errorMessage, isCmdError, parentPath } from "./api";
import { Location, locationKey, NO_CAPS, providerFor, sameLocation } from "./locations";
import {
  applyListing,
  cursor,
  entries,
  group,
  setGroupSignal,
  entryKey,
  folderCount,
  images,
  selection,
  setAnchor,
  setCursor,
  setLoadError,
  setLoading,
  setNavSeq,
  setPreviews,
  setSelection,
  setSortSignal,
  setTileSizeSignal,
  setViewerIndex,
  settings,
  showHidden,
  sort,
  tileSize,
  navSeq,
  viewerIndex,
} from "./store";
import { clearQueue } from "./thumbQueue";
import { toast } from "./toast";
import { reconcile } from "solid-js/store";

export const [location, setLocation] = createSignal<Location | null>(null);

/** The disk folder of the window, or null in a virtual folder. */
export const path = (): string | null => {
  const l = location();
  return l?.kind === "disk" ? l.path : null;
};

/** Sets a disk location (for tests and start-up). */
export const setPath = (p: string | null) => setLocation(p === null ? null : { kind: "disk", path: p });

export const caps = () => {
  const l = location();
  return l ? providerFor(l).caps(l) : NO_CAPS;
};

const back: Location[] = [];
const forward: Location[] = [];
export const [historyState, setHistoryState] = createSignal({ back: 0, forward: 0 });
const syncHistory = () => setHistoryState({ back: back.length, forward: forward.length });

let listSeq = 0;

export interface NavOptions {
  push?: boolean;
  /** Entry key (file name on disk) to select. */
  select?: string;
  openViewer?: boolean;
}

/** Removes a trailing slash, so that path comparisons work. */
export function normPath(p: string): string {
  return p.length > 1 ? p.replace(/\/+$/, "") || "/" : p;
}

function toLocation(t: Location | string): Location {
  if (typeof t === "string") return { kind: "disk", path: normPath(t) };
  return t.kind === "disk" ? { kind: "disk", path: normPath(t.path) } : t;
}

function pushHistory(prev: Location | null, target: Location, push: boolean) {
  if (push && prev && !sameLocation(prev, target)) {
    back.push(prev);
    forward.length = 0;
  }
  syncHistory();
}

export async function navigate(t: Location | string, opts: NavOptions = {}) {
  const target = toLocation(t);
  const { push = true } = opts;
  const prev = location();
  const provider = providerFor(target);
  const c = provider.caps(target);
  const s = settings();
  const prefs = c.folderPrefs ? s?.folderPrefs[locationKey(target)] : undefined;
  const nextSort = prefs?.sort ?? s?.defaultSort ?? sort();
  const nextTile = prefs?.tileSize ?? s?.defaultTileSize ?? tileSize();
  // The grouping is per location; a location without stored prefs has none.
  const nextGroup = prefs?.group ?? "none";
  const seq = ++listSeq;
  setLoading(true);
  try {
    const listing = await provider.list(target, nextSort, showHidden(), nextGroup);
    if (seq !== listSeq) return;
    clearQueue();
    const loc: Location = target.kind === "disk" ? { kind: "disk", path: listing.path } : target;
    batch(() => {
      pushHistory(prev, loc, push);
      setLocation(loc);
      setSortSignal(nextSort);
      setTileSizeSignal(nextTile);
      setGroupSignal(nextGroup);
      applyListing(listing);
      setPreviews(reconcile({}));
      setLoadError(null);
      setViewerIndex(null);
      const idx = opts.select ? listing.entries.findIndex((e) => entryKey(e) === opts.select) : -1;
      setSelection(idx >= 0 ? new Set([entryKey(listing.entries[idx])]) : new Set<string>());
      setAnchor(idx);
      setCursor(idx);
      setNavSeq(navSeq() + 1);
      if (opts.openViewer && idx >= 0 && !listing.entries[idx].d) {
        setViewerIndex(idx - listing.entries.filter((e) => e.d).length);
      }
    });
    document.title = provider.title(loc);
  } catch (e) {
    if (seq !== listSeq) return;
    batch(() => {
      pushHistory(prev, target, push);
      setLocation(target);
      applyListing({ path: "", entries: [] });
      setSelection(new Set<string>());
      setLoadError(isCmdError(e) ? e : { kind: "other", message: errorMessage(e) });
      setNavSeq(navSeq() + 1);
    });
  } finally {
    if (seq === listSeq) setLoading(false);
  }
}

/** Re-lists the current location; keeps the selection and the scroll position (FSW-05). */
export async function reload() {
  const loc = location();
  if (!loc) return;
  const seq = ++listSeq;
  try {
    const listing = await providerFor(loc).list(loc, sort(), showHidden(), group());
    if (seq !== listSeq || !sameLocation(location(), loc)) return;
    const viewing = viewerIndex();
    const viewedKey = viewing !== null ? images()[viewing] && entryKey(images()[viewing]) : undefined;
    batch(() => {
      const keys = new Set(listing.entries.map(entryKey));
      const sel = new Set([...selection()].filter((k) => keys.has(k)));
      const cur = entries()[cursor()];
      const cursorKey = cur ? entryKey(cur) : undefined;
      applyListing(listing);
      setSelection(sel);
      setCursor(cursorKey ? listing.entries.findIndex((e) => entryKey(e) === cursorKey) : -1);
      setLoadError(null);
      if (viewing !== null) {
        const imgs = images();
        const vi = viewedKey ? imgs.findIndex((e) => entryKey(e) === viewedKey) : -1;
        if (imgs.length === 0) setViewerIndex(null);
        else if (vi >= 0) setViewerIndex(vi);
        else setViewerIndex(Math.min(viewing, imgs.length - 1));
      }
    });
  } catch (e) {
    if (seq === listSeq) setLoadError(isCmdError(e) ? e : { kind: "other", message: errorMessage(e) });
  }
}

export function goBack() {
  const p = back.pop();
  if (p === undefined) return;
  const cur = location();
  if (cur) forward.push(cur);
  syncHistory();
  navigate(p, { push: false });
}

export function goForward() {
  const p = forward.pop();
  if (p === undefined) return;
  const cur = location();
  if (cur) back.push(cur);
  syncHistory();
  navigate(p, { push: false });
}

/** True if Enclosing Folder is possible. */
export function canGoParent(): boolean {
  const p = path();
  return caps().parent && !!p && p !== "/";
}

export function goParent() {
  const cur = path();
  if (!cur || !caps().parent) return;
  const parent = parentPath(cur);
  if (parent) navigate(parent, { select: baseName(cur) });
}

export async function openFolderDialog() {
  const p = await api.pickFolder();
  if (p) navigate(p);
}

/** Opens a path from drag and drop or "Open With": a folder, or the folder of an image. */
export async function openPath(p: string) {
  const kind = await api.pathKind(p);
  if (kind === "dir") navigate(p);
  else if (kind === "file") {
    const parent = parentPath(p);
    if (parent) navigate(parent, { select: baseName(p), openViewer: true });
  } else toast(`“${baseName(p)}” does not exist.`, "error");
}

/** Opens the entry at a grid index: a folder in the grid, an image in the viewer. */
export function openEntry(index: number) {
  const e = entries()[index];
  if (!e) return;
  if (e.d) navigate(e.p ?? joinCurrent(e.n));
  else setViewerIndex(index - folderCount());
}

function joinCurrent(name: string): string {
  const p = path() ?? "/";
  return p.endsWith("/") ? p + name : `${p}/${name}`;
}

/** Clears the history (tests). */
export function resetHistory() {
  back.length = 0;
  forward.length = 0;
  syncHistory();
}
