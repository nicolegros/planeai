import { describe, it, expect } from "vitest";
import { nameFromPath } from "../name-from-path";

describe("nameFromPath", () => {
  it("returns last segment of a Unix path", () => {
    expect(nameFromPath("/Users/me/my-app")).toBe("my-app");
  });

  it("returns last segment of a Windows path", () => {
    expect(nameFromPath("C:\\Users\\me\\my-app")).toBe("my-app");
  });

  it("ignores trailing separator", () => {
    expect(nameFromPath("/Users/me/my-app/")).toBe("my-app");
  });

  it("returns empty string for empty input", () => {
    expect(nameFromPath("")).toBe("");
  });
});
