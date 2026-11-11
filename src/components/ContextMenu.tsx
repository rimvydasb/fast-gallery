import { createEffect, createSignal, For, onCleanup, Show } from "solid-js";
import { api, baseName, errorMessage } from "../lib/api";
import {
  copySelection,
  editMode,
  entries,
  entryPath,
  openEntry,
  openFolderDialog,
  path,
  requestTrashFolder,
  revealSelection,
  selectedEntries,
  settings,
  showHidden,
  toggleFavorite,
  toggleHidden,
  trashSelection,
} from "../lib/store";
import { toast } from "../lib/toast";

export interface MenuItem {
  label: string;
  shortcut?: string;
  action?: () => void | Promise<void>;
  /** Replaces the menu with these items (a keyboard-friendly submenu). */
  submenu?: () => Promise<MenuItem[]>;
  separator?: boolean;
  danger?: boolean;
}

const [menu, setMenu] = createSignal<{ x: number; y: number; items: MenuItem[] } | null>(null);

export const contextMenuOpen = () => menu() !== null;
export const closeContextMenu = () => setMenu(null);

export function showMenu(x: number, y: number, items: MenuItem[]) {
  setMenu({ x, y, items });
}

function openWithItems(p: string): () => Promise<MenuItem[]> {
  return async () => {
    try {
      const apps = await api.openWithApps(p);
      if (apps.length === 0) return [{ label: "No applications" }];
      return apps.map((a) => ({
        label: a.default ? `${a.name} (default)` : a.name,
        action: () => api.openWith(p, a.path).catch((e) => toast(errorMessage(e), "error")),
      }));
    } catch (e) {
      toast(errorMessage(e), "error");
      return [];
    }
  };
}

/** Items for the current selection (or the folder, if nothing is selected). */
export function selectionMenuItems(): MenuItem[] {
  const sel = selectedEntries();
  const cur = path();
  const favs = settings()?.favorites ?? [];
  if (sel.length === 0) {
    const items: MenuItem[] = [
      { label: "Open Folder…", shortcut: "⌘O", action: openFolderDialog },
      { label: "New Window", shortcut: "⌘N", action: () => api.newWindow() },
    ];
    if (cur) {
      items.push(
        { separator: true, label: "" },
        {
          label: favs.includes(cur) ? "Remove Folder from Favorites" : "Add Folder to Favorites",
          shortcut: "⌘D",
          action: () => toggleFavorite(cur),
        },
        { label: showHidden() ? "Hide Hidden Files" : "Show Hidden Files", shortcut: "⌘⇧.", action: toggleHidden },
        { label: "Reveal Folder in Finder", shortcut: "⌘⇧R", action: () => api.revealInFinder([cur]) },
      );
    }
    return items;
  }
  const one = sel.length === 1 ? sel[0] : null;
  const onePath = one ? entryPath(one) : null;
  const items: MenuItem[] = [];
  if (one && onePath) {
    items.push({ label: one.d ? "Open" : "Open in Viewer", shortcut: one.d ? "⌘↓" : "↩", action: () => openEntry(entries().indexOf(one)) });
    if (!one.d) items.push({ label: "Open With", submenu: openWithItems(onePath) });
    if (one.d)
      items.push({
        label: favs.includes(onePath) ? "Remove from Favorites" : "Add to Favorites",
        shortcut: "⌘D",
        action: () => toggleFavorite(onePath),
      });
  }
  items.push(
    { label: "Reveal in Finder", shortcut: "⌘⇧R", action: () => revealSelection() },
    { label: sel.length === 1 ? "Copy" : `Copy ${sel.length} Items`, shortcut: "⌘C", action: () => copySelection() },
  );
  if (one && !one.d && onePath) {
    items.push({
      label: "Set as Desktop Picture",
      action: () =>
        api
          .setDesktopPicture(onePath)
          .then(() => toast(`Set “${baseName(onePath)}” as the desktop picture.`))
          .catch((e) => toast(errorMessage(e), "error")),
    });
  }
  // EDIT-06: actions that change files only in edit mode.
  if (editMode() && sel.some((e) => !e.d)) {
    items.push({ separator: true, label: "" }, { label: "Move to Trash", shortcut: "⌘⌫", danger: true, action: trashSelection });
  }
  if (editMode() && one?.d && onePath) {
    items.push(
      { separator: true, label: "" },
      { label: "Move Folder to Trash…", danger: true, action: () => requestTrashFolder(onePath) },
    );
  }
  return items;
}

