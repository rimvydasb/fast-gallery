import { beforeEach, describe, expect, it, vi } from "vitest";
import { apiMock, file, folder } from "../test/helpers";

const mocked = vi.hoisted(() => ({ api: {} as Record<string, ReturnType<typeof vi.fn>> }));
vi.mock("../lib/api", async (orig) => ({ ...(await orig<typeof import("../lib/api")>()), api: mocked.api }));

import { selectionMenuItems } from "./ContextMenu";
import { renameRequest, setEditModeSignal, setEntries, setPath, setRenameRequest, setSelection } from "../lib/store";

const labels = () => selectionMenuItems().map((i) => i.label);
const rename = () => selectionMenuItems().find((i) => i.label === "Rename…");

beforeEach(() => {
  Object.assign(mocked.api, apiMock());
  setRenameRequest(null);
  setPath("/pics");
  setEntries([folder("trip"), file("a.jpg"), file("b.jpg")]);
  setSelection(new Set(["a.jpg"]));
  setEditModeSignal(true);
});

describe("context menu: Rename…", () => {
  it("is offered for one file in edit mode, before Move to Trash", () => {
    const l = labels();
    expect(l).toContain("Rename…");
    expect(l.indexOf("Rename…")).toBeLessThan(l.indexOf("Move to Trash"));
  });

  it("opens the rename dialog for that file", async () => {
    await rename()!.action!();
    expect(renameRequest()).toBe("/pics/a.jpg");
  });

  it("is hidden when edit mode is off (EDIT-06)", () => {
    setEditModeSignal(false);
    expect(labels()).not.toContain("Rename…");
  });

  it("is hidden for a folder", () => {
    setSelection(new Set(["trip"]));
    expect(labels()).not.toContain("Rename…");
  });

  it("is hidden for several items", () => {
    setSelection(new Set(["a.jpg", "b.jpg"]));
    expect(labels()).not.toContain("Rename…");
  });

  it("is hidden when nothing is selected", () => {
    setSelection(new Set<string>());
    expect(labels()).not.toContain("Rename…");
  });
});
