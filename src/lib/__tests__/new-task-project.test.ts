import { describe, expect, it } from "vitest";
import { newTaskProjectPath } from "../new-task-project";
import type { Project } from "../types";

const project = (id: string): Project => ({
  id,
  name: id,
  path: `/repos/${id}`,
  hidden: false,
  path_missing: false,
});
const projects = [project("alpha"), project("beta"), project("gamma")];

describe("newTaskProjectPath", () => {
  it("prefers the focused task's project", () => {
    expect(newTaskProjectPath(project("gamma"), "beta", projects)).toBe("/repos/gamma");
  });

  it("falls back to the active session's project", () => {
    expect(newTaskProjectPath(null, "beta", projects)).toBe("/repos/beta");
  });

  it("falls back to the first project when the session's project is unknown", () => {
    expect(newTaskProjectPath(undefined, "deleted", projects)).toBe("/repos/alpha");
  });

  it("is empty without projects", () => {
    expect(newTaskProjectPath(null, null, [])).toBe("");
  });
});
