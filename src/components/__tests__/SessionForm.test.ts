import { describe, it, expect, vi } from "vitest";
import { mount, flushSync, tick } from "svelte";

vi.mock("../../lib/api", () => ({
  sessions: { launch: vi.fn(() => new Promise(() => {})) },
  projects: { listBranches: vi.fn(() => Promise.resolve([])) },
  tasks: {
    list: vi.fn(() =>
      Promise.resolve([
        {
          key: "PROJ-1",
          title: "Fix bug",
          description: "",
          status: "todo",
          priority: 0,
          base_branch: "main",
        },
      ]),
    ),
    listAll: vi.fn(() =>
      Promise.resolve([
        {
          key: "PROJ-1",
          title: "Fix bug",
          description: "",
          status: "todo",
          priority: 0,
          base_branch: "main",
        },
      ]),
    ),
  },
}));

vi.mock("../../lib/settings.svelte", () => ({
  getSettings: () => ({
    providers: { claude: { command: "claude", yolo_flag: null } },
    default_provider: "claude",
    task_management: {},
  }),
}));

import SessionForm from "../SessionForm.svelte";

const baseProps = {
  projects: [{ id: "p1", name: "Project", path: "/tmp/proj", hidden: false }],
  sessions: [],
  onCreated: vi.fn(),
  onCancel: vi.fn(),
  onCreateTask: vi.fn(),
};

function renderForm(props = {}) {
  const target = document.createElement("div");
  mount(SessionForm, { target, props: { ...baseProps, ...props } });
  return target;
}

describe("SessionForm", () => {
  it("always renders task selection and a New task route", () => {
    const target = renderForm();
    expect(target.querySelector("[data-field='task']")).not.toBeNull();
    expect(target.querySelector("[role='toolbar'] button")?.textContent).toContain("New task");
  });

  it("routes the New task button to the task form", () => {
    const onCreateTask = vi.fn();
    const target = renderForm({ onCreateTask });
    (target.querySelector("[role='toolbar'] button") as HTMLButtonElement).click();
    expect(onCreateTask).toHaveBeenCalledOnce();
  });

  it("keeps the task picker available when task-prefilled", () => {
    const target = renderForm({
      taskPrefill: {
        key: "PROJ-1",
        title: "Fix bug",
        description: "",
        branch: "fix/bug",
        name: "Fix bug",
        prompt: "Fix it",
      },
    });
    expect(target.querySelector("[data-field='task']")).not.toBeNull();
  });

  it("uses M to route to New task in normal mode", () => {
    const onCreateTask = vi.fn();
    const target = renderForm({ onCreateTask });
    target.querySelector<HTMLElement>("[data-form-keyboard]")!.dispatchEvent(
      new KeyboardEvent("keydown", { key: "m", bubbles: true, cancelable: true }),
    );
    expect(onCreateTask).toHaveBeenCalledOnce();
  });

  it("does not render a manual launch mode", () => {
    const target = renderForm();
    expect(target.textContent).not.toContain("Manual");
  });

  it("sets base branch from task's base_branch when task is selected", async () => {
    const target = renderForm({
      taskPrefill: {
        key: "PROJ-1",
        title: "Fix bug",
        description: "",
        branch: "fix/bug",
        name: "PROJ-1: Fix bug",
        prompt: "Fix it",
        baseBranch: "main",
      },
    });

    // Wait for async task loading effect to fire and call onTaskSelected
    await tick();
    flushSync();
    await tick();
    flushSync();

    // The base branch field should be rendered (isNewBranch is true since branches list is empty)
    const baseField = target.querySelector("[data-field='base']");
    expect(baseField).not.toBeNull();
    // The select input should contain the task's base_branch value
    const baseInput = baseField!.querySelector("input");
    expect(baseInput).not.toBeNull();
    expect(baseInput!.value).toBe("main");
  });

  it("focuses the task picker with T in normal mode", async () => {
    const target = renderForm();
    document.body.append(target);
    await tick();
    flushSync();
    const taskInput = target.querySelector<HTMLInputElement>("[data-field='task'] input")!;
    target.querySelector<HTMLElement>("[data-form-keyboard]")!.dispatchEvent(
      new KeyboardEvent("keydown", { key: "t", bubbles: true, cancelable: true }),
    );
    expect(document.activeElement).toBe(taskInput);
  });

  it("submit button is not disabled initially", () => {
    const target = renderForm();
    const submitBtn = target.querySelector<HTMLButtonElement>("button[type='submit']")!;
    expect(submitBtn.disabled).toBe(false);
  });

  it("submit button becomes disabled with spinner during submission", async () => {
    const target = renderForm({
      taskPrefill: {
        key: "PROJ-1",
        title: "Fix bug",
        description: "",
        branch: "feat/test",
        name: "Fix bug",
        prompt: "Fix it",
      },
    });

    // Wait for async task loading effect to fire and populate branch
    await tick();
    flushSync();
    await tick();
    flushSync();

    const submitBtn = target.querySelector<HTMLButtonElement>("button[type='submit']")!;

    // Trigger submit via form submission
    const form = target.querySelector("form")!;
    form.dispatchEvent(new Event("submit", { bubbles: true }));
    await tick();
    flushSync();

    // Button should now be disabled and contain a spinner (svg from LoaderCircle)
    expect(submitBtn.disabled).toBe(true);
    expect(submitBtn.querySelector("svg")).not.toBeNull();
  });
});

  it("numbers an additional task agent and defaults it to an isolated worktree", async () => {
    const target = renderForm({
      sessions: [{
        id: "existing",
        project_id: "p1",
        name: "Agent 1",
        branch: "proj-1/fix-bug",
        status: "active",
        task_key: "PROJ-1",
        worktree_path: "/tmp/worktree",
      }],
      taskPrefill: {
        key: "PROJ-1",
        title: "Fix bug",
        description: "",
        branch: "",
        name: "",
        prompt: "",
      },
    });

    await tick();
    flushSync();
    await tick();
    flushSync();

    expect(target.querySelector<HTMLInputElement>("input[placeholder='My session...']")?.value).toBe("Agent 2");
    expect(target.querySelector<HTMLInputElement>("[data-field='branch'] input")?.value).toBe("proj-1/fix-bug--2");
    expect(target.querySelector<HTMLInputElement>("#use-worktree")?.checked).toBe(true);
  });
