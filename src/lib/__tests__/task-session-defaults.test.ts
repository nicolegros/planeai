import { describe, expect, it } from "vitest";
import cases from "../../../src-tauri/src/task_start_cases.json";
import { taskSessionDefaults } from "../task-session-defaults";

// The backend planner runs the same cases, so the forms preview what it starts.
describe("taskSessionDefaults", () => {
  it.each(cases)("$case", ({ task, templates, expected }) => {
    expect(taskSessionDefaults(task, templates)).toEqual(expected);
  });
});
