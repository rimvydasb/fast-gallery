// Action registry (D4): each user action is defined once here. Toolbar buttons, context menu items,
// keyboard shortcuts, sidebar and breadcrumb clicks, and native menu events call `runAction`; a
// control reads `isEnabled` and `blockedReason`. The task rule is generic: it uses only the action
// category and, for file changes, the target paths.
import { api, Entry, errorMessage, GroupKey, SortSpec } from "./api";
import { collectionName, collections, Location } from "./locations";
import {
  canGoParent,
  caps,
  goBack,
  goForward,
  goParent,
  historyState,
  location,
  navigate,
  openEntry,
  openFolderDialog,
  openPath,
} from "./navigation";
import {
  copySelection,
  DeleteRequest,
  editMode,
  entryPath,
  images,
  infoOpen,
  pendingAnalysis,
  requestRename,
  requestTrashItems,
  revealSelection,
  selectAll,
  selectedPaths,
  setInfoOpen,
  setGroup,
  setSort,
  setTileSize,
  tileSize,
  toggleEditMode,
  toggleFavorite,
  toggleHidden,
  trashEntry,
  trashItems,
  trashSelection,
  trashViewerImage,
  undoTargets,
  undoTrash,
  viewerIndex,
} from "./store";
import { busyReason, lockedBy, lockedReason, task } from "./tasks";
import { toast } from "./toast";
import { toggleSidebar } from "./ui";
import { startBadPhotoScan } from "./badPhotos";
import { startAnalysis } from "./analysis";

export type ActionCategory =
  | "navigation" // changes the location of a window, or opens a window
  | "fileChange" // changes files or folders on disk
  | "dataChange" // changes app-wide data, for example clear cache
  | "task" // starts or cancels a task
  | "view"; // changes only the view: selection, viewer, zoom, sort, info panel

export interface Action<A = void> {
  id: string;
  /** Menu and tooltip text. */
  label: (args: A) => string;
  /** Display text, for example "⌘[". */
  shortcut?: string;
  category: ActionCategory;
  /** Paths the action changes; required for "fileChange". */
  targets?: (args: A) => string[];
  /** Feature rule, for example edit mode or LocationCaps. */
  enabled: (args: A) => boolean;
  /** Why `enabled` is false, for a toast on a shortcut (for example "Turn on edit mode"). */
  disabledReason?: (args: A) => string | null;
  run: (args: A) => unknown;
}

/** The argument type of each action. */
export interface ActionArgs {
  "nav.back": void;
  "nav.forward": void;
  "nav.parent": void;
  "nav.open": Location | string;
  "nav.openPath": string;
  "nav.openEntry": number;
  "nav.openFolderDialog": void;
  "nav.newWindow": void;
  "nav.openCollection": string;
  "view.toggleInfo": void;
  "view.toggleSidebar": void;
  "view.toggleHidden": void;
  "view.toggleEditMode": void;
  "view.selectAll": void;
  "view.copy": string[] | undefined;
  "view.reveal": string[] | undefined;
  "view.toggleFavorite": string | null;
  "view.sort": SortSpec;
  "view.group": GroupKey;
  "view.tileSize": number;
  "file.trashSelection": void;
  "file.trashViewerImage": void;
  "file.trashEntry": Entry;
  "file.requestTrashItems": Entry[];
  "file.trashItems": DeleteRequest;
  "file.rename": Entry;
  "file.undo": void;
  "data.clearCache": void;
  "data.removeCollection": string;
  "task.scanBadPhotos": string | null;
  "task.analyzePhotos": void;
  "task.cancel": void;
}

export type ActionId = keyof ActionArgs;

const EDIT_TRASH = "Turn on edit mode (⌘E) to move files to the Trash.";
const EDIT_UNDO = "Turn on edit mode (⌘E) to undo a move to the Trash.";
const EDIT_RENAME = "Turn on edit mode (⌘E) to rename files.";

const always = () => true;

function viewerImagePath(): string | null {
  const vi = viewerIndex();
  const img = vi !== null ? images()[vi] : undefined;
  return img ? entryPath(img) : null;
}

/** Leaves a location that no longer exists. */
async function leaveRemovedCollection(id: string) {
  const l = location();
  if (l?.kind !== "collection" || l.id !== id) return;
  if (historyState().back > 0) goBack();
  else navigate(await api.homeDir());
}

