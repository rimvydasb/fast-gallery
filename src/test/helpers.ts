import { vi } from "vitest";
import type { Entry, TaskStatus } from "../lib/api";

export const file = (n: string): Entry => ({ n, d: false, s: 1, m: 1 });
export const folder = (n: string): Entry => ({ n, d: true, s: 0, m: 1 });

/** Awaits pending microtasks and effects. No timers, so tests stay deterministic. */
export const flush = async () => {
  for (let i = 0; i < 5; i++) await Promise.resolve();
};

export const apiMock = () => ({
  renameFile: vi.fn(),
  listDir: vi.fn(),
  setEditMode: vi.fn(),
  trashFiles: vi.fn(),
  undoTrash: vi.fn(),
  openWithApps: vi.fn(),
  openWith: vi.fn(),
  revealInFinder: vi.fn(),
  copyFiles: vi.fn(),
  newWindow: vi.fn(),
  toggleFavorite: vi.fn(),
  clearCache: vi.fn(),
  cancelTask: vi.fn(),
  startScanBadPhotos: vi.fn(),
  listCollection: vi.fn(),
  removeCollection: vi.fn(),
  getTaskStatus: vi.fn(),
  homeDir: vi.fn(),
  setFolderPrefs: vi.fn(),
  startAnalyzePhotos: vi.fn(),
});

/** A running scan task with a lock on `root`. */
export const scanTask = (root: string, window = "main"): TaskStatus => ({
  id: 1,
  kind: "scanBadPhotos",
  window,
  root,
  locks: [root],
  done: 70,
  total: 200,
});
