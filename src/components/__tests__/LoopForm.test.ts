import { describe, it, expect, vi } from "vitest";
import { mount, flushSync, tick } from "svelte";

vi.mock("../../lib/api", () => ({
  loops: {
    recipes: vi.fn(() =>
      Promise.resolve([
        {
          id: "maker-verifier",
          name: "Maker-Verifier",
          description: "Two-agent loop",
          source: "builtin",
          inputs: {
            goal: {
              required: true,
              input_type: "textarea",
              label: "Goal",
              description: "What should this loop accomplish?",
            },
            task_key: { required: false, input_type: "task", label: "Task" },
            base_branch: { required: false, input_type: "branch", label: "Base branch" },
          },
        },
        {
          id: "plan-implement-review",
          name: "Plan-Implement-Review",
          description: "Three-stage loop",
          source: "builtin",
          inputs: {
            goal: { required: true, input_type: "textarea", label: "Goal" },
            scope: { required: false, input_type: "text", label: "Scope", default: "full" },
          },
        },
      ]),
    ),
    create: vi.fn(() =>
      Promise.resolve({
        id: "loop-123",
        project_id: "p1",
        task_key: null,
        strategy: "maker-verifier",
        goal: "Fix the bug",
        status: "running",
        current_round: 0,
        max_rounds: 3,
        created_at: "2026-01-01T00:00:00Z",
        updated_at: "2026-01-01T00:00:00Z",
      }),
    ),
  },
  tasks: {
    list: vi.fn(() =>
      Promise.resolve([
        {
          key: "PLA-1",
          title: "Fix auth",
          status: "todo",
          description: "",
          priority: 0,
          blocked_by: [],
          tags: [],
          base_branch: "main",
        },
      ]),
    ),
  },
  projects: {
    listBranches: vi.fn(() => Promise.resolve(["main", "dev"])),
  },
}));

import LoopForm from "../LoopForm.svelte";

const baseProps = {
  projects: [{ id: "p1", name: "myproject", path: "/tmp/proj" }],
  onCreated: vi.fn(),
  onCancel: vi.fn(),
};

function renderForm(props = {}) {
  const target = document.createElement("div");
  mount(LoopForm, { target, props: { ...baseProps, ...props } });
  return target;
}

