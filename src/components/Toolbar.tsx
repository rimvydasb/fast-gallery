import { Show } from "solid-js";
import { SortKey } from "../lib/api";
import {
  editMode,
  goBack,
  goForward,
  goParent,
  historyState,
  infoOpen,
  path,
  setInfoOpen,
  setSort,
  sort,
  toggleEditMode,
} from "../lib/store";
import { Breadcrumbs } from "./Breadcrumbs";
import { toggleSidebar } from "../lib/ui";

const SORTS: { key: SortKey; label: string }[] = [
  { key: "name", label: "Name" },
  { key: "modified", label: "Date Modified" },
  { key: "taken", label: "Date Taken" },
  { key: "size", label: "Size" },
];

export function Icon(props: { d: string }) {
  return (
    <svg viewBox="0 0 20 20" aria-hidden="true" class="icon">
      <path d={props.d} fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" />
    </svg>
  );
}

export const ICONS = {
  sidebar: "M3 4h14v12H3zM8 4v12",
  back: "M12 4l-6 6 6 6",
  forward: "M8 4l6 6-6 6",
  up: "M10 15V5M5 10l5-5 5 5",
  info: "M10 18a8 8 0 1 0 0-16 8 8 0 0 0 0 16zM10 9v5M10 6.5v.01",
  edit: "M4 16l1-4 8-8 3 3-8 8zM11 6l3 3",
  trash: "M4 6h12M8 6V4h4v2M6 6l1 10h6l1-10",
  grid: "M3 3h6v6H3zM11 3h6v6h-6zM3 11h6v6H3zM11 11h6v6h-6z",
  play: "M6 4l10 6-10 6z",
  pause: "M6 4v12M14 4v12",
  rotL: "M4 9a6 6 0 1 1 2 5M4 4v5h5",
  rotR: "M16 9a6 6 0 1 0-2 5M16 4v5h-5",
  strip: "M2 13h16v4H2zM2 3h16v8H2z",
  asc: "M6 15V5M3 8l3-3 3 3M12 6h5M12 10h4M12 14h3",
  desc: "M6 5v10M3 12l3 3 3-3M12 6h3M12 10h4M12 14h5",
};

/** Toolbar in the transparent title bar (PLAT-05). */
export function Toolbar() {
  return (
    <header class="toolbar" data-tauri-drag-region>
      <div class="traffic-space" data-tauri-drag-region />
      <button class="tb-btn" title="Toggle Sidebar (⌘⌥S)" aria-label="Toggle sidebar" onClick={toggleSidebar}>
        <Icon d={ICONS.sidebar} />
      </button>
      <button class="tb-btn" title="Back (⌘[)" aria-label="Back" disabled={historyState().back === 0} onClick={goBack}>
        <Icon d={ICONS.back} />
      </button>
      <button class="tb-btn" title="Forward (⌘])" aria-label="Forward" disabled={historyState().forward === 0} onClick={goForward}>
        <Icon d={ICONS.forward} />
      </button>
      <button class="tb-btn" title="Enclosing Folder (⌘↑)" aria-label="Enclosing folder" disabled={!path() || path() === "/"} onClick={goParent}>
        <Icon d={ICONS.up} />
      </button>
      <Breadcrumbs />
      <div class="tb-spacer" data-tauri-drag-region />
      <label class="sort-control" title="Sort order">
        <span class="visually-hidden">Sort by</span>
        <select value={sort().key} onChange={(e) => setSort({ ...sort(), key: e.currentTarget.value as SortKey })}>
          {SORTS.map((s) => (
            <option value={s.key}>{s.label}</option>
          ))}
        </select>
      </label>
      <button
        class="tb-btn"
        title={sort().desc ? "Descending" : "Ascending"}
        aria-label={sort().desc ? "Sort descending" : "Sort ascending"}
        onClick={() => setSort({ ...sort(), desc: !sort().desc })}
      >
        <Icon d={sort().desc ? ICONS.desc : ICONS.asc} />
      </button>
      <button
        class="tb-btn edit-toggle"
        classList={{ on: editMode() }}
        title={editMode() ? "Turn Off Edit Mode (⌘E)" : "Turn On Edit Mode (⌘E)"}
        aria-pressed={editMode()}
        aria-label="Edit mode"
        onClick={toggleEditMode}
      >
        <Icon d={ICONS.edit} />
        <Show when={editMode()}>
          <span class="edit-pill">Editing</span>
        </Show>
      </button>
      <button class="tb-btn" classList={{ on: infoOpen() }} title="Info (⌘I)" aria-pressed={infoOpen()} aria-label="Info" onClick={() => setInfoOpen(!infoOpen())}>
        <Icon d={ICONS.info} />
      </button>
    </header>
  );
}