const DEFS: { [K in ActionId]: Omit<Action<ActionArgs[K]>, "id"> } = {
  // ----- navigation -----
  "nav.back": {
    label: () => "Back",
    shortcut: "⌘[",
    category: "navigation",
    enabled: () => historyState().back > 0,
    run: goBack,
  },
  "nav.forward": {
    label: () => "Forward",
    shortcut: "⌘]",
    category: "navigation",
    enabled: () => historyState().forward > 0,
    run: goForward,
  },
  "nav.parent": {
    label: () => "Enclosing Folder",
    shortcut: "⌘↑",
    category: "navigation",
    enabled: canGoParent,
    run: goParent,
  },
  "nav.open": {
    label: () => "Open",
    category: "navigation",
    enabled: (to: Location | string) => !!to,
    run: (to: Location | string) => navigate(to),
  },
  "nav.openPath": {
    label: () => "Open",
    category: "navigation",
    enabled: (p: string) => !!p,
    run: (p: string) => openPath(p),
  },
  "nav.openEntry": {
    label: () => "Open",
    category: "navigation",
    enabled: (i: number) => i >= 0,
    run: (i: number) => openEntry(i),
  },
  "nav.openFolderDialog": {
    label: () => "Open Folder…",
    shortcut: "⌘O",
    category: "navigation",
    enabled: always,
    run: openFolderDialog,
  },
  "nav.newWindow": {
    label: () => "New Window",
    shortcut: "⌘N",
    category: "navigation",
    enabled: always,
    run: () => api.newWindow(),
  },
  "nav.openCollection": {
    label: (id: string) => {
      const c = collections().find((x) => x.id === id);
      return c ? `${c.name} (${c.count.toLocaleString()})` : collectionName(id);
    },
    category: "navigation",
    enabled: (id: string) => collections().some((c) => c.id === id),
    run: (id: string) => navigate({ kind: "collection", id }),
  },

  // ----- view -----
  "view.toggleInfo": {
    label: () => (infoOpen() ? "Hide Info" : "Show Info"),
    shortcut: "⌘I",
    category: "view",
    enabled: always,
    run: () => setInfoOpen(!infoOpen()),
  },
  "view.toggleSidebar": {
    label: () => "Toggle Sidebar",
    shortcut: "⌘⌥S",
    category: "view",
    enabled: always,
    run: toggleSidebar,
  },
  "view.toggleHidden": {
    label: () => "Show Hidden Files",
    shortcut: "⌘⇧.",
    category: "view",
    enabled: () => caps().folderTiles,
    run: toggleHidden,
  },
  // Edit mode on and off stays enabled while a task runs; the fileChange rule protects the tree.
  "view.toggleEditMode": {
    label: () => (editMode() ? "Turn Off Edit Mode" : "Turn On Edit Mode"),
    shortcut: "⌘E",
    category: "view",
    enabled: always,
    run: toggleEditMode,
  },
  "view.selectAll": {
    label: () => "Select All",
    shortcut: "⌘A",
    category: "view",
    enabled: always,
    run: selectAll,
  },
  "view.copy": {
    label: (paths: string[] | undefined) => {
      const n = (paths ?? selectedPaths()).length;
      return n === 1 ? "Copy" : `Copy ${n} Items`;
    },
    shortcut: "⌘C",
    category: "view",
    enabled: (paths: string[] | undefined) => (paths ?? selectedPaths()).length > 0,
    run: (paths: string[] | undefined) => copySelection(paths),
  },
  "view.reveal": {
    label: () => "Reveal in Finder",
    shortcut: "⌘⇧R",
    category: "view",
    enabled: always,
    run: (paths: string[] | undefined) => revealSelection(paths),
  },
  "view.toggleFavorite": {
    label: () => "Add to Favorites",
    shortcut: "⌘D",
    category: "view",
    enabled: (p: string | null) => !!p,
    run: (p: string | null) => toggleFavorite(p!),
  },
  "view.sort": {
    label: () => "Sort",
    category: "view",
    enabled: always,
    run: (s: SortSpec) => setSort(s),
  },
  "view.group": {
    label: () => "Group",
    category: "view",
    enabled: always,
    run: (g: GroupKey) => setGroup(g),
  },
  "view.tileSize": {
    label: () => "Tile Size",
    category: "view",
    enabled: always,
    run: (delta: number) => setTileSize(tileSize() + delta),
  },

  // ----- fileChange (EDIT-06: only in edit mode) -----
  "file.trashSelection": {
    label: () => "Move to Trash",
    shortcut: "⌘⌫",
    category: "fileChange",
    targets: () => selectedPaths(),
    enabled: () => editMode() && selectedPaths().length > 0,
    disabledReason: () => (editMode() ? null : EDIT_TRASH),
    run: trashSelection,
  },
  "file.trashViewerImage": {
    label: () => "Move to Trash",
    shortcut: "⌘⌫",
    category: "fileChange",
    targets: () => {
      const p = viewerImagePath();
      return p ? [p] : [];
    },
    enabled: () => editMode() && viewerImagePath() !== null,
    disabledReason: () => (editMode() ? null : EDIT_TRASH),
    run: trashViewerImage,
  },
  "file.trashEntry": {
    label: () => "Move to Trash",
    category: "fileChange",
    targets: (e: Entry) => [entryPath(e)],
    enabled: () => editMode(),
    disabledReason: () => EDIT_TRASH,
    run: (e: Entry) => trashEntry(e),
  },
  "file.requestTrashItems": {
    label: (items: Entry[]) => (items.length === 1 ? "Move Folder to Trash…" : `Move ${items.length} Items to Trash…`),
    shortcut: "⌘⌫",
    category: "fileChange",
    targets: (items: Entry[]) => items.map(entryPath),
    enabled: (items: Entry[]) => editMode() && items.length > 0,
    disabledReason: () => (editMode() ? null : EDIT_TRASH),
    run: (items: Entry[]) => requestTrashItems(items),
  },
  "file.trashItems": {
    label: () => "Move to Trash",
    category: "fileChange",
    targets: (r: DeleteRequest) => [...r.folders, ...r.files],
    enabled: () => editMode(),
    disabledReason: () => EDIT_TRASH,
    run: (r: DeleteRequest) => trashItems(r),
  },
  "file.rename": {
    label: () => "Rename…",
    category: "fileChange",
    targets: (e: Entry) => [entryPath(e)],
    enabled: (e: Entry) => editMode() && !e.d,
    disabledReason: () => (editMode() ? null : EDIT_RENAME),
    run: (e: Entry) => requestRename(e),
  },
  "file.undo": {
    label: () => "Undo",
    shortcut: "⌘Z",
    category: "fileChange",
    targets: () => undoTargets(),
    enabled: () => editMode(),
    disabledReason: () => EDIT_UNDO,
    run: undoTrash,
  },

  // ----- dataChange -----
  "data.clearCache": {
    label: () => "Clear Cache",
    category: "dataChange",
    enabled: always,
    run: () => api.clearCache(),
  },
  "data.removeCollection": {
    label: () => "Remove Collection",
    category: "dataChange",
    enabled: (id: string) => !!id,
    run: async (id: string) => {
      try {
        await api.removeCollection(id);
        await leaveRemovedCollection(id);
      } catch (e) {
        toast(errorMessage(e), "error");
      }
    },
  },

  // ----- task -----
  "task.scanBadPhotos": {
    label: () => "Scan for Bad Photos",
    category: "task",
    enabled: (root: string | null) => !!root,
    run: (root: string | null) => startBadPhotoScan(root!),
  },
  "task.analyzePhotos": {
    label: () => "Analyze Photos",
    category: "task",
    enabled: () => pendingAnalysis() !== null,
    run: () => startAnalysis(true),
  },
  "task.cancel": {
    label: () => "Cancel Scan",
    category: "task",
    enabled: () => task() !== null,
    run: () => api.cancelTask(),
  },
};

