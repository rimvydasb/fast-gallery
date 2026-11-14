// Keyboard shortcuts (A11Y-01). ⌘N, ⌘O, ⌘, and ⌃⌘F are native menu items; everything else is here.
import {
  anchor,
  copySelection,
  cursor,
  entries,
  entryPath,
  goBack,
  goForward,
  goParent,
  infoOpen,
  openEntry,
  path,
  revealSelection,
  selectAll,
  selectedEntries,
  selectOnly,
  selectRange,
  setAnchor,
  setInfoOpen,
  setTileSize,
  tileSize,
  toggleEditMode,
  toggleFavorite,
  toggleHidden,
  trashSelection,
  trashViewerImage,
  undoTrash,
  viewerIndex,
  images,
  clearSelection,
  deleteRequest,
} from "./store";
import { gridCols, scrollToIndex } from "../views/GridView";
import * as viewer from "../views/ViewerView";
import { contextMenuOpen, openContextMenu } from "../components/ContextMenu";
import { toggleSidebar } from "./ui";

const TILE_STEP = 32;

function isTextInput(t: EventTarget | null): boolean {
  if (!(t instanceof HTMLElement)) return false;
  if (t instanceof HTMLInputElement) return t.type !== "range" && t.type !== "checkbox";
  return t instanceof HTMLTextAreaElement || t.isContentEditable;
}

// ----- GRID-16: type to select -----
let typed = "";
let typedAt = 0;

function typeSelect(ch: string) {
  const now = performance.now();
  typed = now - typedAt > 1000 ? ch : typed + ch;
  typedAt = now;
  const q = typed.toLocaleLowerCase();
  const e = entries();
  const i = e.findIndex((x) => x.n.toLocaleLowerCase().startsWith(q));
  if (i >= 0) {
    selectOnly(i);
    scrollToIndex(i);
  }
}

function moveCursor(delta: number, extend: boolean) {
  const n = entries().length;
  if (n === 0) return;
  const c = cursor();
  const next = c < 0 ? 0 : Math.max(0, Math.min(n - 1, c + delta));
  if (extend) {
    if (anchor() < 0) setAnchor(c < 0 ? 0 : c);
    selectRange(next);
  } else selectOnly(next);
  scrollToIndex(next);
}

function gridKey(e: KeyboardEvent): boolean {
  const cmd = e.metaKey;
  const k = e.key;
  if (cmd && !e.altKey) {
    switch (k) {
      case "ArrowUp":
        goParent();
        return true;
      case "ArrowDown": {
        const c = cursor();
        if (c >= 0) openEntry(c);
        return true;
      }
      case "a":
        selectAll();
        return true;
      case "Backspace":
        trashSelection();
        return true;
      case "c":
        copySelection();
        return true;
      case "d": {
        const sel = selectedEntries();
        const p = sel.length === 1 && sel[0].d ? entryPath(sel[0]) : path();
        if (p) toggleFavorite(p);
        return true;
      }
      case "=":
      case "+":
        setTileSize(tileSize() + TILE_STEP);
        return true;
      case "-":
        setTileSize(tileSize() - TILE_STEP);
        return true;
    }
    return false;
  }
  if (e.altKey || e.ctrlKey) return false;
  switch (k) {
    case "ArrowLeft":
      moveCursor(-1, e.shiftKey);
      return true;
    case "ArrowRight":
      moveCursor(1, e.shiftKey);
      return true;
    case "ArrowUp":
      moveCursor(-gridCols(), e.shiftKey);
      return true;
    case "ArrowDown":
      moveCursor(gridCols(), e.shiftKey);
      return true;
    case "Home":
      moveCursor(-entries().length, e.shiftKey);
      return true;
    case "End":
      moveCursor(entries().length, e.shiftKey);
      return true;
    case "Enter": {
      const c = cursor();
      if (c >= 0) openEntry(c);
      return true;
    }
    case "Escape":
      clearSelection();
      return true;
  }
  if (k.length === 1) {
    if (k === " " && typed === "") {
      // Space opens the viewer at the selected image, like Quick Look.
      const c = cursor();
      if (c >= 0 && !entries()[c]?.d) openEntry(c);
      return true;
    }
    typeSelect(k);
    return true;
  }
  return false;
}

