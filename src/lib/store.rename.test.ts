import { beforeEach, describe, expect, it, vi } from "vitest";
import { apiMock, file, folder } from "../test/helpers";

const mocked = vi.hoisted(() => ({ api: {} as Record<string, ReturnType<typeof vi.fn>> }));
vi.mock("./api", async (orig) => ({ ...(await orig<typeof import("./api")>()), api: mocked.api }));

import {
  renameFile,
  renameRequest,
  requestRename,
  selection,
  setEditModeSignal,
  setEntries,
  setPath,
  setRenameRequest,
  setSelection,
} from "./store";
import { setToasts, toasts } from "./toast";

const DIR = "/pics";
const texts = () => toasts().map((t) => t.text);

beforeEach(() => {
  Object.assign(mocked.api, apiMock());
  setToasts([]);
  setRenameRequest(null);
  setPath(DIR);
  setEntries([folder("trip"), file("a.jpg"), file("b.jpg")]);
  setSelection(new Set(["a.jpg"]));
  setEditModeSignal(true);
});

describe("requestRename", () => {
  it("opens the dialog for a file in edit mode", () => {
    requestRename(file("a.jpg"));
    expect(renameRequest()).toBe("/pics/a.jpg");
  });

  it("does nothing but tell the user when edit mode is off (EDIT-06)", () => {
    setEditModeSignal(false);
    requestRename(file("a.jpg"));
    expect(renameRequest()).toBeNull();
    expect(texts()).toEqual(["Turn on edit mode (⌘E) to rename files."]);
  });

  it("ignores folders", () => {
    requestRename(folder("trip"));
    expect(renameRequest()).toBeNull();
  });
});

describe("renameFile", () => {
  it("does not call the backend when edit mode is off", async () => {
    setEditModeSignal(false);
    expect(await renameFile("/pics/a.jpg", "c.jpg")).toMatch(/edit mode/);
    expect(mocked.api.renameFile).not.toHaveBeenCalled();
  });

  it.each(["", "  ", "x/y.jpg", "x:y.jpg", ".."])("rejects the invalid name %j without a backend call", async (n) => {
    expect(await renameFile("/pics/a.jpg", n)).toBeTruthy();
    expect(mocked.api.renameFile).not.toHaveBeenCalled();
  });

  it("rejects an unchanged name", async () => {
    expect(await renameFile("/pics/a.jpg", "a.jpg")).toBe("The name did not change.");
    expect(mocked.api.renameFile).not.toHaveBeenCalled();
  });

  it("renames, closes the dialog, reloads, and selects the renamed file", async () => {
    setRenameRequest("/pics/a.jpg");
    mocked.api.renameFile.mockResolvedValue("/pics/c.jpg");
    mocked.api.listDir.mockResolvedValue({ path: DIR, entries: [folder("trip"), file("b.jpg"), file("c.jpg")] });

    expect(await renameFile("/pics/a.jpg", "  c.jpg  ")).toBeNull();

    expect(mocked.api.renameFile).toHaveBeenCalledWith("/pics/a.jpg", "c.jpg"); // trimmed
    expect(renameRequest()).toBeNull();
    expect(texts()).toEqual(["Renamed “a.jpg” to “c.jpg”. ⌘Z to undo."]);
    expect([...selection()]).toEqual(["c.jpg"]);
  });

  it("returns the backend error and keeps the dialog open", async () => {
    setRenameRequest("/pics/a.jpg");
    mocked.api.renameFile.mockRejectedValue({ kind: "other", message: "A file named “b.jpg” already exists." });

    expect(await renameFile("/pics/a.jpg", "b.jpg")).toBe("A file named “b.jpg” already exists.");

    expect(renameRequest()).toBe("/pics/a.jpg");
    expect(mocked.api.listDir).not.toHaveBeenCalled();
    expect(texts()).toEqual([]);
  });
});
