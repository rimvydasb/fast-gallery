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
  DirListing,
  GroupInfo,
  GroupKey,
  AnalysisNeed,
  isIgnoredPath,
  joinPath,
  parentPath,
  Settings,
  SortSpec,
  sortKeyString,
} from "./api";
import { caps, location, path, reload } from "./navigation";
import { locationKey } from "./locations";
import { validateFileName } from "./rename";
import { toast } from "./toast";

export const MIN_TILE = 96;
export const MAX_TILE = 512;

export const [settings, setSettings] = createSignal<Settings | null>(null);
export const [entries, setEntries] = createSignal<Entry[]>([]);
export const [sort, setSortSignal] = createSignal<SortSpec>({ key: "name", desc: false });
/** The grouping of the grid (per location, like the sort). */
export const [group, setGroupSignal] = createSignal<GroupKey>("none");
/** The groups of the current listing: consecutive ranges of `images()`. Empty without a grouping. */
export const [groups, setGroups] = createSignal<GroupInfo[]>([]);
/** Images of the current listing that wait for an analysis, and which analysis. */
export const [pendingAnalysis, setPendingAnalysis] = createSignal<{ missing: number; need: AnalysisNeed } | null>(null);
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
/** Incremented when the folder previews must be requested again, for example a new ignore list. */
export const [previewEpoch, setPreviewEpoch] = createSignal(0);
/** The home folder of the user, for "~/" in the ignored folders. */
export const [homeDir, setHomeDir] = createSignal("");

/** True if `p` is in the tree of an ignored folder (D6). */
export const isIgnored = (p: string) => isIgnoredPath(p, settings()?.ignoredFolders ?? [], homeDir());

/** Folder previews by absolute folder path. */
export const [previews, setPreviews] = createStore<Record<string, FolderPreview>>({});

export const folderCount = createMemo(() => {
  const e = entries();
  let n = 0;
  while (n < e.length && e[n].d) n++;
  return n;
});
export const images = createMemo(() => entries().slice(folderCount()));

/** Shows a listing: the entries, their groups, and the analysis that the listing still needs. */
export function applyListing(l: DirListing) {
  batch(() => {
    setEntries(l.entries);
    setGroups(l.groups ?? []);
    setPendingAnalysis(l.need && l.missing ? { missing: l.missing, need: l.need } : null);
  });
}

/** The absolute path of an entry: `Entry.p` in a collection, else the current folder and the name. */
export const entryPath = (e: Entry) => e.p ?? joinPath(path() ?? "/", e.n);

/** The selection key of an entry: unique in a listing (names can repeat in a collection). */
export const entryKey = (e: Entry) => e.p ?? e.n;

export function selectedEntries(): Entry[] {
  const sel = selection();
  if (sel.size === 0) return [];
  return entries().filter((e) => sel.has(entryKey(e)));
}

export function selectedPaths(): string[] {
  return selectedEntries().map(entryPath);
}

// ---------- Sort, tile size, hidden files ----------

function savePrefs() {
  const l = location();
  if (l && caps().folderPrefs) api.setFolderPrefs(locationKey(l), { sort: sort(), tileSize: tileSize(), group: group() });
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

export function setGroup(g: GroupKey) {
  if (g === group()) return;
  setGroupSignal(g);
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
    setSelection(new Set([entryKey(e)]));
    setAnchor(index);
    setCursor(index);
  });
}

export function toggleSelect(index: number) {
  const e = entries()[index];
  if (!e) return;
  const k = entryKey(e);
  const next = new Set(selection());
  if (next.has(k)) next.delete(k);
  else next.add(k);
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
  for (let i = lo; i <= hi; i++) next.add(entryKey(e[i]));
  batch(() => {
    setSelection(next);
    setCursor(index);
  });
}

/** Selects the entries from index `lo` to `hi`, for example the images of 1 group. */
export function selectSpan(lo: number, hi: number) {
  const e = entries();
  const next = new Set<string>();
  for (let i = Math.max(0, lo); i <= hi && i < e.length; i++) next.add(entryKey(e[i]));
  batch(() => {
    setSelection(next);
    setAnchor(lo);
    setCursor(lo);
  });
}

