import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { render } from "solid-js/web";
import { apiMock, file, flatStatus, flush, folder } from "../test/helpers";
import type { Entry, Settings } from "./api";

const mocked = vi.hoisted(() => ({ api: {} as Record<string, ReturnType<typeof vi.fn>> }));
vi.mock("./api", async (orig) => ({ ...(await orig<typeof import("./api")>()), api: mocked.api }));

import { isIgnoredPath } from "./api";
import { actionState, runAction } from "./actions";
import { applyFlatProgress, flat, onFlatTreeChanged, resetFlatten, setFlat } from "./flatten";
import { flatFolder, FlatProvider, locationKey, parseLocation, recentLabel } from "./locations";
import { location, navigate, resetHistory, setLocation, setPath } from "./navigation";
import { entries, entryPath, setEntries, setHomeDir, setSelection, setSettings, sort } from "./store";
import { selectionMenuItems } from "../components/ContextMenu";
import { LoadErrorView } from "../views/GridView";
import { IgnoredFolders, settingsSection } from "../views/SettingsView";

const FLAT = { kind: "flat", path: "/Photos/Card" } as const;
const item = (p: string): Entry => ({ n: p.slice(p.lastIndexOf("/") + 1), d: false, s: 1, m: 1, p });
const labels = () => selectionMenuItems().map((i) => i.label);
const menuItem = (label: string) => selectionMenuItems().find((i) => i.label === label);

const settingsWith = (patch: Partial<Settings>): Settings =>
  ({ favorites: [], recents: [], folderPrefs: {}, ignoredFolders: [], flattenCap: 20000, ...patch }) as Settings;

beforeEach(() => {
  Object.assign(mocked.api, apiMock());
  resetFlatten();
  resetHistory();
  setHomeDir("/Users/me");
  setSettings(settingsWith({ ignoredFolders: ["/Library", "~/Library/Caches"] }));
  setPath("/Photos");
  setEntries([folder("Card"), file("a.jpg")]);
  setSelection(new Set<string>());
});

