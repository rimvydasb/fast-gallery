import { vi } from "vitest";
import type { Entry } from "../lib/api";

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
});