export function selectAll() {
  setSelection(new Set(entries().map(entryKey)));
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

/** The paths that ⌘Z restores: the last trashed or renamed items (for the task lock check). */
export const [undoTargets, setUndoTargets] = createSignal<string[]>([]);

/** Removes trashed paths from the listing. */
function dropPaths(done: string[]): Entry[] {
  const gone = new Set(done);
  return entries().filter((e) => !gone.has(entryPath(e)));
}

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
    if (r.trashed.length > 0) setUndoTargets(r.trashed);
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
  const trashed = new Set(sel);
  let next: Entry | undefined;
  for (let i = lastIdx + 1; i < all.length && !next; i++) if (!trashed.has(all[i])) next = all[i];
  for (let i = lastIdx - 1; i >= 0 && !next; i--) if (!trashed.has(all[i])) next = all[i];
  const done = await trashPaths(sel.map(entryPath));
  if (done.length === 0) return;
  const remaining = dropPaths(done);
  batch(() => {
    setEntries(remaining);
    const ni = next ? remaining.findIndex((e) => entryKey(e) === entryKey(next!)) : -1;
    if (ni >= 0) selectOnly(ni);
    else clearSelection();
  });
}

/** Trash of one tile from its trash button; the selection moves to the next tile. */
export async function trashEntry(entry: Entry) {
  const done = await trashPaths([entryPath(entry)]);
  if (done.length === 0) return;
  const all = entries();
  const i = all.indexOf(entry);
  const remaining = all.filter((x) => x !== entry);
  setEntries(remaining);
  const next = remaining[Math.min(i, remaining.length - 1)];
  if (next) setSelection(new Set([entryKey(next)]));
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
      setUndoTargets([]);
      await reload();
      const back = new Set(restored);
      const keys = entries().filter((e) => back.has(entryPath(e))).map(entryKey);
      if (keys.length) setSelection(new Set(keys));
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
  const cur = path();
  const p = paths.length ? paths : cur ? [cur] : [];
  if (p.length) await api.revealInFinder(p);
}

export async function toggleFavorite(p: string) {
  const s = await api.toggleFavorite(p);
  setSettings(s);
  toast(s.favorites.includes(p) ? `Added “${baseName(p)}” to Favorites.` : `Removed “${baseName(p)}” from Favorites.`, "info");
}

/** IGN-08: adds a folder to the ignored folders. */
export async function ignoreFolder(p: string) {
  const s = settings();
  if (!s) return;
  // No local `setSettings`: `settings-changed` must see the change, so that each window lists again (IGN-06).
  await api.updateSettings({ ignoredFolders: [...s.ignoredFolders, p] });
  toast(`“${baseName(p)}” is ignored. Settings shows the list.`, "info", { label: "Open Settings", actionId: "view.openIgnoredSettings" });
}

/** Removes one folder from Recent folders; the other entries keep their order. */
export async function removeRecent(p: string) {
  const s = settings();
  if (!s) return;
  setSettings(await api.updateSettings({ recents: s.recents.filter((r) => r !== p) }));
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
  const remaining = dropPaths(done);
  batch(() => {
    setEntries(remaining);
    clearSelection();
  });
}

// ---------- Rename (edit mode) ----------

/** The file path that waits for a new name in the rename dialog, or null. */
export const [renameRequest, setRenameRequest] = createSignal<string | null>(null);

/** Opens the rename dialog for one file. Requires edit mode (EDIT-06). */
export function requestRename(item: Entry) {
  if (!editMode()) {
    toast("Turn on edit mode (⌘E) to rename files.", "info");
    return;
  }
  if (item.d) return;
  setRenameRequest(entryPath(item));
}

/** FILE-14: renames the file. Returns an error message for the dialog, or null on success. */
export async function renameFile(from: string, newName: string): Promise<string | null> {
  if (!editMode()) return "Turn on edit mode (⌘E) to rename files.";
  const name = newName.trim();
  const invalid = validateFileName(name);
  if (invalid) return invalid;
  if (name === baseName(from)) return "The name did not change.";
  try {
    const to = await api.renameFile(from, name);
    setRenameRequest(null);
    setUndoTargets([from]);
    toast(`Renamed “${baseName(from)}” to “${baseName(to)}”. ⌘Z to undo.`, "info");
    await reload();
    const i = entries().findIndex((e) => entryPath(e) === to);
    if (i >= 0) selectOnly(i);
    return null;
  } catch (e) {
    return errorMessage(e);
  }
}
