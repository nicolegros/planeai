import { describe, it, expect, vi } from "vitest";
import { mount, flushSync, tick } from "svelte";

vi.mock("../../lib/api", () => ({
  sessions: { launch: vi.fn(() => new Promise(() => {})) },
  projects: { listBranches: vi.fn(() => Promise.resolve([])) },
  tasks: {
    list: vi.fn(() =>
      Promise.resolve([
        { key: "PROJ-1", title: "Fix bug", description: "", status: "todo", priority: 0, base_branch: "main" },
      ]),
    ),
    listAll: vi.fn(() =>
      Promise.resolve([
        { key: "PROJ-1", title: "Fix bug", description: "", status: "todo", priority: 0, base_branch: "main" },
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
  projects: [{ id: "p1", name: "Project", path: "/tmp/proj" }],
  sessions: [],
  onCreated: vi.fn(),
  onCancel: vi.fn(),
};

function renderForm(props = {}) {
  const target = document.createElement("div");
  mount(SessionForm, { target, props: { ...baseProps, ...props } });
  return target;
}

describe("SessionForm", () => {
  it("defaults to manual mode when no taskPrefill is provided", () => {
    const target = renderForm();
    const buttons = target.querySelectorAll("[role='toolbar'] button");
    const manualBtn = Array.from(buttons).find((b) => b.textContent?.includes("Manual"));
    expect(manualBtn?.className).toContain("bg-accent");
  });

  it("defaults to task mode when taskPrefill is provided", () => {
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
    const buttons = target.querySelectorAll("[role='toolbar'] button");
    const taskBtn = Array.from(buttons).find((b) => b.textContent?.includes("From task"));
    expect(taskBtn?.className).toContain("bg-accent");
  });

  it("renders Manual button first and From task button second", () => {
    const target = renderForm();
    const buttons = target.querySelectorAll("[role='toolbar'] button");
    expect(buttons[0].textContent).toContain("Manual");
    expect(buttons[1].textContent).toContain("From task");
  });

  it("clears task-prefilled fields when switching from task to manual mode", async () => {
    const target = renderForm({
      taskPrefill: {
        key: "PROJ-1",
        title: "Fix bug",
        description: "A bug to fix",
        branch: "fix/bug",
        name: "PROJ-1: Fix bug",
        prompt: "Fix the bug",
      },
    });

    // Verify starts in task mode with prefilled name
    const nameInput = target.querySelector<HTMLInputElement>("input[placeholder='My session...']")!;
    expect(nameInput.value).toBe("PROJ-1: Fix bug");

    // Switch to manual mode by clicking the Manual button
    const manualBtn = target.querySelectorAll("[role='toolbar'] button")[0] as HTMLButtonElement;
    manualBtn.click();
    flushSync();

    // Name and branch fields should be cleared
    expect(nameInput.value).toBe("");
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
