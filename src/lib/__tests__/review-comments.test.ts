import { describe, it, expect, beforeEach } from "vitest";
import {
  addComment,
  removeComment,
  editComment,
  getComments,
  getFileCommentCount,
  getTotalCommentCount,
  clearComments,
  _resetForTests,
} from "../review-comments.svelte";

describe("review-comments", () => {
  beforeEach(() => {
    _resetForTests();
  });

  it("starts empty for any session", () => {
    expect(getComments("s1")).toEqual([]);
    expect(getTotalCommentCount("s1")).toBe(0);
  });

  it("addComment creates a comment with id and createdAt", () => {
    const c = addComment("s1", {
      filePath: "a.ts",
      type: "line",
      startLine: 5,
      endLine: 5,
      text: "fix this",
    });
    expect(c.id).toBeTruthy();
    expect(c.createdAt).toBeGreaterThan(0);
    expect(c.filePath).toBe("a.ts");
    expect(c.text).toBe("fix this");
    expect(getComments("s1")).toEqual([c]);
  });

  it("addComment appends multiple comments", () => {
    addComment("s1", { filePath: "a.ts", type: "line", startLine: 1, endLine: 1, text: "first" });
    addComment("s1", { filePath: "a.ts", type: "line", startLine: 2, endLine: 2, text: "second" });
    expect(getTotalCommentCount("s1")).toBe(2);
  });

  it("removeComment removes by id", () => {
    const c1 = addComment("s1", {
      filePath: "a.ts",
      type: "line",
      startLine: 1,
      endLine: 1,
      text: "one",
    });
    addComment("s1", { filePath: "a.ts", type: "line", startLine: 2, endLine: 2, text: "two" });
    removeComment("s1", c1.id);
    expect(getTotalCommentCount("s1")).toBe(1);
    expect(getComments("s1")[0].text).toBe("two");
  });

  it("removeComment is no-op for unknown id", () => {
    addComment("s1", { filePath: "a.ts", type: "line", startLine: 1, endLine: 1, text: "keep" });
    removeComment("s1", "nonexistent");
    expect(getTotalCommentCount("s1")).toBe(1);
  });

  it("getFileCommentCount counts per file", () => {
    addComment("s1", { filePath: "a.ts", type: "line", startLine: 1, endLine: 1, text: "a" });
    addComment("s1", { filePath: "b.ts", type: "line", startLine: 1, endLine: 1, text: "b" });
    addComment("s1", { filePath: "a.ts", type: "hunk", startLine: 5, endLine: 10, text: "c" });
    expect(getFileCommentCount("s1", "a.ts")).toBe(2);
    expect(getFileCommentCount("s1", "b.ts")).toBe(1);
  });

  it("clearComments removes all for a session", () => {
    addComment("s1", { filePath: "a.ts", type: "line", startLine: 1, endLine: 1, text: "a" });
    addComment("s1", { filePath: "b.ts", type: "file", startLine: 0, endLine: 0, text: "b" });
    clearComments("s1");
    expect(getComments("s1")).toEqual([]);
    expect(getTotalCommentCount("s1")).toBe(0);
  });

  it("sessions are independent", () => {
    addComment("s1", { filePath: "a.ts", type: "line", startLine: 1, endLine: 1, text: "s1" });
    addComment("s2", { filePath: "a.ts", type: "line", startLine: 1, endLine: 1, text: "s2" });
    clearComments("s1");
    expect(getTotalCommentCount("s1")).toBe(0);
    expect(getTotalCommentCount("s2")).toBe(1);
  });

  it("supports file-level comments (startLine=0, endLine=0)", () => {
    const c = addComment("s1", {
      filePath: "a.ts",
      type: "file",
      startLine: 0,
      endLine: 0,
      text: "general",
    });
    expect(c.type).toBe("file");
    expect(c.startLine).toBe(0);
  });

  it("supports hunk-level comments with range", () => {
    const c = addComment("s1", {
      filePath: "a.ts",
      type: "hunk",
      startLine: 10,
      endLine: 25,
      text: "hunk note",
    });
    expect(c.type).toBe("hunk");
    expect(c.startLine).toBe(10);
    expect(c.endLine).toBe(25);
  });

  it("editComment updates the text of an existing comment", () => {
    const c = addComment("s1", { filePath: "a.ts", type: "line", startLine: 1, endLine: 1, text: "original" });
    editComment("s1", c.id, "updated");
    expect(getComments("s1")[0].text).toBe("updated");
  });

  it("editComment preserves other fields", () => {
    const c = addComment("s1", { filePath: "a.ts", type: "hunk", startLine: 5, endLine: 10, text: "old" });
    editComment("s1", c.id, "new");
    const edited = getComments("s1")[0];
    expect(edited.id).toBe(c.id);
    expect(edited.filePath).toBe("a.ts");
    expect(edited.type).toBe("hunk");
    expect(edited.startLine).toBe(5);
    expect(edited.endLine).toBe(10);
    expect(edited.createdAt).toBe(c.createdAt);
  });

  it("editComment is no-op for unknown session or id", () => {
    addComment("s1", { filePath: "a.ts", type: "line", startLine: 1, endLine: 1, text: "keep" });
    editComment("s1", "nonexistent", "changed");
    editComment("s2", "any", "changed");
    expect(getComments("s1")[0].text).toBe("keep");
  });
});
