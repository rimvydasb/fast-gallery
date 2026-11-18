import { createEffect, createSignal, on, onCleanup, onMount, Show } from "solid-js";
import { listen, UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { api, CollectionSummary, FlatStatus, FolderPreview, FsChange, parentPath, Settings } from "./lib/api";
import {
  applyPreview,
  editMode,
  entries,
  entryPath,
  images,
  infoOpen,
  loadError,
  pendingAnalysis,
  previewEpoch,
  selectedEntries,
  setEditMode,
  setHomeDir,
  setPreviewEpoch,
  setPreviews,
  setSettings,
  setShowHidden,
  settings,
  sort,
  viewerIndex,
  loading,
} from "./lib/store";
import { location, navigate, path, reload } from "./lib/navigation";
import { parseLocation, setCollections } from "./lib/locations";
import { applyFlatProgress, flat, leaveFlatten, onFlatTreeChanged, setFlat } from "./lib/flatten";
import { reconcile } from "solid-js/store";
import { runAction } from "./lib/actions";
import { initTasks, task } from "./lib/tasks";
import { onAnalysisDone, startAnalysis } from "./lib/analysis";
import { onBadPhotoScanDone } from "./lib/badPhotos";
import { onKeyDown } from "./lib/keyboard";
import { sidebarCollapsed } from "./lib/ui";
import { Toolbar } from "./components/Toolbar";
import { Sidebar } from "./components/Sidebar";
import { StatusBar } from "./components/StatusBar";
import { InfoPanel } from "./components/InfoPanel";
import { ContextMenu } from "./components/ContextMenu";
import { DeleteFolderDialog } from "./components/DeleteFolderDialog";
import { RenameDialog } from "./components/RenameDialog";
import { ScanCapDialog } from "./components/ScanCapDialog";
import { Toasts } from "./components/Toasts";
import { FlatBanner, GridView } from "./views/GridView";

/** True if the window shows the flat location of `root`. */
const onFlat = (root: string | undefined) => {
  const l = location();
  return !!root && l?.kind === "flat" && l.path === root;
};

/** D4: lists the flat location again after `flat-progress`, never 2 listings at once, and never
 * during a navigation (a reload would cancel it). A missed refresh runs after the navigation. */
let flatListing = false;
let flatPending = false;
async function refreshFlat() {
  if (loading() || flatListing) {
    flatPending = true;
    return;
  }
  flatPending = false;
  if (!onFlat(flat()?.root)) return;
  flatListing = true;
  try {
    await reload();
  } finally {
    flatListing = false;
  }
  if (flatPending) refreshFlat();
}

/** IGN-06: a new ignore list. The window lists again, and the folder previews resolve again. */
async function onIgnoreListChanged() {
  setPreviews(reconcile({}));
  // A flat location walks again with the new list (a refresh keeps its listing until it ends).
  if (location()?.kind === "flat") setFlat(null);
  await reload();
  setPreviewEpoch(previewEpoch() + 1);
}
import { ViewerView } from "./views/ViewerView";

export function App() {
  const [dropping, setDropping] = createSignal(false);
  const unlisten: UnlistenFn[] = [];

  onMount(async () => {
    window.addEventListener("keydown", onKeyDown);
    // PERF-02: show the window as soon as the shell is rendered; the folder fills in after.
    const shown = getCurrentWindow().show();
    const [s, home, pending, cols] = await Promise.all([
      api.getSettings(),
      api.homeDir(),
      api.takePendingOpen(),
      api.listCollections(),
    ]);
    setSettings(s);
    setHomeDir(home);
    setShowHidden(s.showHidden);
    setCollections(cols);

    // NAV-11: restore the last open location. PLAT-06: or open the file macOS asked us to open.
    const q = new URLSearchParams(window.location.search).get("folder");
    const start = pending
      ? runAction("nav.openPath", pending)
      : navigate(q ?? parseLocation(s.lastFolder ?? home), { push: false });
    const label = getCurrentWindow().label;

    const listeners = await Promise.all([
      setEditMode(false).then(() => () => {}), // EDIT-02
      listen<FolderPreview>("folder-preview-ready", (e) => applyPreview(e.payload)),
      listen<Settings>("settings-changed", (e) => {
        const before = settings()?.ignoredFolders ?? [];
        setSettings(e.payload);
        const after = e.payload.ignoredFolders;
        if (before.length !== after.length || before.some((p, i) => p !== after[i])) onIgnoreListChanged();
      }),
      // D4: the walk of the flat location found more photos, or ended.
      listen<FlatStatus>("flat-progress", (e) => {
        if (applyFlatProgress(e.payload)) refreshFlat();
      }),
      listen("menu-open-folder", () => runAction("nav.openFolderDialog")),
      listen<string>("open-path", (e) => runAction("nav.openPath", e.payload)),
      // The busy state of all windows; the source window opens the scan result (QUAL-12).
      initTasks((d) => {
        onBadPhotoScanDone(d, label);
        onAnalysisDone(d, label);
      }).then((ls) => () => ls.forEach((u) => u())),
      // QUAL-20: collections are shared by all windows; a shown collection is listed again.
      listen<CollectionSummary[]>("collections-changed", (e) => {
        setCollections(e.payload);
        if (location()?.kind === "collection") reload();
      }),
      // FSW-02, FSW-04: update tiles and previews after a file system change.
      listen<FsChange>("fs-changed", (e) => {
        const c = e.payload;
        // FLAT-14: a change of photos in a flattened tree starts a new walk.
        if (onFlat(c.dir)) {
          if (c.photos) onFlatTreeChanged(c.dir);
          return;
        }
        if (c.dir !== path()) return;
        if (c.dirChanged) reload();
        const known = new Set(entries().filter((x) => x.d).map(entryPath));
        const folders = c.folders.filter((f) => known.has(f) || c.dirChanged);
        if (folders.length) api.refreshFolderPreviews(folders, sort());
      }),
      // NAV-02: open a folder by drag and drop onto the window.
      getCurrentWebview().onDragDropEvent((e) => {
        const t = e.payload.type;
        if (t === "enter" || t === "over") setDropping(true);
        else if (t === "leave") setDropping(false);
        else if (t === "drop") {
          setDropping(false);
          const p = e.payload.paths;
          // Ignore files dragged out of this window and dropped back on it.
          if (p.length && !p.every((x) => parentPath(x) === path())) runAction("nav.openPath", p[0]);
        }
      }),
    ]);
    unlisten.push(...listeners);
    await Promise.all([start, shown]);
    if (loadError()?.kind === "notFound" && path() !== home) navigate(home, { push: false });
  });

  // FLAT-07: leaving the flat location stops its walk.
  createEffect(
    on(location, (l) => {
      const s = flat();
      if (s && !(l?.kind === "flat" && l.path === s.root)) leaveFlatten();
    }),
  );

  // A flat refresh that waited for the end of a navigation.
  createEffect(
    on(loading, (busy) => {
      if (!busy && flatPending) refreshFlat();
    }),
  );

  // A sort or a grouping that reads analysis results starts the analysis of the location by itself,
  // when no other task runs.
  createEffect(() => {
    if (pendingAnalysis() && !task()) startAnalysis();
  });

  onCleanup(() => {
    window.removeEventListener("keydown", onKeyDown);
    unlisten.forEach((u) => u());
  });

  const infoTarget = () => {
    const vi = viewerIndex();
    if (vi !== null) {
      const img = images()[vi];
      return img ? { path: entryPath(img), isDir: false } : null;
    }
    const sel = selectedEntries();
    if (sel.length === 1) return { path: entryPath(sel[0]), isDir: sel[0].d };
    return null;
  };

  return (
    <div class="app" classList={{ editing: editMode() }}>
      <Toolbar />
      <div class="body">
        <Show when={!sidebarCollapsed()}>
          <Sidebar />
        </Show>
        <main class="main">
          <GridView />
          <FlatBanner />
        </main>
        <Show when={infoOpen() && viewerIndex() === null}>
          <InfoPanel target={infoTarget()} count={selectedEntries().length} />
        </Show>
      </div>
      <StatusBar />
      <Show when={viewerIndex() !== null}>
        <div class="viewer-layer" classList={{ editing: editMode() }}>
          <ViewerView />
          <Show when={infoOpen()}>
            <InfoPanel target={infoTarget()} count={1} />
          </Show>
        </div>
      </Show>
      <Show when={dropping()}>
        <div class="drop-overlay" aria-hidden="true">
          <div>Drop a folder or image to open it</div>
        </div>
      </Show>
      <ContextMenu />
      <DeleteFolderDialog />
      <RenameDialog />
      <ScanCapDialog />
      <Toasts />
    </div>
  );
}
