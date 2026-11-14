// Window state. Each window has its own JS context, so module state is per window.
import { batch, createMemo, createSignal } from "solid-js";
import { createStore, reconcile } from "solid-js/store";
import {
  api,
  baseName,
  CmdError,
  Entry,
  errorMessage,
  FolderPreview,
  isCmdError,
  joinPath,
  parentPath,
  Settings,
  SortSpec,
  sortKeyString,
} from "./api";
import { clearQueue } from "./thumbQueue";
import { toast } from "./toast";

export const MIN_TILE = 96;
export const MAX_TILE = 512;

export const [settings, setSettings] = createSignal<Settings | null>(null);
export const [path, setPath] = createSignal<string | null>(null);
export const [entries, setEntries] = createSignal<Entry[]>([]);
export const [sort, setSortSignal] = createSignal<SortSpec>({ key: "name", desc: false });
export const [tileSize, setTileSizeSignal] = createSignal(180);
export const [showHidden, setShowHidden] = createSignal(false);
export const [selection, setSelection] = createSignal<ReadonlySet<string>>(new Set());
export const [anchor, setAnchor] = createSignal(-1);
export const [cursor, setCursor] = createSignal(-1);
export const [editMode, setEditModeSignal] = createSignal(false);
export const [viewerIndex, setViewerIndex] = createSignal<number | null>(null);
export const [infoOpen, setInfoOpen] = createSignal(false);
export const [loadError, setLoadError] = createSignal<CmdError | null>(null);
export const [loading, setLoading] = createSignal(false);
/** Incremented on each navigation, so that the grid resets its scroll position. */
export const [navSeq, setNavSeq] = createSignal(0);

/** Folder previews by absolute folder path. */
export const [previews, setPreviews] = createStore<Record<string, FolderPreview>>({});

const back: string[] = [];
const forward: string[] = [];
export const [historyState, setHistoryState] = createSignal({ back: 0, forward: 0 });
const syncHistory = () => setHistoryState({ back: back.length, forward: forward.length });

export const folderCount = createMemo(() => {
  const e = entries();
  let n = 0;
  while (n < e.length && e[n].d) n++;
  return n;
});
export const images = createMemo(() => entries().slice(folderCount()));

export const entryPath = (e: Entry) => joinPath(path() ?? "/", e.n);

export function selectedEntries(): Entry[] {
  const sel = selection();
  if (sel.size === 0) return [];
  return entries().filter((e) => sel.has(e.n));
}

export function selectedPaths(): string[] {
  return selectedEntries().map(entryPath);
}

// ---------- Navigation ----------

let listSeq = 0;

interface NavOptions {
  push?: boolean;
  select?: string;
  openViewer?: boolean;
}

/** Removes a trailing slash, so that path comparisons work. */
export function normPath(p: string): string {
  return p.length > 1 ? p.replace(/\/+$/, "") || "/" : p;
}

export async function navigate(target: string, opts: NavOptions = {}) {
  target = normPath(target);
  const { push = true } = opts;
  const prev = path();
  const s = settings();
  const prefs = s?.folderPrefs[target];
  const nextSort = prefs?.sort ?? s?.defaultSort ?? sort();
  const nextTile = prefs?.tileSize ?? s?.defaultTileSize ?? tileSize();
  const seq = ++listSeq;
  setLoading(true);
  try {
    const listing = await api.listDir(target, nextSort, showHidden());
    if (seq !== listSeq) return;
    clearQueue();
    batch(() => {
      if (push && prev && prev !== target) {
        back.push(prev);
        forward.length = 0;
      }
      syncHistory();
      setPath(listing.path);
      setSortSignal(nextSort);
      setTileSizeSignal(nextTile);
      setEntries(listing.entries);
      setPreviews(reconcile({}));
      setLoadError(null);
      setViewerIndex(null);
      const idx = opts.select ? listing.entries.findIndex((e) => e.n === opts.select) : -1;
      setSelection(idx >= 0 ? new Set([listing.entries[idx].n]) : new Set<string>());
      setAnchor(idx);
      setCursor(idx);
      setNavSeq(navSeq() + 1);
      if (opts.openViewer && idx >= 0 && !listing.entries[idx].d) {
        setViewerIndex(idx - listing.entries.filter((e) => e.d).length);
      }
    });
    document.title = baseName(listing.path) || "Fast Gallery";
  } catch (e) {
    if (seq !== listSeq) return;
    batch(() => {
      if (push && prev && prev !== target) {
        back.push(prev);
        forward.length = 0;
      }
      syncHistory();
      setPath(target);
      setEntries([]);
      setSelection(new Set<string>());
      setLoadError(isCmdError(e) ? e : { kind: "other", message: errorMessage(e) });
      setNavSeq(navSeq() + 1);
    });
  } finally {
    if (seq === listSeq) setLoading(false);
  }
}

