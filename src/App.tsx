import { createSignal, onCleanup, onMount, Show } from "solid-js";
import { listen, UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { api, FolderPreview, FsChange, parentPath, Settings } from "./lib/api";
import {
  applyPreview,
  editMode,
  entries,
  entryPath,
  images,
  infoOpen,
  loadError,
  navigate,
  openFolderDialog,
  openPath,
  path,
  reload,
  selectedEntries,
  setEditMode,
  setSettings,
  setShowHidden,
  sort,
  viewerIndex,
} from "./lib/store";
import { onKeyDown } from "./lib/keyboard";
import { sidebarCollapsed } from "./lib/ui";
import { Toolbar } from "./components/Toolbar";
import { Sidebar } from "./components/Sidebar";
import { StatusBar } from "./components/StatusBar";
import { InfoPanel } from "./components/InfoPanel";
import { ContextMenu } from "./components/ContextMenu";
import { Toasts } from "./components/Toasts";
import { GridView } from "./views/GridView";
import { ViewerView } from "./views/ViewerView";

export function App() {
  const [dropping, setDropping] = createSignal(false);
  const unlisten: UnlistenFn[] = [];

  onMount(async () => {
    window.addEventListener("keydown", onKeyDown);
    // PERF-02: show the window as soon as the shell is rendered; the folder fills in after.
    const shown = getCurrentWindow().show();
    const [s, home, pending] = await Promise.all([api.getSettings(), api.homeDir(), api.takePendingOpen()]);
    setSettings(s);
    setShowHidden(s.showHidden);

    // NAV-11: restore the last open folder. PLAT-06: or open the file macOS asked us to open.
    const q = new URLSearchParams(location.search).get("folder");
    const start = pending ? openPath(pending) : navigate(q ?? s.lastFolder ?? home, { push: false });

    const listeners = await Promise.all([
      setEditMode(false).then(() => () => {}), // EDIT-02
      listen<FolderPreview>("folder-preview-ready", (e) => applyPreview(e.payload)),
      listen<Settings>("settings-changed", (e) => setSettings(e.payload)),
      listen("menu-open-folder", () => openFolderDialog()),
      listen<string>("open-path", (e) => openPath(e.payload)),
      // FSW-02, FSW-04: update tiles and previews after a file system change.
      listen<FsChange>("fs-changed", (e) => {
        const c = e.payload;
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
          if (p.length && !p.every((x) => parentPath(x) === path())) openPath(p[0]);
        }
      }),
    ]);
    unlisten.push(...listeners);
    await Promise.all([start, shown]);
    if (loadError()?.kind === "notFound" && path() !== home) navigate(home, { push: false });
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
      <Toasts />
    </div>
  );
}
