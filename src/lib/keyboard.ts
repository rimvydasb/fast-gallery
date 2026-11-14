// Keyboard shortcuts (A11Y-01). ⌘N, ⌘O, ⌘, and ⌃⌘F are native menu items; everything else is here.
// Each command goes through the action registry (D4), so a running task blocks it the same way as a
// button or a menu item. Cursor movement and viewer controls are local to the views.
import {
  anchor,
  cursor,
  entries,
  entryPath,
  selectedEntries,
  selectOnly,
  selectRange,
  setAnchor,
  viewerIndex,
  images,
  clearSelection,
  deleteRequest,
  renameRequest,
} from "./store";
import { caps, path } from "./navigation";
import { runAction } from "./actions";
import { scanCapInfo } from "./badPhotos";
import { gridMove, scrollToIndex } from "../views/GridView";
import * as viewer from "../views/ViewerView";
import { contextMenuOpen, openContextMenu } from "../components/ContextMenu";

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
        runAction("nav.parent");
        return true;
      case "ArrowDown": {
        const c = cursor();
        if (c >= 0) runAction("nav.openEntry", c);
        return true;
      }
      case "a":
        runAction("view.selectAll");
        return true;
      case "Backspace": {
        // A selection with folders asks for a confirmation (DeleteFolderDialog).
        const sel = selectedEntries();
        if (sel.some((e) => e.d)) runAction("file.requestTrashItems", sel);
        else runAction("file.trashSelection");
        return true;
      }
      case "c":
        runAction("view.copy");
        return true;
      case "d": {
        const sel = selectedEntries();
        const p = sel.length === 1 && sel[0].d ? entryPath(sel[0]) : caps().favorites ? path() : null;
        runAction("view.toggleFavorite", p);
        return true;
      }
      case "=":
      case "+":
        runAction("view.tileSize", TILE_STEP);
        return true;
      case "-":
        runAction("view.tileSize", -TILE_STEP);
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
      moveCursor(gridMove(cursor(), -1) - cursor(), e.shiftKey);
      return true;
    case "ArrowDown":
      moveCursor(gridMove(cursor(), 1) - cursor(), e.shiftKey);
      return true;
    case "Home":
      moveCursor(-entries().length, e.shiftKey);
      return true;
    case "End":
      moveCursor(entries().length, e.shiftKey);
      return true;
    case "Enter": {
      const c = cursor();
      if (c >= 0) runAction("nav.openEntry", c);
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
      if (c >= 0 && !entries()[c]?.d) runAction("nav.openEntry", c);
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
        runAction("file.trashViewerImage");
        return true;
      case "c": {
        const vi = viewerIndex();
        const img = vi !== null ? images()[vi] : undefined;
        if (img) runAction("view.copy", [entryPath(img)]);
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
  if (e.defaultPrevented || contextMenuOpen() || deleteRequest() !== null || renameRequest() !== null || scanCapInfo() !== null)
    return;
  if (isTextInput(e.target)) return;
  const cmd = e.metaKey;
  let handled = false;

  // Shortcuts in both views.
  if (cmd && !e.ctrlKey) {
    if (e.altKey && (e.code === "KeyS" || e.key === "s")) {
      runAction("view.toggleSidebar");
      handled = true;
    } else if (!e.altKey) {
      switch (e.key.toLowerCase()) {
        case "i":
          runAction("view.toggleInfo");
          handled = true;
          break;
        case "e":
          runAction("view.toggleEditMode");
          handled = true;
          break;
        case "z":
          if (!e.shiftKey) {
            runAction("file.undo");
            handled = true;
          }
          break;
        case "[":
          if (viewerIndex() === null) runAction("nav.back");
          handled = true;
          break;
        case "]":
          if (viewerIndex() === null) runAction("nav.forward");
          handled = true;
          break;
      }
      if (!handled && e.shiftKey && e.code === "KeyR") {
        const vi = viewerIndex();
        const img = vi !== null ? images()[vi] : undefined;
        runAction("view.reveal", img ? [entryPath(img)] : undefined);
        handled = true;
      }
      if (!handled && e.shiftKey && e.code === "Period" && viewerIndex() === null) {
        runAction("view.toggleHidden");
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

