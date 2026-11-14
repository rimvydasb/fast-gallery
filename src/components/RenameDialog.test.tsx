import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { render } from "solid-js/web";
import { apiMock, file, flush } from "../test/helpers";

const mocked = vi.hoisted(() => ({ api: {} as Record<string, ReturnType<typeof vi.fn>> }));
vi.mock("../lib/api", async (orig) => ({ ...(await orig<typeof import("../lib/api")>()), api: mocked.api }));

import { RenameDialog } from "./RenameDialog";
import { renameRequest, requestRename, setEditModeSignal, setEntries, setPath, setRenameRequest } from "../lib/store";
import { setToasts } from "../lib/toast";

let dispose: () => void;
let host: HTMLElement;

const input = () => host.querySelector<HTMLInputElement>("input.dialog-input")!;
const error = () => host.querySelector(".dialog-error")!.textContent;
const key = (k: string, init: KeyboardEventInit = {}) =>
  input().dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true, ...init }));
const type = (v: string) => {
  input().value = v;
  input().dispatchEvent(new InputEvent("input", { bubbles: true }));
};

beforeEach(async () => {
  Object.assign(mocked.api, apiMock());
  setToasts([]);
  setPath("/pics");
  setEntries([file("photo.jpg"), file("b.jpg")]);
  setEditModeSignal(true);
  host = document.body.appendChild(document.createElement("div"));
  dispose = render(() => <RenameDialog />, host);
  requestRename(file("photo.jpg"));
  await flush();
});

afterEach(() => {
  dispose();
  host.remove();
  setRenameRequest(null);
});

describe("RenameDialog", () => {
  it("shows nothing until a rename is requested", () => {
    setRenameRequest(null);
    expect(host.querySelector(".dialog")).toBeNull();
  });

  it("prefills the name and selects the part before the extension", () => {
    expect(host.querySelector("h2")!.textContent).toContain("photo.jpg");
    expect(input().value).toBe("photo.jpg");
    expect([input().selectionStart, input().selectionEnd]).toEqual([0, 5]);
    expect(document.activeElement).toBe(input());
  });

  it("closes on Escape and on Cancel without a backend call", async () => {
    key("Escape");
    expect(renameRequest()).toBeNull();
    requestRename(file("photo.jpg"));
    await flush();
    host.querySelector<HTMLButtonElement>("#rename-cancel")!.click();
    expect(renameRequest()).toBeNull();
    expect(mocked.api.renameFile).not.toHaveBeenCalled();
  });

  it("closes without a backend call if the name is unchanged", async () => {
    key("Enter");
    await flush();
    expect(renameRequest()).toBeNull();
    expect(mocked.api.renameFile).not.toHaveBeenCalled();
  });

  it("shows the reason for an invalid name and clears it when the user types", async () => {
    type("a/b.jpg");
    key("Enter");
    await flush();
    expect(error()).toContain("cannot contain");
    expect(input().getAttribute("aria-invalid")).toBe("true");
    expect(mocked.api.renameFile).not.toHaveBeenCalled();
    type("ab.jpg");
    expect(error()).toBe("");
    expect(input().getAttribute("aria-invalid")).toBe("false");
  });

  it("renames on Enter and closes", async () => {
    mocked.api.renameFile.mockResolvedValue("/pics/new.jpg");
    mocked.api.listDir.mockResolvedValue({ path: "/pics", entries: [file("b.jpg"), file("new.jpg")] });
    type("new.jpg");
    key("Enter");
    await flush();
    expect(mocked.api.renameFile).toHaveBeenCalledExactlyOnceWith("/pics/photo.jpg", "new.jpg");
    expect(renameRequest()).toBeNull();
  });

  it("does not submit on Enter while an IME composes", async () => {
    type("new.jpg");
    key("Enter", { isComposing: true });
    await flush();
    expect(mocked.api.renameFile).not.toHaveBeenCalled();
    expect(renameRequest()).toBe("/pics/photo.jpg");
  });

  it("keeps the dialog open and shows a backend error, such as a name collision", async () => {
    mocked.api.renameFile.mockRejectedValue({ kind: "other", message: "A file named “b.jpg” already exists." });
    type("b.jpg");
    host.querySelector<HTMLButtonElement>("#rename-confirm")!.click();
    await flush();
    expect(error()).toBe("A file named “b.jpg” already exists.");
    expect(renameRequest()).toBe("/pics/photo.jpg");
    expect(host.querySelector<HTMLButtonElement>("#rename-confirm")!.disabled).toBe(false);
  });

  it("sends one request if the user presses Enter twice during a slow rename", async () => {
    let finish!: (p: string) => void;
    mocked.api.renameFile.mockReturnValue(new Promise<string>((r) => (finish = r)));
    mocked.api.listDir.mockResolvedValue({ path: "/pics", entries: [file("new.jpg")] });
    type("new.jpg");
    key("Enter");
    key("Enter");
    await flush();
    expect(host.querySelector<HTMLButtonElement>("#rename-confirm")!.disabled).toBe(true);
    finish("/pics/new.jpg");
    await flush();
    expect(mocked.api.renameFile).toHaveBeenCalledTimes(1);
  });

  it("does not leak the Escape key to the window shortcuts", () => {
    const onWindow = vi.fn();
    window.addEventListener("keydown", onWindow);
    key("Escape");
    window.removeEventListener("keydown", onWindow);
    expect(onWindow).not.toHaveBeenCalled();
  });
});