describe("flat location (FLAT-03, FLAT-15, FLAT-16)", () => {
  it("round-trips the flat: key", () => {
    expect(locationKey(FLAT)).toBe("flat:/Photos/Card");
    expect(parseLocation("flat:/Photos/Card")).toEqual(FLAT);
    for (const l of [FLAT, { kind: "disk", path: "/a" }, { kind: "collection", id: "x" }] as const) {
      expect(parseLocation(locationKey(l))).toEqual(l);
    }
  });

  it("shows the disk crumbs, then [Flattened], and a Recents label", () => {
    expect(FlatProvider.crumbs(FLAT).map((c) => c.label)).toEqual(["Computer", "Photos", "Card", "[Flattened]"]);
    expect(FlatProvider.title(FLAT)).toBe("Card (Flattened)");
    expect(FlatProvider.parent(FLAT)).toEqual({ to: { kind: "disk", path: "/Photos/Card" } });
    expect(recentLabel("flat:/Photos/Card")).toBe("Card (Flattened)");
    expect(recentLabel("/Photos/Card")).toBe("Card");
  });

  it("starts a walk, lists it newest first, and lists again on progress", async () => {
    mocked.api.startFlatten.mockResolvedValue(flatStatus("/Photos/Card"));
    mocked.api.listFlat.mockResolvedValue({ path: "flat:/Photos/Card", entries: [item("/Photos/Card/DCIM/100/IMG_1.JPG")] });
    await navigate(FLAT);
    expect(mocked.api.startFlatten).toHaveBeenCalledWith("/Photos/Card");
    expect(mocked.api.listFlat).toHaveBeenCalledWith("main-1", { key: "modified", desc: true }, "none");
    expect(location()).toEqual(FLAT);
    expect(sort()).toEqual({ key: "modified", desc: true });
    expect(entries().map(entryPath)).toEqual(["/Photos/Card/DCIM/100/IMG_1.JPG"]);

    // A later listing of the same root uses the same session.
    await navigate(FLAT, { push: false });
    expect(mocked.api.startFlatten).toHaveBeenCalledTimes(1);

    expect(applyFlatProgress(flatStatus("/Photos/Card", { count: 40 }))).toBe(true);
    expect(flat()?.count).toBe(40);
    expect(applyFlatProgress(flatStatus("/Photos/Card", { id: "other", count: 9 }))).toBe(false);
    // A refresh walk does not change the listing until it ends.
    expect(applyFlatProgress(flatStatus("/Photos/Card", { refreshing: true }))).toBe(false);
  });

  it("keeps a progress event that arrives before the reply of start_flatten", async () => {
    let reply!: (s: unknown) => void;
    mocked.api.startFlatten.mockReturnValue(new Promise((r) => (reply = r)));
    const { startFlatten } = await import("./flatten");
    const started = startFlatten("/Photos/Card");
    applyFlatProgress(flatStatus("/Photos/Card", { count: 18, done: true }));
    reply(flatStatus("/Photos/Card"));
    expect(await started).toMatchObject({ count: 18, done: true });
    expect(flat()?.done).toBe(true);
  });

  it("stops the walk when the window leaves the location (FLAT-07)", async () => {
    const { leaveFlatten } = await import("./flatten");
    setFlat(flatStatus("/Photos/Card", { count: 3 }));
    mocked.api.cancelFlatten.mockResolvedValue(flatStatus("/Photos/Card", { cancelled: true }));
    leaveFlatten();
    expect(mocked.api.cancelFlatten).toHaveBeenCalledWith("main-1");
    expect(flat()).toBeNull();
  });

  it("walks again after a file change, after the running walk (FLAT-14)", async () => {
    mocked.api.startFlatten.mockResolvedValue(flatStatus("/Photos/Card", { refreshing: true }));
    setFlat(flatStatus("/Photos/Card", { count: 3 }));
    onFlatTreeChanged("/Photos/Card");
    expect(mocked.api.startFlatten).not.toHaveBeenCalled();
    applyFlatProgress(flatStatus("/Photos/Card", { count: 5, done: true }));
    expect(mocked.api.startFlatten).toHaveBeenCalledTimes(1);
    await flush();
    setFlat(flatStatus("/Photos/Card", { count: 5, done: true }));
    onFlatTreeChanged("/Photos/Card");
    expect(mocked.api.startFlatten).toHaveBeenCalledTimes(2);
    // A cancelled walk stays cancelled.
    setFlat(flatStatus("/Photos/Card", { cancelled: true }));
    onFlatTreeChanged("/Photos/Card");
    expect(mocked.api.startFlatten).toHaveBeenCalledTimes(2);
  });

  it("shows the folder of a photo relative to the root (FLAT-11)", () => {
    expect(flatFolder(FLAT, item("/Photos/Card/DCIM/100CANON/IMG_1.JPG"))).toBe("DCIM/100CANON");
    expect(flatFolder(FLAT, "/Photos/Card/top.jpg")).toBe("Card");
    expect(flatFolder({ kind: "disk", path: "/Photos" }, "/Photos/a.jpg")).toBeNull();
  });
});

describe("context menu (FLAT-01, FLAT-02, FLAT-12, IGN-08)", () => {
  it("offers Flatten on 1 folder and on the folder background", () => {
    setSelection(new Set(["Card"]));
    expect(menuItem("Flatten")?.disabled).toBe(false);
    expect(labels()).toContain("Ignore Folder");
    setSelection(new Set<string>());
    expect(menuItem("Flatten")?.disabled).toBe(false);
    setSelection(new Set(["a.jpg"]));
    expect(labels()).not.toContain("Flatten");
  });

  it("disables Flatten on / and in an ignored tree", () => {
    setPath("/");
    setEntries([folder("Users")]);
    const root = menuItem("Flatten")!;
    expect(root.disabled).toBe(true);
    expect(root.title).toBe("Flatten a folder inside the disk");
    expect(actionState("nav.flatten", "/").enabled).toBe(false);
    expect(actionState("nav.flatten", "/Users/me/Library/Caches/x").reason).toBe("This folder is ignored");
    expect(actionState("nav.flatten", "/library").enabled).toBe(false);
    expect(actionState("nav.flatten", "/Users/me/Library").enabled).toBe(true);
  });

  it("offers Show in Folder for a photo of a flat location", async () => {
    setLocation(FLAT);
    setEntries([item("/Photos/Card/DCIM/IMG_1.JPG")]);
    setSelection(new Set(["/Photos/Card/DCIM/IMG_1.JPG"]));
    expect(labels()).toContain("Show in Folder");
    expect(labels()).not.toContain("Flatten");
    mocked.api.listDir.mockResolvedValue({ path: "/Photos/Card/DCIM", entries: [file("IMG_1.JPG")] });
    menuItem("Show in Folder")!.action!();
    await flush();
    expect(mocked.api.listDir).toHaveBeenCalledWith("/Photos/Card/DCIM", expect.anything(), false, "none");
    expect(location()).toEqual({ kind: "disk", path: "/Photos/Card/DCIM" });
  });

  it("adds a folder to the ignored folders", async () => {
    mocked.api.updateSettings.mockImplementation(async (p) => settingsWith(p));
    setSelection(new Set(["Card"]));
    runAction("view.ignoreFolder", "/Photos/Card");
    await flush();
    expect(mocked.api.updateSettings).toHaveBeenCalledWith({ ignoredFolders: ["/Library", "~/Library/Caches", "/Photos/Card"] });
  });
});