export function openContextMenu(x: number, y: number) {
  showMenu(x, y, selectionMenuItems());
}

export function ContextMenu() {
  let el!: HTMLDivElement;
  const [active, setActive] = createSignal(0);
  const [pos, setPos] = createSignal({ x: 0, y: 0 });

  const run = async (item: MenuItem) => {
    if (item.separator) return;
    if (item.submenu) {
      const m = menu();
      const sub = await item.submenu();
      if (m) setMenu({ ...m, items: sub });
      setActive(0);
      return;
    }
    closeContextMenu();
    await item.action?.();
  };

  const selectable = (i: number) => {
    const it = menu()?.items[i];
    return !!it && !it.separator && (!!it.action || !!it.submenu);
  };

  const move = (d: number) => {
    const n = menu()?.items.length ?? 0;
    let i = active();
    for (let k = 0; k < n; k++) {
      i = (i + d + n) % n;
      if (selectable(i)) break;
    }
    setActive(i);
  };

  createEffect(() => {
    const m = menu();
    if (!m) return;
    setActive(m.items.findIndex((_, i) => selectable(i)));
    setPos({ x: m.x, y: m.y });
    queueMicrotask(() => {
      if (!el) return;
      const r = el.getBoundingClientRect();
      setPos({
        x: Math.min(m.x, window.innerWidth - r.width - 6),
        y: Math.min(m.y, window.innerHeight - r.height - 6),
      });
      el.focus();
    });
  });

  const onKey = (e: KeyboardEvent) => {
    if (!menu()) return;
    e.stopPropagation();
    e.preventDefault();
    if (e.key === "ArrowDown") move(1);
    else if (e.key === "ArrowUp") move(-1);
    else if (e.key === "Escape") closeContextMenu();
    else if (e.key === "Enter" || e.key === " " || e.key === "ArrowRight") {
      const it = menu()!.items[active()];
      if (it && (e.key !== "ArrowRight" || it.submenu)) run(it);
    }
  };

  const onDocDown = (e: MouseEvent) => {
    if (menu() && el && !el.contains(e.target as Node)) closeContextMenu();
  };
  document.addEventListener("mousedown", onDocDown, true);
  window.addEventListener("blur", closeContextMenu);
  onCleanup(() => {
    document.removeEventListener("mousedown", onDocDown, true);
    window.removeEventListener("blur", closeContextMenu);
  });

  return (
    <Show when={menu()}>
      {(m) => (
        <div
          class="context-menu"
          ref={el}
          role="menu"
          tabIndex={-1}
          style={{ left: `${pos().x}px`, top: `${pos().y}px` }}
          onKeyDown={onKey}
          onContextMenu={(e) => e.preventDefault()}
        >
          <For each={m().items}>
            {(item, i) =>
              item.separator ? (
                <div class="menu-sep" role="separator" />
              ) : (
                <div
                  role="menuitem"
                  aria-disabled={!item.action && !item.submenu}
                  class="menu-item"
                  classList={{ active: active() === i(), danger: !!item.danger, disabled: !item.action && !item.submenu }}
                  onMouseEnter={() => selectable(i()) && setActive(i())}
                  onClick={() => run(item)}
                >
                  <span>{item.label}</span>
                  <span class="menu-shortcut">{item.submenu ? "›" : item.shortcut ?? ""}</span>
                </div>
              )
            }
          </For>
        </div>
      )}
    </Show>
  );
}