describe("LoopForm", () => {
  it("renders dynamic input fields from selected recipe", async () => {
    const target = renderForm();
    await tick();
    flushSync();
    // Wait for recipes to load
    await tick();
    flushSync();

    // The first recipe (maker-verifier) is auto-selected and has goal, task_key, base_branch inputs
    const goalField = target.querySelector("[data-field='input-goal']");
    expect(goalField).not.toBeNull();
    expect(goalField!.querySelector("textarea")).not.toBeNull();

    const taskField = target.querySelector("[data-field='input-task_key']");
    expect(taskField).not.toBeNull();

    const branchField = target.querySelector("[data-field='input-base_branch']");
    expect(branchField).not.toBeNull();
  });

  it("renders submit button", async () => {
    const target = renderForm();
    await tick();
    flushSync();

    const submitBtn = target.querySelector<HTMLButtonElement>("button[type='submit']");
    expect(submitBtn).not.toBeNull();
    expect(submitBtn!.textContent).toContain("Start loop");
  });

  it("submits with correct params including dynamic inputs", async () => {
    const { loops } = await import("../../lib/api");
    vi.mocked(loops.create).mockClear();
    const target = renderForm();
    await tick();
    flushSync();
    await tick();
    flushSync();

    // Fill in the goal textarea (dynamic input)
    const goalTextarea = target.querySelector<HTMLTextAreaElement>(
      "[data-field='input-goal'] textarea",
    )!;
    goalTextarea.value = "Fix the authentication bug";
    goalTextarea.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();

    // Submit the form
    const form = target.querySelector("form")!;
    form.dispatchEvent(new Event("submit", { bubbles: true }));
    await tick();
    flushSync();

    expect(loops.create).toHaveBeenCalledWith(
      expect.objectContaining({
        projectId: "p1",
        recipeId: "maker-verifier",
        inputs: expect.objectContaining({
          goal: "Fix the authentication bug",
        }),
        start: true,
      }),
    );
  });

  it("submits with start=false when draft checkbox is checked", async () => {
    const { loops } = await import("../../lib/api");
    vi.mocked(loops.create).mockClear();
    const target = renderForm();
    await tick();
    flushSync();
    await tick();
    flushSync();

    // Fill in the required goal
    const goalTextarea = target.querySelector<HTMLTextAreaElement>(
      "[data-field='input-goal'] textarea",
    )!;
    goalTextarea.value = "Draft loop";
    goalTextarea.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();

    // Check the draft checkbox
    const draftCheckbox = target.querySelector<HTMLInputElement>("input[data-field='draft']")!;
    draftCheckbox.checked = true;
    draftCheckbox.dispatchEvent(new Event("change", { bubbles: true }));
    flushSync();

    // Submit
    const form = target.querySelector("form")!;
    form.dispatchEvent(new Event("submit", { bubbles: true }));
    await tick();
    flushSync();

    expect(loops.create).toHaveBeenCalledWith(
      expect.objectContaining({
        start: false,
      }),
    );
  });

  it("calls onCreated with the new loop on success", async () => {
    baseProps.onCreated.mockClear();
    const target = renderForm();
    await tick();
    flushSync();
    await tick();
    flushSync();

    const goalTextarea = target.querySelector<HTMLTextAreaElement>(
      "[data-field='input-goal'] textarea",
    )!;
    goalTextarea.value = "Fix the bug";
    goalTextarea.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();

    const form = target.querySelector("form")!;
    form.dispatchEvent(new Event("submit", { bubbles: true }));
    await tick();
    flushSync();
    await tick();
    flushSync();

    expect(baseProps.onCreated).toHaveBeenCalledWith(expect.objectContaining({ id: "loop-123" }));
  });

  it("disables submit when required inputs are empty", async () => {
    const target = renderForm();
    await tick();
    flushSync();
    await tick();
    flushSync();

    const submitBtn = target.querySelector<HTMLButtonElement>("button[type='submit']")!;
    // Goal is required and empty by default, submit should be disabled
    expect(submitBtn.disabled).toBe(true);
  });

  it("enables submit when all required inputs have values", async () => {
    const target = renderForm();
    await tick();
    flushSync();
    await tick();
    flushSync();

    // Fill in goal (the only required field for maker-verifier)
    const goalTextarea = target.querySelector<HTMLTextAreaElement>(
      "[data-field='input-goal'] textarea",
    )!;
    goalTextarea.value = "Some goal";
    goalTextarea.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();

    const submitBtn = target.querySelector<HTMLButtonElement>("button[type='submit']")!;
    expect(submitBtn.disabled).toBe(false);
  });

  it("shows labels from recipe input definitions", async () => {
    const target = renderForm();
    await tick();
    flushSync();
    await tick();
    flushSync();

    // Check that the label "Goal" appears (from the recipe input def)
    const goalField = target.querySelector("[data-field='input-goal']");
    expect(goalField!.textContent).toContain("Goal");

    const taskField = target.querySelector("[data-field='input-task_key']");
    expect(taskField!.textContent).toContain("Task");
  });

  it("pre-fills task input when taskKey prop is provided", async () => {
    const target = renderForm({ taskKey: "PLA-1" });
    await tick();
    flushSync();
    await tick();
    flushSync();

    // The task_key input should be pre-filled with PLA-1
    // For a Select-based input, we check the underlying value via the input element
    const taskField = target.querySelector("[data-field='input-task_key']");
    expect(taskField).not.toBeNull();
    const taskInput = taskField!.querySelector<HTMLInputElement>("input");
    // The combobox input displays the label for the selected value
    expect(taskInput?.value).toContain("PLA-1");
  });

  it("renders text input for text-type fields with default value", async () => {
    const { loops } = await import("../../lib/api");
    // Switch to plan-implement-review recipe which has a "scope" text field with default "full"
    vi.mocked(loops.recipes).mockResolvedValueOnce([
      {
        id: "plan-implement-review",
        name: "Plan-Implement-Review",
        description: "Three-stage loop",
        source: "builtin",
        inputs: {
          goal: { required: true, input_type: "textarea", label: "Goal" },
          scope: { required: false, input_type: "text", label: "Scope", default: "full" },
        },
      },
    ]);

    const target = renderForm();
    await tick();
    flushSync();
    await tick();
    flushSync();

    const scopeField = target.querySelector("[data-field='input-scope']");
    expect(scopeField).not.toBeNull();
    const scopeInput = scopeField!.querySelector<HTMLInputElement>("input[type='text']");
    expect(scopeInput).not.toBeNull();
    expect(scopeInput!.value).toBe("full");
  });

  it("only includes non-empty inputs in submission", async () => {
    const { loops } = await import("../../lib/api");
    vi.mocked(loops.create).mockClear();
    const target = renderForm();
    await tick();
    flushSync();
    await tick();
    flushSync();

    // Fill only goal, leave task_key and base_branch empty
    const goalTextarea = target.querySelector<HTMLTextAreaElement>(
      "[data-field='input-goal'] textarea",
    )!;
    goalTextarea.value = "Just the goal";
    goalTextarea.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();

    const form = target.querySelector("form")!;
    form.dispatchEvent(new Event("submit", { bubbles: true }));
    await tick();
    flushSync();

    const callArgs = vi.mocked(loops.create).mock.calls[0][0];
    expect(callArgs.inputs).toEqual({ goal: "Just the goal" });
    // task_key and base_branch should not be present since they were empty
    expect(callArgs.inputs).not.toHaveProperty("task_key");
    expect(callArgs.inputs).not.toHaveProperty("base_branch");
  });
});
