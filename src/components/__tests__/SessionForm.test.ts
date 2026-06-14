import { describe, it, expect, vi } from "vitest";
import { mount } from "svelte";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(() => Promise.resolve([])),
}));

vi.mock("../../lib/settings.svelte", () => ({
  getSettings: () => ({
    providers: { claude: { command: "claude", yolo_flag: null } },
    default_provider: "claude",
    task_managers: { jira: {} },
    default_task_manager: "jira",
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
    expect(manualBtn?.className).toContain("bg-primary-500");
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
    expect(taskBtn?.className).toContain("bg-primary-500");
  });

  it("renders Manual button first and From task button second", () => {
    const target = renderForm();
    const buttons = target.querySelectorAll("[role='toolbar'] button");
    expect(buttons[0].textContent).toContain("Manual");
    expect(buttons[1].textContent).toContain("From task");
  });
});