describe("ignored folders (IGN-04, IGN-05, IGN-09)", () => {
  it("matches a tree without case and without a trailing /", () => {
    const list = ["/Library", "~/Library/Caches/"];
    expect(isIgnoredPath("/Library", list, "/Users/me")).toBe(true);
    expect(isIgnoredPath("/library/Fonts/", list, "/Users/me")).toBe(true);
    expect(isIgnoredPath("/Users/me/Library/Caches/x", list, "/Users/me")).toBe(true);
    expect(isIgnoredPath("/Users/me/Library", list, "/Users/me")).toBe(false);
    expect(isIgnoredPath("/Library2", list, "/Users/me")).toBe(false);
    expect(isIgnoredPath("/", list, "/Users/me")).toBe(false);
  });

  let dispose: (() => void) | undefined;
  let host: HTMLElement | undefined;
  afterEach(() => {
    dispose?.();
    host?.remove();
  });

  it("shows the ignored message with Open Settings for the ignored error", () => {
    host = document.body.appendChild(document.createElement("div"));
    dispose = render(() => <LoadErrorView error={{ kind: "ignored", message: "This folder is ignored" }} />, host!);
    expect(host!.textContent).toContain("This folder is ignored");
    const btn = [...host!.querySelectorAll("button")].find((b) => b.textContent === "Open Settings")!;
    btn.click();
    expect(mocked.api.openSettings).toHaveBeenCalledWith("ignored");
    expect(host!.textContent).not.toContain("Privacy");
  });

  it("edits the list in Settings: Add Folder…, Remove, Restore Defaults (IGN-01)", async () => {
    host = document.body.appendChild(document.createElement("div"));
    const update = vi.fn().mockResolvedValue(undefined);
    dispose = render(() => <IgnoredFolders list={["/Library", "/X"]} update={update} />, host!);
    const button = (t: string) => [...host!.querySelectorAll("button")].find((b) => b.textContent === t)!;
    expect(button("Remove").disabled).toBe(true);
    host!.querySelectorAll<HTMLElement>(".ignored-item")[1].click();
    button("Remove").click();
    expect(update).toHaveBeenCalledWith({ ignoredFolders: ["/Library"] });
    mocked.api.pickFolder.mockResolvedValue("/Photos/Junk");
    button("Add Folder…").click();
    await flush();
    expect(update).toHaveBeenCalledWith({ ignoredFolders: ["/Library", "/X", "/Photos/Junk"] });
    button("Restore Defaults").click();
    expect(update).toHaveBeenCalledWith({ ignoredFolders: null });
    expect(settingsSection("#settings/ignored")).toBe("ignored");
    expect(settingsSection("#settings")).toBeNull();
  });

  it("navigates to an ignored folder with the ignored error", async () => {
    mocked.api.listDir.mockRejectedValue({ kind: "ignored", message: "This folder is ignored" });
    const { loadError } = await import("./store");
    await navigate("/Users/me/Library/Caches");
    expect(loadError()?.kind).toBe("ignored");
    expect(entries()).toEqual([]);
  });
});
