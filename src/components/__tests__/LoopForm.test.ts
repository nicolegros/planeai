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
        },
        {
          id: "plan-implement-review",
          name: "Plan-Implement-Review",
          description: "Three-stage loop",
          source: "builtin",
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
}));

import LoopForm from "../LoopForm.svelte";

const baseProps = {
  projectId: "p1",
  projectPath: "/tmp/proj",
  onCreated: vi.fn(),
  onCancel: vi.fn(),
};

function renderForm(props = {}) {
  const target = document.createElement("div");
  mount(LoopForm, { target, props: { ...baseProps, ...props } });
  return target;
}

describe("LoopForm", () => {
  it("renders goal field and submit button", async () => {
    const target = renderForm();
    await tick();
    flushSync();

    const goalInput = target.querySelector<HTMLTextAreaElement>("textarea[data-field='goal']");
    expect(goalInput).not.toBeNull();

    const submitBtn = target.querySelector<HTMLButtonElement>("button[type='submit']");
    expect(submitBtn).not.toBeNull();
    expect(submitBtn!.textContent).toContain("Start loop");
  });

  it("submits with correct params when start is checked", async () => {
    const { loops } = await import("../../lib/api");
    const target = renderForm();
    await tick();
    flushSync();

    // Fill in the goal
    const goalInput = target.querySelector<HTMLTextAreaElement>("textarea[data-field='goal']")!;
    goalInput.value = "Fix the authentication bug";
    goalInput.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();

    // Submit the form
    const form = target.querySelector("form")!;
    form.dispatchEvent(new Event("submit", { bubbles: true }));
    await tick();
    flushSync();

    expect(loops.create).toHaveBeenCalledWith(
      expect.objectContaining({
        projectId: "p1",
        goal: "Fix the authentication bug",
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

    // Fill in the goal
    const goalInput = target.querySelector<HTMLTextAreaElement>("textarea[data-field='goal']")!;
    goalInput.value = "Draft loop";
    goalInput.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();

    // Check the draft checkbox — need change event for bind:checked
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
    const target = renderForm();
    await tick();
    flushSync();

    const goalInput = target.querySelector<HTMLTextAreaElement>("textarea[data-field='goal']")!;
    goalInput.value = "Fix the bug";
    goalInput.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();

    const form = target.querySelector("form")!;
    form.dispatchEvent(new Event("submit", { bubbles: true }));
    await tick();
    flushSync();
    // Allow promise to resolve
    await tick();
    flushSync();

    expect(baseProps.onCreated).toHaveBeenCalledWith(expect.objectContaining({ id: "loop-123" }));
  });

  it("disables submit when goal is empty", async () => {
    const target = renderForm();
    await tick();
    flushSync();

    const submitBtn = target.querySelector<HTMLButtonElement>("button[type='submit']")!;
    // Goal is empty by default, submit should be disabled
    expect(submitBtn.disabled).toBe(true);
  });
});
