import { beforeEach, describe, expect, it, vi } from "vitest";
import { apiMock, flush } from "../test/helpers";
import type { DirListing, Entry, TaskDone, TaskStatus } from "./api";

const mocked = vi.hoisted(() => ({ api: {} as Record<string, ReturnType<typeof vi.fn>> }));
vi.mock("./api", async (orig) => ({ ...(await orig<typeof import("./api")>()), api: mocked.api }));

import { entries, group, groups, pendingAnalysis, setGroup, setSettings } from "./store";
import { navigate, resetHistory } from "./navigation";
import { onAnalysisDone, resetAnalysis, startAnalysis } from "./analysis";
import { setTask, task } from "./tasks";
import { groupLabel } from "./format";

const file = (n: string): Entry => ({ n, d: false, s: 1, m: 1 });
const DIR = "/Photos";
const status: TaskStatus = { id: 7, kind: "analyzePhotos", window: "main", root: DIR, locks: [DIR], done: 0, total: 3 };
const done = (outcome: TaskDone["outcome"], failed = 0): TaskDone => ({
  id: 7,
  kind: "analyzePhotos",
  window: "main",
  root: DIR,
  outcome,
  result: { scanned: 3, analyzed: 3 - failed, failed },
});
const listing = (missing: number): DirListing => ({
  path: DIR,
  entries: [file("a.jpg"), file("b.jpg"), file("c.jpg")],
  groups: missing
    ? [{ label: "Not analyzed", count: 3, kind: "missing" }]
    : [
        { label: "Duplicates 1", count: 2 },
        { label: "No duplicates", count: 1, kind: "rest" },
      ],
  missing: missing || undefined,
  need: "similarity",
});

beforeEach(async () => {
  Object.assign(mocked.api, apiMock());
  mocked.api.startAnalyzePhotos.mockResolvedValue(status);
  setTask(null);
  resetHistory();
  resetAnalysis();
  setSettings(null);
  mocked.api.listDir.mockResolvedValue({ path: DIR, entries: [file("a.jpg"), file("b.jpg"), file("c.jpg")] });
  await navigate(DIR);
});

describe("grouping", () => {
  it("lists again with the group and keeps the groups of the listing", async () => {
    expect(group()).toBe("none");
    expect(groups()).toEqual([]);
    mocked.api.listDir.mockResolvedValue(listing(0));
    setGroup("same");
    await flush();
    expect(mocked.api.listDir).toHaveBeenLastCalledWith(DIR, expect.anything(), false, "same");
    expect(groups().map((g) => g.count)).toEqual([2, 1]);
    expect(entries()).toHaveLength(3);
    expect(pendingAnalysis()).toBeNull();
    expect(mocked.api.setFolderPrefs).toHaveBeenLastCalledWith(DIR, expect.objectContaining({ group: "same" }));
  });

  it("formats date groups", () => {
    expect(groupLabel({ label: "Burst 1" })).toBe("Burst 1");
    expect(groupLabel({ label: "2001-02-03", kind: "date" })).toContain("2001");
    const t = new Date();
    const iso = `${t.getFullYear()}-${String(t.getMonth() + 1).padStart(2, "0")}-${String(t.getDate()).padStart(2, "0")}`;
    expect(groupLabel({ label: iso, kind: "date" })).toBe("Today");
  });
});

describe("analysis for a grouping", () => {
  beforeEach(async () => {
    mocked.api.listDir.mockResolvedValue(listing(3));
    setGroup("same");
    await flush();
  });

  it("starts for the missing images and lists again when the task ends", async () => {
    expect(pendingAnalysis()).toEqual({ missing: 3, need: "similarity" });
    await startAnalysis();
    expect(mocked.api.startAnalyzePhotos).toHaveBeenCalledWith(DIR, "similarity", false);
    expect(task()?.kind).toBe("analyzePhotos");

    mocked.api.listDir.mockResolvedValue(listing(0));
    setTask(null);
    onAnalysisDone(done("finished"), "main");
    await flush();
    expect(pendingAnalysis()).toBeNull();
    expect(groups()[0].label).toBe("Duplicates 1");
  });

  it("does not start again for images that failed, or after a cancel, but a manual start does", async () => {
    await startAnalysis();
    setTask(null);
    // 1 image cannot be decoded: it stays without a result.
    mocked.api.listDir.mockResolvedValue(listing(1));
    onAnalysisDone(done("finished", 1), "main");
    await flush();
    expect(pendingAnalysis()?.missing).toBe(1);
    await startAnalysis();
    expect(mocked.api.startAnalyzePhotos).toHaveBeenCalledTimes(1);
    await startAnalysis(true);
    expect(mocked.api.startAnalyzePhotos).toHaveBeenCalledTimes(2);

    setTask(null);
    onAnalysisDone(done("cancelled"), "main");
    await flush();
    await startAnalysis();
    expect(mocked.api.startAnalyzePhotos).toHaveBeenCalledTimes(2);
  });

  it("does not start while another task runs, and tries again after a busy error", async () => {
    setTask({ ...status, kind: "scanBadPhotos" });
    await startAnalysis();
    expect(mocked.api.startAnalyzePhotos).not.toHaveBeenCalled();
    setTask(null);
    mocked.api.startAnalyzePhotos.mockRejectedValueOnce({ kind: "busy", message: "A task is running." });
    await startAnalysis();
    await startAnalysis();
    expect(mocked.api.startAnalyzePhotos).toHaveBeenCalledTimes(2);
  });
});