function viewerKey(e: KeyboardEvent): boolean {
  const cmd = e.metaKey;
  const k = e.key;
  if (cmd && !e.altKey && !e.ctrlKey) {
    switch (k) {
      case "=":
      case "+":
        viewer.zoomIn();
        return true;
      case "-":
        viewer.zoomOut();
        return true;
      case "0":
        viewer.zoomFit();
        return true;
      case "1":
        viewer.zoomActual();
        return true;
      case "r":
        viewer.rotate(90);
        return true;
      case "l":
        viewer.rotate(-90);
        return true;
      case "Backspace":
        trashViewerImage();
        return true;
      case "c": {
        const vi = viewerIndex();
        const img = vi !== null ? images()[vi] : undefined;
        if (img) copySelection([entryPath(img)]);
        return true;
      }
    }
    return false;
  }
  if (e.altKey || e.ctrlKey || cmd) return false;
  switch (k) {
    case "ArrowRight":
    case "PageDown":
      viewer.next();
      return true;
    case " ":
      if (e.shiftKey) viewer.prev();
      else viewer.next();
      return true;
    case "ArrowLeft":
    case "PageUp":
      viewer.prev();
      return true;
    case "Home":
      viewer.first();
      return true;
    case "End":
      viewer.last();
      return true;
    case "Escape":
      viewer.closeViewer();
      return true;
    case "f":
    case "F":
      viewer.toggleFilmstrip();
      return true;
    case "s":
    case "S":
      viewer.toggleSlideshow();
      return true;
  }
  return false;
}

export function onKeyDown(e: KeyboardEvent) {
  if (e.defaultPrevented || contextMenuOpen() || deleteRequest() !== null) return;
  if (isTextInput(e.target)) return;
  const cmd = e.metaKey;
  let handled = false;

  // Shortcuts in both views.
  if (cmd && !e.ctrlKey) {
    if (e.altKey && (e.code === "KeyS" || e.key === "s")) {
      toggleSidebar();
      handled = true;
    } else if (!e.altKey) {
      switch (e.key.toLowerCase()) {
        case "i":
          setInfoOpen(!infoOpen());
          handled = true;
          break;
        case "e":
          toggleEditMode();
          handled = true;
          break;
        case "z":
          if (!e.shiftKey) {
            undoTrash();
            handled = true;
          }
          break;
        case "[":
          if (viewerIndex() === null) goBack();
          handled = true;
          break;
        case "]":
          if (viewerIndex() === null) goForward();
          handled = true;
          break;
      }
      if (!handled && e.shiftKey && e.code === "KeyR") {
        const vi = viewerIndex();
        const img = vi !== null ? images()[vi] : undefined;
        revealSelection(img ? [entryPath(img)] : undefined);
        handled = true;
      }
      if (!handled && e.shiftKey && e.code === "Period" && viewerIndex() === null) {
        toggleHidden();
        handled = true;
      }
    }
  }
  if (!handled && ((e.shiftKey && e.key === "F10") || e.key === "ContextMenu") && viewerIndex() === null) {
    const c = cursor();
    const el = c >= 0 ? document.getElementById(`tile-${c}`) : null;
    const r = el?.getBoundingClientRect();
    openContextMenu(r ? r.left + r.width / 2 : 200, r ? r.top + r.height / 2 : 200);
    handled = true;
  }
  if (!handled) handled = viewerIndex() !== null ? viewerKey(e) : gridKey(e);
  if (handled) {
    e.preventDefault();
    e.stopPropagation();
  }
}