// The registry stores each action with its id; the argument type is checked at the call sites.
type AnyAction = Action<unknown>;
const REGISTRY = new Map<string, AnyAction>(
  Object.entries(DEFS).map(([id, d]) => [id, { id, ...(d as unknown as Omit<AnyAction, "id">) }]),
);

export function getAction(id: ActionId): AnyAction {
  const a = REGISTRY.get(id);
  if (!a) throw new Error(`Unknown action ${id}`);
  return a;
}

/** The task rule (table in D4): why a running task blocks the action, or null. */
function taskBlock(a: AnyAction, args: unknown): string | null {
  const t = task();
  if (!t) return null;
  switch (a.category) {
    case "navigation":
    case "view":
      return null;
    case "fileChange": {
      const lock = lockedBy(a.targets?.(args) ?? []);
      return lock ? lockedReason(lock) : null;
    }
    case "dataChange":
      return busyReason(t);
    case "task":
      return a.id === "task.cancel" ? null : busyReason(t);
  }
}

type ArgList<K extends ActionId> = [ActionArgs[K]] extends [void]
  ? [args?: undefined]
  : [undefined] extends [ActionArgs[K]]
    ? [args?: ActionArgs[K]]
    : [args: ActionArgs[K]];

/** The task rule AND the feature rule of the action. */
export function isEnabled<K extends ActionId>(id: K, ...[args]: ArgList<K>): boolean {
  const a = getAction(id);
  return taskBlock(a, args) === null && a.enabled(args);
}

/** Text for a tooltip or a toast: why the action is not available now, or null. */
export function blockedReason<K extends ActionId>(id: K, ...[args]: ArgList<K>): string | null {
  const a = getAction(id);
  return taskBlock(a, args) ?? (a.enabled(args) ? null : (a.disabledReason?.(args) ?? null));
}

export function actionLabel<K extends ActionId>(id: K, ...[args]: ArgList<K>): string {
  return getAction(id).label(args);
}

/** Runs the action if it is enabled. Else shows the reason as a toast, so that a blocked shortcut is not silent. */
export function runAction<K extends ActionId>(id: K, ...[args]: ArgList<K>): unknown {
  const a = getAction(id);
  const reason = blockedReason(id, ...([args] as ArgList<K>));
  if (reason !== null || !a.enabled(args)) {
    if (reason) toast(reason, "info");
    return;
  }
  return a.run(args);
}

/** Runs an action by an id from data (for example a toast button). */
export function runActionById(id: string, args?: unknown): unknown {
  if (!REGISTRY.has(id)) return;
  return (runAction as (id: string, args?: unknown) => unknown)(id, args);
}

/** Label, shortcut, enabled state, and reason of an action, for a menu item or a button. */
export function actionState(id: ActionId, args?: unknown) {
  const a = getAction(id);
  const reason = taskBlock(a, args) ?? (a.enabled(args) ? null : (a.disabledReason?.(args) ?? null));
  return { label: a.label(args), shortcut: a.shortcut, enabled: reason === null && a.enabled(args), reason };
}
