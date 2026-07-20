import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { mount, unmount, tick } from "svelte";

vi.mock("../../lib/api", () => ({
  jira: { assign: vi.fn(() => Promise.resolve()) },
}));

vi.mock("../../lib/keyboard", () => ({
  isPlatformMod: () => false,
  MOD_ENTER_HINT: "⌘↵",
}));

vi.mock("../../lib/jira-task-store.svelte", () => ({
  loadJiraTasks: vi.fn(() => Promise.resolve()),
}));

vi.mock("../../lib/task-store.svelte", () => ({
  refresh: vi.fn(() => Promise.resolve()),
}));

import AssignJiraDialog from "../AssignJiraDialog.svelte";
import type { TaskItem, Project } from "../../lib/types";

function makeTask(overrides: Partial<TaskItem> = {}): TaskItem {
  return {
    key: "PLA-42",
    title: "Fix login redirect loop",
    status: "todo",
    description:
      "When a user logs in via SSO they get redirected back to the login page in an infinite loop.",
    priority: 1,
    blocked_by: [],
    tags: [],
    parent_key: null,
    url: null,
    base_branch: "main",
    ...overrides,
  };
}

const projects: Project[] = [{ id: "p1", name: "myapp", path: "/tmp/myapp" }];

let component: Record<string, unknown> | null = null;
let mountTarget: HTMLElement | null = null;

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  if (component) {
    unmount(component);
    component = null;
  }
  if (mountTarget) {
    mountTarget.remove();
    mountTarget = null;
  }
  vi.runOnlyPendingTimers();
  vi.useRealTimers();
});

async function renderDialog(task: TaskItem = makeTask()) {
  mountTarget = document.createElement("div");
  document.body.appendChild(mountTarget);
  component = mount(AssignJiraDialog, {
    target: mountTarget,
    props: {
      task,
      projects,
      onClose: vi.fn(),
      onNewProject: vi.fn(),
    },
  });
  await tick();
  // Dialog uses a portal — content is rendered into document.body
  return document.body;
}

describe("AssignJiraDialog", () => {
  it("displays the task key and title", async () => {
    const target = await renderDialog();
    expect(target.textContent).toContain("PLA-42");
    expect(target.textContent).toContain("Fix login redirect loop");
  });

  it("displays the description", async () => {
    const target = await renderDialog();
    expect(target.textContent).toContain("When a user logs in via SSO");
  });

  it("does not show description section when description is empty", async () => {
    const target = await renderDialog(makeTask({ description: "" }));
    const descSection = target.querySelector("[data-testid='task-description']");
    expect(descSection).toBeNull();
  });
});