/** Re-lists the current folder; keeps the selection and the scroll position (FSW-05). */
export async function reload() {
  const p = path();
  if (!p) return;
  const seq = ++listSeq;
  try {
    const listing = await api.listDir(p, sort(), showHidden());
    if (seq !== listSeq || listing.path !== path()) return;
    const viewing = viewerIndex();
    const viewedName = viewing !== null ? images()[viewing]?.n : undefined;
    batch(() => {
      const names = new Set(listing.entries.map((e) => e.n));
      const sel = new Set([...selection()].filter((n) => names.has(n)));
      const cursorName = entries()[cursor()]?.n;
      setEntries(listing.entries);
      setSelection(sel);
      const ci = cursorName ? listing.entries.findIndex((e) => e.n === cursorName) : -1;
      setCursor(ci);
      setLoadError(null);
      if (viewing !== null) {
        const imgs = images();
        const vi = viewedName ? imgs.findIndex((e) => e.n === viewedName) : -1;
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
  const cur = path();
  if (cur) forward.push(cur);
  syncHistory();
  navigate(p, { push: false });
}

export function goForward() {
  const p = forward.pop();
  if (p === undefined) return;
  const cur = path();
  if (cur) back.push(cur);
  syncHistory();
  navigate(p, { push: false });
}

export function goParent() {
  const cur = path();
  if (!cur) return;
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
  if (e.d) navigate(entryPath(e));
  else setViewerIndex(index - folderCount());
}

// ---------- Sort, tile size, hidden files ----------

function savePrefs() {
  const p = path();
  if (p) api.setFolderPrefs(p, { sort: sort(), tileSize: tileSize() });
}

let prefsTimer: number | undefined;
function savePrefsSoon() {
  clearTimeout(prefsTimer);
  prefsTimer = window.setTimeout(savePrefs, 400);
}

export function setSort(s: SortSpec) {
  if (sortKeyString(s) === sortKeyString(sort())) return;
  setSortSignal(s);
  setPreviews(reconcile({}));
  savePrefs();
  reload();
}

export function setTileSize(n: number) {
  const v = Math.max(MIN_TILE, Math.min(MAX_TILE, Math.round(n)));
  setTileSizeSignal(v);
  savePrefsSoon();
}

export function toggleHidden() {
  setShowHidden(!showHidden());
  reload();
}

// ---------- Previews ----------

let previewReq = 0;

/** Requests previews for folders in the given order (visible first) — FPV-14, FPV-18. */
export async function requestPreviews(folders: string[]) {
  const s = sort();
  const req = ++previewReq;
  if (folders.length === 0) return;
  try {
    const cached = await api.getFolderPreviews(folders, s);
    if (req !== previewReq) return;
    batch(() => {
      for (const p of cached) if (!previews[p.folder]) setPreviews(p.folder, p);
    });
  } catch (e) {
    console.error(e);
  }
}

export function applyPreview(p: FolderPreview) {
  const cur = path();
  if (!cur || parentPath(p.folder) !== cur || p.sort !== sortKeyString(sort())) return;
  setPreviews(p.folder, reconcile(p));
}

// ---------- Selection ----------

export function selectOnly(index: number) {
  const e = entries()[index];
  if (!e) return;
  batch(() => {
    setSelection(new Set([e.n]));
    setAnchor(index);
    setCursor(index);
  });
}

export function toggleSelect(index: number) {
  const e = entries()[index];
  if (!e) return;
  const next = new Set(selection());
  if (next.has(e.n)) next.delete(e.n);
  else next.add(e.n);
  batch(() => {
    setSelection(next);
    setAnchor(index);
    setCursor(index);
  });
}

export function selectRange(index: number, additive = false) {
  const a = anchor() >= 0 ? anchor() : index;
  const [lo, hi] = a < index ? [a, index] : [index, a];
  const e = entries();
  const next = additive ? new Set(selection()) : new Set<string>();
  for (let i = lo; i <= hi; i++) next.add(e[i].n);
  batch(() => {
    setSelection(next);
    setCursor(index);
  });
}

export function selectAll() {
  setSelection(new Set(entries().map((e) => e.n)));
}

export function clearSelection() {
  setSelection(new Set<string>());
}

// ---------- Edit mode and file actions ----------

export async function setEditMode(on: boolean) {
  const v = await api.setEditMode(on);
  setEditModeSignal(v);
}

export const toggleEditMode = () => setEditMode(!editMode());

/** FILE-01: moves the given images to the Trash. Requires edit mode (EDIT-05, EDIT-06). */
export async function trashPaths(paths: string[]): Promise<string[]> {
  if (!editMode()) {
    toast("Turn on edit mode (⌘E) to move files to the Trash.", "info");
    return [];
  }
  if (paths.length === 0) return [];
  try {
    const r = await api.trashFiles(paths);
    for (const f of r.failed) toast(`Could not move “${baseName(f.path)}” to the Trash: ${f.error}`, "error");
    if (r.trashed.length > 0) {
      toast(
        r.trashed.length === 1
          ? `Moved “${baseName(r.trashed[0])}” to the Trash. ⌘Z to undo.`
          : `Moved ${r.trashed.length} items to the Trash. ⌘Z to undo.`,
        "info",
      );
    }
    return r.trashed;
  } catch (e) {
    toast(errorMessage(e), "error");
    return [];
  }
}

/** Trash from the grid; the selection moves to the next tile (FILE-07). */
export async function trashSelection() {
  const picked = selectedEntries();
  if (picked.some((e) => e.d)) {
    requestTrashItems(picked);
    return;
  }
  const sel = picked;
  if (sel.length === 0) return;
  const all = entries();
  const lastIdx = Math.max(...sel.map((e) => all.indexOf(e)));
  const trashedNames = new Set(sel.map((e) => e.n));
  let next: Entry | undefined;
  for (let i = lastIdx + 1; i < all.length && !next; i++) if (!trashedNames.has(all[i].n)) next = all[i];
  for (let i = lastIdx - 1; i >= 0 && !next; i--) if (!trashedNames.has(all[i].n)) next = all[i];
  const done = await trashPaths(sel.map(entryPath));
  if (done.length === 0) return;
  const gone = new Set(done.map(baseName));
  const remaining = entries().filter((e) => !gone.has(e.n));
  batch(() => {
    setEntries(remaining);
    const ni = next ? remaining.findIndex((e) => e.n === next!.n) : -1;
    if (ni >= 0) selectOnly(ni);
    else clearSelection();
  });
}

/** Trash from the viewer; the viewer shows the next image (FILE-06). */
export async function trashViewerImage() {
  const vi = viewerIndex();
  if (vi === null) return;
  const img = images()[vi];
  if (!img) return;
  const done = await trashPaths([entryPath(img)]);
  if (done.length === 0) return;
  const remaining = entries().filter((e) => e !== img);
  batch(() => {
    setEntries(remaining);
    const n = images().length;
    setViewerIndex(n === 0 ? null : Math.min(vi, n - 1));
  });
}

export async function undoTrash() {
  if (!editMode()) {
    toast("Turn on edit mode (⌘E) to undo a move to the Trash.", "info");
    return;
  }
  try {
    const restored = await api.undoTrash();
    if (restored.length) {
      toast(restored.length === 1 ? `Restored “${baseName(restored[0])}”.` : `Restored ${restored.length} items.`, "info");
      await reload();
      const names = new Set(restored.filter((r) => parentPath(r) === path()).map(baseName));
      if (names.size) setSelection(names);
    }
  } catch (e) {
    toast(errorMessage(e), "error");
  }
}

export async function copySelection(paths = selectedPaths()) {
  if (paths.length === 0) return;
  await api.copyFiles(paths);
  toast(paths.length === 1 ? `Copied “${baseName(paths[0])}”.` : `Copied ${paths.length} items.`, "info");
}

export async function revealSelection(paths = selectedPaths()) {
  const p = paths.length ? paths : path() ? [path()!] : [];
  if (p.length) await api.revealInFinder(p);
}

export async function toggleFavorite(p: string) {
  const s = await api.toggleFavorite(p);
  setSettings(s);
  toast(s.favorites.includes(p) ? `Added “${baseName(p)}” to Favorites.` : `Removed “${baseName(p)}” from Favorites.`, "info");
}

// ---------- Delete folders / mixed selections (edit mode) ----------

export interface DeleteRequest {
  folders: string[];
  files: string[];
}

/** The items that wait for a delete confirmation, or null. */
export const [deleteRequest, setDeleteRequest] = createSignal<DeleteRequest | null>(null);

/** Asks for a confirmation before items that include folders move to the Trash. Requires edit mode. */
export function requestTrashItems(items: Entry[]) {
  if (!editMode()) {
    toast("Turn on edit mode (⌘E) to move items to the Trash.", "info");
    return;
  }
  if (items.length === 0) return;
  setDeleteRequest({
    folders: items.filter((e) => e.d).map(entryPath),
    files: items.filter((e) => !e.d).map(entryPath),
  });
}

/** Moves the folders and files to the Trash (undo with ⌘Z) and refreshes the listing. */
export async function trashItems(req: DeleteRequest) {
  setDeleteRequest(null);
  const done = await trashPaths([...req.folders, ...req.files]);
  if (done.length === 0) return;
  const gone = new Set(done.map(baseName));
  const remaining = entries().filter((e) => !gone.has(e.n));
  batch(() => {
    setEntries(remaining);
    clearSelection();
  });
}
