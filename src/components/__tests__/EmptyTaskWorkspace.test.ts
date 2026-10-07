import { afterEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import EmptyTaskWorkspace from "../EmptyTaskWorkspace.svelte";
import type { Session, TaskItem } from "../../lib/types";

const task: TaskItem = {
  key: "PLA-315",
  title: "Keep workspaces after agents close",
  description: "Task context remains available.",
  status: "in_progress",
  priority: 1,
  parent_key: null,
  blocked_by: [],
  tags: [],
  url: null,
  base_branch: "main",
};

const archived = {
  id: "archived-agent",
  name: "Archived agent",
  branch: "feat/task-workspace",
} as Session;

const mounted: Array<ReturnType<typeof mount>> = [];
afterEach(() => {
  for (const component of mounted.splice(0)) unmount(component);
  document.body.replaceChildren();
});

function render(archivedSessions: Session[] = [], shown: TaskItem = task) {
  const target = document.body.appendChild(document.createElement("div"));
  const onNewSession = vi.fn();
  const onRestore = vi.fn();
  const onEdit = vi.fn();
  mounted.push(
    mount(EmptyTaskWorkspace, {
      target,
      props: { task: shown, archivedSessions, onNewSession, onRestore, onEdit },
    }),
  );
  return { target, onNewSession, onRestore, onEdit };
}

describe("EmptyTaskWorkspace", () => {
  it("makes restoring the first archived agent the primary action", () => {
    const { target } = render([archived]);

    expect(target.textContent).toContain("PLA-315");
    expect(target.textContent).toContain(task.title);
    expect(target.querySelector("button")?.textContent).toContain("Restore Archived agent");
  });

  it("labels priorities on the 0-5 scale plugins such as Jira write", () => {
    const label = (priority: number) =>
      render([], { ...task, priority }).target.querySelectorAll("dd")[1]?.textContent;

    expect([-1, 0, 1, 2, 3, 4, 5, 9].map(label)).toEqual([
      "None",
      "None",
      "Lowest",
      "Low",
      "Medium",
      "High",
      "Highest",
      "Highest",
    ]);
  });

  it("routes N, R, and E shortcuts while active", () => {
    const { onNewSession, onRestore, onEdit } = render([archived]);

    window.dispatchEvent(new KeyboardEvent("keydown", { key: "n", bubbles: true }));
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "r", bubbles: true }));
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "e", bubbles: true }));

    expect(onNewSession).toHaveBeenCalledOnce();
    expect(onRestore).toHaveBeenCalledWith(archived);
    expect(onEdit).toHaveBeenCalledOnce();
  });

  it("renders the description as markdown", () => {
    const { target } = render([], { ...task, description: "Ship **bold** changes." });

    expect(target.querySelector("[data-markdown-view] strong")?.textContent).toBe("bold");
  });

  it("ignores workspace shortcuts while a text input is focused", () => {
    const { onNewSession } = render();
    const input = document.body.appendChild(document.createElement("input"));
    input.focus();

    input.dispatchEvent(new KeyboardEvent("keydown", { key: "n", bubbles: true }));

    expect(onNewSession).not.toHaveBeenCalled();
  });
});
