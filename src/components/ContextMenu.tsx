import { createEffect, createSignal, For, onCleanup, Show } from "solid-js";
import { api, errorMessage } from "../lib/api";
import { editMode, entries, entryPath, selectedEntries, settings, showHidden } from "../lib/store";
import { caps, location, path } from "../lib/navigation";
import { ActionId, actionState, runActionById } from "../lib/actions";
import { inTaskTree, task } from "../lib/tasks";
import { toast } from "../lib/toast";
import { Icon, ICONS } from "./Toolbar";

export interface MenuItem {
  label: string;
  shortcut?: string;
  action?: () => unknown;
  /** Replaces the menu with these items (a keyboard-friendly submenu). */
  submenu?: () => Promise<MenuItem[]>;
  separator?: boolean;
  danger?: boolean;
  /** Path data for a leading icon (see ICONS). */
  icon?: string;
  /** Shown greyed out; `title` says why. */
  disabled?: boolean;
  /** Tooltip, for example the reason why the item is disabled. */
  title?: string;
}

/** A menu item that runs a registry action; disabled with the blocked reason as its tooltip (D4). */
export function actionItem(id: ActionId, args?: unknown, extra: Partial<MenuItem> = {}): MenuItem {
  const st = actionState(id, args);
  return {
    label: st.label,
    shortcut: st.shortcut,
    ...extra,
    disabled: !st.enabled,
    title: st.reason ?? undefined,
    action: () => runActionById(id, args),
  };
}

/**
 * "Scan for Bad Photos" for a scan root. While a task runs: "Cancel Scan" in the tree of the task
 * root, else the scan item, disabled (QUAL-05, QUAL-30).
 */
export function scanItem(root: string | null): MenuItem | null {
  if (!root) return null;
  const t = task();
  if (t && inTaskTree(root, t)) return actionItem("task.cancel", undefined, { icon: ICONS.cancel });
  return actionItem("task.scanBadPhotos", root, { icon: ICONS.scan });
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

/** Items for the current selection (or the location, if nothing is selected). */
export function selectionMenuItems(): MenuItem[] {
  const sel = selectedEntries();
  const loc = location();
  const cur = path();
  const c = caps();
  const favs = settings()?.favorites ?? [];
  const sep: MenuItem = { separator: true, label: "" };
  if (sel.length === 0) {
    const items: MenuItem[] = [
      actionItem("nav.openFolderDialog", undefined, { icon: ICONS.folder }),
      actionItem("nav.newWindow", undefined, { icon: ICONS.window }),
    ];
    if (cur) {
      items.push(sep);
      if (c.favorites)
        items.push(
          actionItem("view.toggleFavorite", cur, {
            label: favs.includes(cur) ? "Remove Folder from Favorites" : "Add Folder to Favorites",
            icon: ICONS.star,
          }),
        );
      if (c.folderTiles)
        items.push(
          actionItem("view.toggleHidden", undefined, {
            label: showHidden() ? "Hide Hidden Files" : "Show Hidden Files",
            icon: showHidden() ? ICONS.eyeOff : ICONS.eye,
          }),
        );
      items.push(actionItem("view.reveal", [cur], { label: "Reveal Folder in Finder", icon: ICONS.reveal }));
      const scan = c.scan ? scanItem(cur) : null;
      if (scan) items.push(sep, scan);
    }
    if (loc?.kind === "collection") items.push(sep, actionItem("data.removeCollection", loc.id, { icon: ICONS.remove }));
    return items;
  }
  const one = sel.length === 1 ? sel[0] : null;
  const onePath = one ? entryPath(one) : null;
  const items: MenuItem[] = [];
  if (one && onePath) {
    items.push(
      actionItem("nav.openEntry", entries().indexOf(one), {
        label: one.d ? "Open" : "Open in Viewer",
        icon: ICONS.open,
        shortcut: one.d ? "⌘↓" : "↩",
      }),
    );
    if (!one.d) items.push({ label: "Open With", icon: ICONS.openWith, submenu: openWithItems(onePath) });
    if (one.d)
      items.push(
        actionItem("view.toggleFavorite", onePath, {
          label: favs.includes(onePath) ? "Remove from Favorites" : "Add to Favorites",
          icon: ICONS.star,
        }),
      );
  }
  items.push(actionItem("view.reveal", undefined, { icon: ICONS.reveal }), actionItem("view.copy", undefined, { icon: ICONS.copy }));
  // QUAL-07: a scan of 1 selected folder.
  const scan = one?.d && c.scan ? scanItem(onePath) : null;
  if (scan) items.push(sep, scan);
  // EDIT-06: actions that change files only in edit mode.
  if (editMode()) {
    const hasFolder = sel.some((e) => e.d);
    items.push(sep);
    if (one && !one.d) items.push(actionItem("file.rename", one, { icon: ICONS.rename }));
    const trash = hasFolder
      ? actionItem("file.requestTrashItems", sel, { icon: ICONS.trash, danger: true })
      : actionItem("file.trashSelection", undefined, {
          label: sel.length === 1 ? "Move to Trash" : `Move ${sel.length} Items to Trash`,
          icon: ICONS.trash,
          danger: true,
        });
    items.push(trash);
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
    if (item.separator || item.disabled) return;
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
    return !!it && !it.separator && !it.disabled && (!!it.action || !!it.submenu);
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
                  aria-disabled={!selectable(i())}
                  class="menu-item"
                  title={item.title}
                  classList={{ active: active() === i(), danger: !!item.danger, disabled: !selectable(i()) }}
                  onMouseEnter={() => selectable(i()) && setActive(i())}
                  onClick={() => run(item)}
                >
                  <span class="menu-label">
                    {item.icon && <Icon d={item.icon} />}
                    {item.label}
                  </span>
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
