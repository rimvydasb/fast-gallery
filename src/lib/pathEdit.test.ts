import { beforeEach, describe, expect, it, vi } from "vitest";
import { apiMock, flush } from "../test/helpers";

const mocked = vi.hoisted(() => ({ api: {} as Record<string, ReturnType<typeof vi.fn>> }));
vi.mock("./api", async (orig) => ({ ...(await orig<typeof import("./api")>()), api: mocked.api }));

import { location, resetHistory, setLocation, setPath } from "./navigation";
import { setCollections } from "./locations";
import { normalizePath, pathText, submitPath } from "./pathEdit";
import { setToasts, toasts } from "./toast";

beforeEach(() => {
  Object.assign(mocked.api, apiMock());
  mocked.api.listDir.mockImplementation(async (path: string) => ({ path, entries: [] }));
  mocked.api.homeDir.mockResolvedValue("/Users/me");
  setToasts([]);
  resetHistory();
  setCollections([{ id: "bad-photos", name: "Bad photos", icon: "quality", count: 2, root: "/Users/me/2014" }]);
  setPath("/Users/me/Photos");
});

describe("path field", () => {
  it("shows the disk path, or the root and [name] of a virtual folder", () => {
    expect(pathText({ kind: "disk", path: "/a/b" })).toBe("/a/b");
    expect(pathText({ kind: "collection", id: "bad-photos" })).toBe("/Users/me/2014/[Bad photos]");
    setCollections([{ id: "x", name: "X", icon: "quality", count: 0, root: null }]);
    expect(pathText({ kind: "collection", id: "x" })).toBe("Collections/[X]");
  });

  it("normalizes paths", () => {
    expect(normalizePath("/a//b/./c/../")).toBe("/a/b");
    expect(normalizePath("/..")).toBe("/");
  });

  it("goes to a folder; accepts ~, quotes, and relative paths", async () => {
    mocked.api.pathKind.mockResolvedValue("dir");
    expect(await submitPath(' "~/Desktop/" ')).toBe(true);
    expect(location()).toEqual({ kind: "disk", path: "/Users/me/Desktop" });
    expect(await submitPath("../Downloads")).toBe(true);
    expect(location()).toEqual({ kind: "disk", path: "/Users/me/Downloads" });
  });

  it("opens the folder of a file", async () => {
    mocked.api.pathKind.mockResolvedValueOnce("file").mockResolvedValueOnce("file");
    await submitPath("/Users/me/x/IMG_1.jpg");
    await flush();
    expect(location()).toEqual({ kind: "disk", path: "/Users/me/x" });
  });

  it("shows an error and stays for a path that does not exist", async () => {
    mocked.api.pathKind.mockResolvedValue("missing");
    expect(await submitPath("/nope")).toBe(false);
    expect(location()).toEqual({ kind: "disk", path: "/Users/me/Photos" });
    expect(toasts().slice(-1)[0]?.text).toContain("“/nope” does not exist");
  });

  it("does nothing for the current location", async () => {
    setLocation({ kind: "collection", id: "bad-photos" });
    expect(await submitPath("/Users/me/2014/[Bad photos]")).toBe(true);
    expect(mocked.api.pathKind).not.toHaveBeenCalled();
    expect(await submitPath("Desktop")).toBe(false);
    expect(toasts().slice(-1)[0]?.text).toContain("not a full path");
  });
});
