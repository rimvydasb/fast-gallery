import { describe, expect, it } from "vitest";
import { stemLength, validateFileName } from "./rename";

describe("validateFileName", () => {
  it.each(["a.jpg", "IMG 0001.HEIC", "žąsis – 2024.jpg", "no-extension", ".hidden", "a.b.c.png"])("accepts %j", (n) => {
    expect(validateFileName(n)).toBeNull();
  });

  it.each([
    ["", "empty"],
    ["   ", "empty"],
    [".", "not a valid"],
    ["..", "not a valid"],
    ["a/b.jpg", "cannot contain"],
    ["a:b.jpg", "cannot contain"],
    ["a\0b.jpg", "cannot contain"],
  ])("rejects %j", (n, reason) => {
    expect(validateFileName(n)).toContain(reason);
  });

  it("limits the name to 255 bytes, not characters", () => {
    expect(validateFileName("x".repeat(255))).toBeNull();
    expect(validateFileName("x".repeat(256))).toBe("The name is too long.");
    expect(validateFileName("ž".repeat(128))).toBe("The name is too long."); // 256 bytes
    expect(validateFileName("ž".repeat(127))).toBeNull(); // 254 bytes
  });
});

describe("stemLength", () => {
  it("excludes the extension", () => {
    expect(stemLength("photo.jpg")).toBe(5);
    expect(stemLength("a.b.jpeg")).toBe(3);
  });
  it("selects the whole name if there is no extension, or the dot starts the name", () => {
    expect(stemLength("photo")).toBe(5);
    expect(stemLength(".hidden")).toBe(7);
  });
});
