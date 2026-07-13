import { describe, it, expect } from "vitest";
import { shouldHideProject } from "../sidebar-session-order";

describe("shouldHideProject", () => {
  it("hides when hideEmpty is true and project has zero orphans and zero visible tasks", () => {
    expect(shouldHideProject(0, 0, true)).toBe(true);
  });

  it("does not hide when hideEmpty is false", () => {
    expect(shouldHideProject(0, 0, false)).toBe(false);
  });

  it("does not hide when project has orphan sessions", () => {
    expect(shouldHideProject(2, 0, true)).toBe(false);
  });

  it("does not hide when project has visible tasks", () => {
    expect(shouldHideProject(0, 3, true)).toBe(false);
  });

  it("does not hide when project has both orphans and visible tasks", () => {
    expect(shouldHideProject(1, 2, true)).toBe(false);
  });

  it("does not hide when project has loops", () => {
    expect(shouldHideProject(0, 0, true, 1)).toBe(false);
  });

  it("hides when hideEmpty and no orphans, tasks, or loops", () => {
    expect(shouldHideProject(0, 0, true, 0)).toBe(true);
  });
});
