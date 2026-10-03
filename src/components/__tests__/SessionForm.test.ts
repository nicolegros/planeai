import { describe, it, expect, vi } from "vitest";
import { mount, flushSync, tick } from "svelte";

vi.mock("../../lib/api", () => ({
  sessions: { launch: vi.fn(() => new Promise(() => {})) },
  projects: {
    listBranches: vi.fn(() => Promise.resolve([])),
    detectDefaultBranch: vi.fn(() => Promise.resolve("main")),
  },
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
    target
      .querySelector<HTMLElement>("[data-form-keyboard]")!
      .dispatchEvent(new KeyboardEvent("keydown", { key: "m", bubbles: true, cancelable: true }));
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
    target
      .querySelector<HTMLElement>("[data-form-keyboard]")!
      .dispatchEvent(new KeyboardEvent("keydown", { key: "t", bubbles: true, cancelable: true }));
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

it("numbers the name of an additional task agent while defaulting it to an isolated worktree", async () => {
  const target = renderForm({
    sessions: [
      {
        id: "existing",
        project_id: "p1",
        name: "Agent 1",
        branch: "proj-1/fix-bug",
        status: "active",
        task_key: "PROJ-1",
        worktree_path: "/tmp/worktree",
      },
    ],
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

  expect(target.querySelector<HTMLInputElement>("input[placeholder='My session...']")?.value).toBe(
    "Fix bug (2)",
  );
  expect(target.querySelector<HTMLInputElement>("[data-field='branch'] input")?.value).toBe(
    "proj-1/fix-bug--2",
  );
  expect(target.querySelector<HTMLInputElement>("#use-worktree")?.checked).toBe(true);
});

it("keeps a name typed before the task list finishes loading", async () => {
  const target = renderForm({
    taskPrefill: {
      key: "PROJ-1",
      title: "Fix bug",
      description: "",
      branch: "",
      name: "Fix bug",
      prompt: "",
    },
  });
  const nameInput = target.querySelector<HTMLInputElement>("input[placeholder='My session...']")!;
  nameInput.value = "Reviewer agent";
  nameInput.dispatchEvent(new Event("input", { bubbles: true }));

  await tick();
  flushSync();
  await tick();
  flushSync();

  expect(nameInput.value).toBe("Reviewer agent");
});

it("resets a typed name when another task is picked", async () => {
  const { tasks } = await import("../../lib/api");
  vi.mocked(tasks.list).mockResolvedValueOnce([
    {
      key: "PROJ-1",
      title: "Fix bug",
      description: "",
      status: "todo",
      priority: 0,
      base_branch: "main",
    },
    {
      key: "PROJ-2",
      title: "Add feature",
      description: "",
      status: "todo",
      priority: 0,
      base_branch: "main",
    },
  ] as never);
  const target = renderForm();
  document.body.append(target);
  await tick();
  flushSync();
  await tick();

  const nameInput = target.querySelector<HTMLInputElement>("input[placeholder='My session...']")!;
  nameInput.value = "Reviewer agent";
  nameInput.dispatchEvent(new Event("input", { bubbles: true }));

  const taskInput = target.querySelector<HTMLInputElement>("[data-field='task'] input")!;
  taskInput.focus();
  taskInput.value = "Add feature";
  taskInput.dispatchEvent(new Event("input", { bubbles: true }));
  await tick();
  flushSync();
  const option = await vi.waitFor(() => {
    const found = [...document.querySelectorAll<HTMLElement>("[role='option']")].find((el) =>
      el.textContent?.includes("PROJ-2"),
    );
    expect(found).toBeTruthy();
    return found!;
  });
  option.dispatchEvent(new PointerEvent("pointerup", { bubbles: true }));
  option.click();
  await tick();
  flushSync();

  expect(nameInput.value).toBe("Add feature");
  target.remove();
});

describe("cross-project task link", () => {
  const twoProjects = [
    { id: "p1", name: "Project", path: "/tmp/proj", hidden: false },
    { id: "p2", name: "Other", path: "/tmp/other", hidden: false },
  ];
  const prefill = {
    key: "PROJ-1",
    title: "Fix bug",
    description: "",
    branch: "",
    name: "Fix bug",
    prompt: "",
    baseBranch: "release/2.x",
    projectId: "p1",
  };

  async function settle() {
    for (let i = 0; i < 4; i++) {
      await tick();
      flushSync();
    }
  }

  async function pickOption(target: HTMLElement, field: string, text: string) {
    const input = target.querySelector<HTMLInputElement>(`[data-field='${field}'] input`)!;
    input.focus();
    input.value = text;
    input.dispatchEvent(new Event("input", { bubbles: true }));
    await tick();
    flushSync();
    const option = await vi.waitFor(() => {
      const found = [...document.querySelectorAll<HTMLElement>("[role='option']")].find((el) =>
        el.textContent?.includes(text),
      );
      expect(found).toBeTruthy();
      return found!;
    });
    option.dispatchEvent(new PointerEvent("pointerup", { bubbles: true }));
    option.click();
    await settle();
  }

  async function renderSwitchedToOther(props = {}) {
    const { projects, tasks } = await import("../../lib/api");
    vi.mocked(projects.listBranches).mockImplementation((path: string) =>
      Promise.resolve(path === "/tmp/other" ? ["main"] : ["main", "release/2.x"]),
    );
    vi.mocked(tasks.listAll).mockImplementation(
      (path: string) =>
        Promise.resolve(
          path === "/tmp/other"
            ? [
                {
                  key: "OTH-1",
                  title: "Other task",
                  description: "",
                  status: "todo",
                  priority: 0,
                  base_branch: "main",
                },
              ]
            : [
                {
                  key: "PROJ-1",
                  title: "Fix bug",
                  description: "",
                  status: "todo",
                  priority: 0,
                  base_branch: "release/2.x",
                },
              ],
        ) as never,
    );
    const target = document.createElement("div");
    document.body.append(target);
    mount(SessionForm, {
      target,
      props: {
        ...baseProps,
        projects: twoProjects,
        currentProjectId: "p1",
        taskPrefill: prefill,
        ...props,
      },
    });
    await settle();
    await pickOption(target, "project", "Other");
    return target;
  }

  it("keeps the task linked and launches with the task's project", async () => {
    const { sessions } = await import("../../lib/api");
    vi.mocked(sessions.launch).mockClear();
    const target = await renderSwitchedToOther();

    expect(target.querySelector<HTMLInputElement>("[data-field='task'] input")?.value).toBe(
      "PROJ-1: Fix bug · Project",
    );
    expect(target.querySelector("[data-field='task']")?.textContent).toContain(
      "Linked to task in Project",
    );

    target.querySelector("form")!.dispatchEvent(new Event("submit", { bubbles: true }));
    await tick();
    expect(sessions.launch).toHaveBeenCalledWith(
      expect.objectContaining({
        projectId: "p2",
        repoPath: "/tmp/other",
        taskKey: "PROJ-1",
        taskProjectId: "p1",
      }),
    );
    target.remove();
  });

  it("relinks to a task of the new project when one is picked", async () => {
    const { sessions } = await import("../../lib/api");
    vi.mocked(sessions.launch).mockClear();
    const target = await renderSwitchedToOther();
    await pickOption(target, "task", "OTH-1");

    expect(target.querySelector("[data-field='task']")?.textContent).not.toContain(
      "Linked to task in",
    );
    target.querySelector("form")!.dispatchEvent(new Event("submit", { bubbles: true }));
    await tick();
    expect(sessions.launch).toHaveBeenCalledWith(
      expect.objectContaining({ projectId: "p2", taskKey: "OTH-1", taskProjectId: null }),
    );
    target.remove();
  });

  it("falls back to the other repo's default branch when it lacks the task's base", async () => {
    const target = await renderSwitchedToOther();

    const base = target.querySelector("[data-field='base']")!;
    expect(base.querySelector("input")!.value).toBe("main");
    expect(target.textContent).toContain("Task base release/2.x not found in Other, using main");
    target.remove();
  });

  it("asks for a base branch when the other repo has neither the task's base nor a detectable default", async () => {
    const { projects, sessions } = await import("../../lib/api");
    vi.mocked(sessions.launch).mockClear();
    vi.mocked(projects.detectDefaultBranch).mockRejectedValue("could not detect default branch");
    const target = await renderSwitchedToOther();

    expect(target.querySelector<HTMLInputElement>("[data-field='base'] input")!.value).toBe("");
    expect(target.textContent).toContain(
      "Task base release/2.x not found in Other, pick a base branch",
    );
    target.querySelector("form")!.dispatchEvent(new Event("submit", { bubbles: true }));
    await tick();
    flushSync();
    expect(target.textContent).toContain("Select a base branch.");
    expect(sessions.launch).not.toHaveBeenCalled();
    vi.mocked(projects.detectDefaultBranch).mockResolvedValue("main");
    target.remove();
  });

  it("keeps the prefilled task linked when the project changes before its tasks load", async () => {
    const { tasks } = await import("../../lib/api");
    let releaseOwnerTasks!: () => void;
    vi.mocked(tasks.listAll).mockImplementation((path: string) =>
      path === "/tmp/other"
        ? (Promise.resolve([]) as never)
        : (new Promise((resolve) => {
            releaseOwnerTasks = () =>
              resolve([
                {
                  key: "PROJ-1",
                  title: "Fix bug",
                  description: "",
                  status: "todo",
                  priority: 0,
                  base_branch: "main",
                },
              ] as never);
          }) as never),
    );
    const target = document.createElement("div");
    document.body.append(target);
    mount(SessionForm, {
      target,
      props: { ...baseProps, projects: twoProjects, currentProjectId: "p1", taskPrefill: prefill },
    });
    await settle();
    await pickOption(target, "project", "Other");
    releaseOwnerTasks();
    await settle();

    expect(target.querySelector("[data-field='task']")?.textContent).toContain(
      "Linked to task in Project",
    );
    target.remove();
  });

  it("numbers the name across the task but the branch only within the repo", async () => {
    const target = await renderSwitchedToOther({
      sessions: [
        {
          id: "existing",
          project_id: "p1",
          task_project_id: null,
          name: "Fix bug",
          branch: "proj-1/fix-bug",
          status: "active",
          task_key: "PROJ-1",
          worktree_path: null,
        },
      ],
    });

    expect(
      target.querySelector<HTMLInputElement>("input[placeholder='My session...']")?.value,
    ).toBe("Fix bug (2)");
    expect(target.querySelector<HTMLInputElement>("[data-field='branch'] input")?.value).toBe(
      "proj-1/fix-bug",
    );
    expect(target.querySelector<HTMLInputElement>("#use-worktree")?.checked).toBe(false);
    target.remove();
  });
});

describe("SessionForm with a provider that cannot auto-approve", () => {
  it("shows auto-approve off and ignores its shortcut, keeping the choice for other providers", () => {
    const target = renderForm({
      runtimeProviders: [
        {
          key: "chat:claude",
          plugin: { id: "chat", name: "Chat" },
          provider: {
            id: "claude",
            label: "Claude (chat)",
            entrypoint: "ui/chat.js",
            supports: [],
          },
        },
      ],
    });
    const form = target.querySelector<HTMLElement>("[data-form-keyboard]")!;
    const press = (key: string) => {
      form.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true }));
      flushSync();
    };
    const checkbox = () => target.querySelector<HTMLInputElement>("#auto-approve")!;
    expect(checkbox().checked).toBe(true);
    press("p");
    expect(checkbox().disabled).toBe(true);
    expect(checkbox().checked).toBe(false);
    press("a");
    press("p");
    expect(checkbox().checked).toBe(true);
  });
});
