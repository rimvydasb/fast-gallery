import { beforeEach, describe, expect, it, vi } from "vitest";
import { apiMock, flush, scanTask } from "../test/helpers";
import type { Entry } from "./api";

const mocked = vi.hoisted(() => ({ api: {} as Record<string, ReturnType<typeof vi.fn>> }));
vi.mock("./api", async (orig) => ({ ...(await orig<typeof import("./api")>()), api: mocked.api }));

import { entries, entryPath, selectedPaths, selection, setEditModeSignal, setViewerIndex, toggleSelect, trashSelection } from "./store";
import { historyState, location, navigate, resetHistory, setPath } from "./navigation";
import { locationKey, parseLocation, DiskProvider, setCollections } from "./locations";
import { onBadPhotoScanDone, scanCapInfo, setScanCapInfo, startBadPhotoScan } from "./badPhotos";
import { setTask } from "./tasks";
import { setToasts, toasts } from "./toast";

const item = (p: string, b?: string[]): Entry => ({ n: p.slice(p.lastIndexOf("/") + 1), d: false, s: 1, m: 1, p, b });
const BAD = { kind: "collection", id: "bad-photos" } as const;

beforeEach(() => {
  Object.assign(mocked.api, apiMock());
  setToasts([]);
  setTask(null);
  setScanCapInfo(null);
  setViewerIndex(null);
  resetHistory();
  setCollections([{ id: "bad-photos", name: "Bad photos", icon: "quality", count: 2 }]);
  setPath("/Photos");
  setEditModeSignal(true);
});

describe("locations", () => {
  it("round-trips location keys", () => {
    for (const l of [{ kind: "disk", path: "/a/b" }, BAD] as const) expect(parseLocation(locationKey(l))).toEqual(l);
  });

  it("shows /Volumes/X as one crumb", () => {
    expect(DiskProvider.crumbs({ kind: "disk", path: "/Volumes/Disk/x" }).map((c) => c.label)).toEqual(["Disk", "x"]);
  });
});

describe("virtual folder (QUAL-22)", () => {
  it("lists a collection, selects by path, and trashes by path", async () => {
    // 2 files with the same name in different folders.
    mocked.api.listCollection.mockResolvedValue({
      path: "collection:bad-photos",
      entries: [item("/a/IMG_1.jpg", ["quality.blurry"]), item("/b/IMG_1.jpg"), item("/b/IMG_2.jpg")],
    });
    await navigate(BAD);
    expect(location()).toEqual(BAD);
    expect(historyState().back).toBe(1);
    expect(document.title).toBe("Bad photos");
    toggleSelect(1);
    expect([...selection()]).toEqual(["/b/IMG_1.jpg"]);
    expect(selectedPaths()).toEqual(["/b/IMG_1.jpg"]);
    expect(entryPath(entries()[0])).toBe("/a/IMG_1.jpg");

    mocked.api.trashFiles.mockResolvedValue({ trashed: ["/b/IMG_1.jpg"], failed: [] });
    await trashSelection();
    expect(mocked.api.trashFiles).toHaveBeenCalledWith(["/b/IMG_1.jpg"]);
    expect(entries().map(entryPath)).toEqual(["/a/IMG_1.jpg", "/b/IMG_2.jpg"]);
    expect([...selection()]).toEqual(["/b/IMG_2.jpg"]);
  });
});

describe("scan result (QUAL-12, QUAL-13)", () => {
  const done = (capped: boolean, window = "main") => ({
    id: 1,
    kind: "scanBadPhotos" as const,
    window,
    root: "/Photos/2026",
    outcome: "finished" as const,
    result: { collectionId: "bad-photos", found: 23, scanned: capped ? 200 : 140, capped },
  });

  beforeEach(() => {
    mocked.api.startScanBadPhotos.mockResolvedValue(scanTask("/Photos/2026"));
    mocked.api.listCollection.mockResolvedValue({ path: "collection:bad-photos", entries: [] });
  });

  it("opens Bad photos in the source window at the origin location", async () => {
    await startBadPhotoScan("/Photos/2026");
    onBadPhotoScanDone(done(false), "main");
    await flush();
    expect(location()).toEqual(BAD);
    expect(toasts().map((t) => t.text)).toEqual(["Found 23 bad photos in 140 photos."]);
  });

  it("offers an Open button when the user browsed away", async () => {
    await startBadPhotoScan("/Photos/2026");
    setPath("/Elsewhere");
    onBadPhotoScanDone(done(false), "main");
    await flush();
    expect(location()).toEqual({ kind: "disk", path: "/Elsewhere" });
    expect(toasts()[0].action).toEqual({ label: "Open", actionId: "nav.openCollection", args: "bad-photos" });
  });

  it("does not close an open viewer", async () => {
    await startBadPhotoScan("/Photos/2026");
    setViewerIndex(0);
    onBadPhotoScanDone(done(true), "main");
    expect(location()).toEqual({ kind: "disk", path: "/Photos" });
    expect(scanCapInfo()).toMatchObject({ cap: 200, found: 23, rootName: "2026", showOpen: true });
  });

  it("shows the cap dialog without Open after the auto-open", async () => {
    await startBadPhotoScan("/Photos/2026");
    onBadPhotoScanDone(done(true), "main");
    await flush();
    expect(location()).toEqual(BAD);
    expect(scanCapInfo()?.showOpen).toBe(false);
  });

  it("does nothing in other windows", () => {
    onBadPhotoScanDone(done(false, "w2"), "main");
    expect(toasts()).toEqual([]);
    expect(location()).toEqual({ kind: "disk", path: "/Photos" });
  });

  it("does not open anything after a cancel", async () => {
    await startBadPhotoScan("/Photos/2026");
    onBadPhotoScanDone({ ...done(false), outcome: "cancelled" }, "main");
    expect(location()).toEqual({ kind: "disk", path: "/Photos" });
    expect(toasts().map((t) => t.text)).toEqual(["The scan was cancelled."]);
  });
});
