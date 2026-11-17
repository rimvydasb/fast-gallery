import { beforeEach, describe, expect, it, vi } from "vitest";
import { apiMock, file, flush, folder, scanTask } from "../test/helpers";

const mocked = vi.hoisted(() => ({ api: {} as Record<string, ReturnType<typeof vi.fn>> }));
vi.mock("./api", async (orig) => ({ ...(await orig<typeof import("./api")>()), api: mocked.api }));

import { actionState, blockedReason, isEnabled, runAction } from "./actions";
import { setEditModeSignal, setEntries, setSelection, setUndoTargets, setViewerIndex } from "./store";
import { resetHistory, setLocation, setPath } from "./navigation";
import { setTask } from "./tasks";
import { setToasts, toasts } from "./toast";
import { selectionMenuItems } from "../components/ContextMenu";
import { collectionsMenuItems } from "../components/Toolbar";
import { setCollections } from "./locations";

const texts = () => toasts().map((t) => t.text);
const labels = () => selectionMenuItems().map((i) => i.label);
const item = (label: string) => selectionMenuItems().find((i) => i.label === label);

beforeEach(() => {
  Object.assign(mocked.api, apiMock());
  mocked.api.trashFiles.mockResolvedValue({ trashed: [], failed: [] });
  setToasts([]);
  setTask(null);
  setCollections([]);
  resetHistory();
  setViewerIndex(null);
  setUndoTargets([]);
  setPath("/Photos");
  setEntries([folder("2025"), folder("2026"), file("a.jpg")]);
  setSelection(new Set<string>());
  setEditModeSignal(true);
});

describe("task rule (D4, QUAL-02, QUAL-04)", () => {
  it("blocks a fileChange action only for targets in the lock", async () => {
    setTask(scanTask("/Photos/2026"));
    setPath("/Photos/2026");
    setEntries([file("x.jpg")]);
    setSelection(new Set(["x.jpg"]));
    expect(isEnabled("file.trashSelection")).toBe(false);
    expect(blockedReason("file.trashSelection")).toBe("“2026” is being scanned.");

    // ⌘⌫ in the scanned tree shows the toast and does not call the backend.
    runAction("file.trashSelection");
    await flush();
    expect(mocked.api.trashFiles).not.toHaveBeenCalled();
    expect(texts()).toEqual(["“2026” is being scanned."]);

    // Outside the tree the file moves.
    setPath("/Photos/2025");
    setEntries([file("y.jpg")]);
    setSelection(new Set(["y.jpg"]));
    expect(isEnabled("file.trashSelection")).toBe(true);
    runAction("file.trashSelection");
    await flush();
    expect(mocked.api.trashFiles).toHaveBeenCalledWith(["/Photos/2025/y.jpg"]);
  });

  it("blocks a folder that contains the locked tree", () => {
    setTask(scanTask("/Photos/2026"));
    expect(isEnabled("file.requestTrashItems", [folder("2026")])).toBe(false);
    expect(isEnabled("file.requestTrashItems", [folder("2025")])).toBe(true);
    setPath("/");
    expect(isEnabled("file.requestTrashItems", [folder("Photos")])).toBe(false);
    expect(isEnabled("file.requestTrashItems", [folder("Photos2")])).toBe(true);
  });

  it("blocks undo when the restored paths are in the lock", () => {
    setTask(scanTask("/Photos/2026"));
    setUndoTargets(["/Photos/2026/x.jpg"]);
    expect(isEnabled("file.undo")).toBe(false);
    setUndoTargets(["/Photos/2025/x.jpg"]);
    expect(isEnabled("file.undo")).toBe(true);
  });

  it("keeps navigation and view actions available", () => {
    setTask(scanTask("/Photos/2026"));
    setSelection(new Set(["a.jpg"]));
    for (const id of ["nav.openFolderDialog", "nav.newWindow", "view.toggleInfo", "view.toggleEditMode", "view.copy", "view.selectAll"] as const) {
      expect(isEnabled(id), id).toBe(true);
    }
    expect(isEnabled("nav.parent")).toBe(true);
    expect(isEnabled("nav.open", "/Photos/2026")).toBe(true);
  });

  it("blocks dataChange and task starts, but not cancel", () => {
    expect(isEnabled("data.clearCache")).toBe(true);
    setTask(scanTask("/Photos/2026"));
    expect(isEnabled("data.clearCache")).toBe(false);
    expect(blockedReason("data.clearCache")).toBe("A scan of “2026” is running.");
    expect(isEnabled("data.removeCollection", "bad-photos")).toBe(false);
    expect(isEnabled("task.scanBadPhotos", "/Photos/2025")).toBe(false);
    expect(isEnabled("task.cancel")).toBe(true);
  });

  it("explains a disabled file action when edit mode is off", () => {
    setEditModeSignal(false);
    setSelection(new Set(["a.jpg"]));
    expect(actionState("file.trashSelection").enabled).toBe(false);
    runAction("file.trashSelection");
    expect(texts()).toEqual(["Turn on edit mode (⌘E) to move files to the Trash."]);
  });
});

describe("context menu scan items (QUAL-05, QUAL-07, QUAL-30)", () => {
  it("offers a scan on the background and on 1 folder", () => {
    expect(item("Scan for Bad Photos")?.disabled).toBe(false);
    setSelection(new Set(["2026"]));
    item("Scan for Bad Photos")!.action!();
    expect(mocked.api.startScanBadPhotos).toHaveBeenCalledWith("/Photos/2026");
    setSelection(new Set(["2026", "2025"]));
    expect(labels()).not.toContain("Scan for Bad Photos");
    setSelection(new Set(["a.jpg"]));
    expect(labels()).not.toContain("Scan for Bad Photos");
  });

  it("offers a scan in both edit mode states", () => {
    setEditModeSignal(false);
    expect(labels()).toContain("Scan for Bad Photos");
  });

  it("shows Cancel Scan in the task tree and a disabled scan elsewhere", () => {
    setTask(scanTask("/Photos/2026"));
    setSelection(new Set(["2026"]));
    expect(labels()).toContain("Cancel Scan");
    setSelection(new Set(["2025"]));
    const scan = item("Scan for Bad Photos")!;
    expect(scan.disabled).toBe(true);
    expect(scan.title).toBe("A scan of “2026” is running.");
    setSelection(new Set<string>());
    setPath("/Photos/2026/Summer");
    expect(labels()).toContain("Cancel Scan");
    item("Cancel Scan")!.action!();
    expect(mocked.api.cancelTask).toHaveBeenCalled();
  });

  it("is not available in a virtual folder; Remove Collection is", () => {
    setLocation({ kind: "collection", id: "bad-photos" });
    setEntries([{ ...file("a.jpg"), p: "/x/a.jpg" }]);
    expect(labels()).not.toContain("Scan for Bad Photos");
    expect(labels()).toContain("Remove Collection");
  });
});

describe("Collections menu (QUAL-21)", () => {
  it("lists the collections and Cancel Scan while a task runs", () => {
    expect(collectionsMenuItems().map((i) => i.label)).toEqual(["No Collections"]);
    setCollections([{ id: "bad-photos", name: "Bad photos", icon: "quality", count: 23, root: null }]);
    setTask(scanTask("/Photos/2026"));
    expect(collectionsMenuItems().map((i) => i.label)).toEqual(["Bad photos (23)", "", "Cancel Scan"]);
  });
});
