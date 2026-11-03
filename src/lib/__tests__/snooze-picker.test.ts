import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Project, TaskItem } from "../types";

const { snoozeTask, wakeTask } = vi.hoisted(() => ({
  snoozeTask: vi.fn().mockResolvedValue(true),
  wakeTask: vi.fn().mockResolvedValue(true),
}));
vi.mock("../task-moves", () => ({ snoozeTask, wakeTask }));
vi.mock("../toasts.svelte", () => ({ showToast: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));

import {
  getCustomSnooze,
  getSnoozeMenu,
  closeSnoozeMenu,
  closeCustomSnooze,
  snoozeMenuEntry,
  snoozeMenuItems,
  snoozeOrWake,
} from "../snooze-picker.svelte";
import { showToast } from "../toasts.svelte";
import type { MenuItem } from "../../components/ui/ContextMenu.svelte";

const project: Project = { id: "p", name: "p", path: "/repo", hidden: false, path_missing: false };
const NOW = new Date(2026, 10, 3, 10, 0);

function task(overrides: Partial<TaskItem> = {}): TaskItem {
  return {
    key: "PLA-1",
    title: "Later",
    status: "in_progress",
    description: "",
    priority: 0,
    blocked_by: [],
    tags: [],
    parent_key: null,
    url: null,
    base_branch: "main",
    last_visited_at: null,
    last_activity_at: null,
    snoozed_until: null,
    snoozed_at: null,
    woke_at: null,
    ...overrides,
  };
}

const snoozed = task({
  snoozed_until: new Date(2026, 10, 4, 9, 0).toISOString(),
  snoozed_at: NOW.toISOString(),
});

const leaves = (items: MenuItem[]) =>
  items.filter((item): item is Extract<MenuItem, { onSelect: () => void }> => "onSelect" in item);

describe("snooze picker", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(NOW);
  });

  afterEach(() => {
    closeSnoozeMenu();
    closeCustomSnooze();
    vi.useRealTimers();
    vi.clearAllMocks();
  });

  it("lists the presets numbered 1-5, then Custom on C", () => {
    const items = leaves(snoozeMenuItems({ task: task(), project }, true));
    expect(items.map((item) => [item.label.replace(/ \(.*\)$/, ""), item.shortcut])).toEqual([
      ["In 1 hour", "1"],
      ["In 3 hours", "2"],
      ["This evening", "3"],
      ["Tomorrow", "4"],
      ["Next week", "5"],
      ["Custom…", "C"],
    ]);
  });

  it("resolves a preset at the moment of the click", () => {
    const [inOneHour] = leaves(snoozeMenuItems({ task: task(), project }, false));
    vi.setSystemTime(new Date(2026, 10, 3, 10, 30));

    inOneHour.onSelect();

    expect(snoozeTask.mock.calls).toEqual([[task(), project, new Date(2026, 10, 3, 11, 30)]]);
  });

  it("opens Custom in place of the menu", () => {
    const target = { task: task(), project };
    snoozeOrWake(target);
    leaves(snoozeMenuItems(target, true)).at(-1)!.onSelect();

    expect([getSnoozeMenu(), getCustomSnooze()]).toEqual([null, target]);
  });

  describe("giving focus back", () => {
    function focusedField(): HTMLTextAreaElement {
      const field = document.createElement("textarea");
      document.body.append(field);
      field.focus();
      return field;
    }

    function takeFocus(): void {
      const menuItem = document.createElement("button");
      document.body.append(menuItem);
      menuItem.focus();
      menuItem.remove();
    }

    afterEach(() => document.body.replaceChildren());

    it("returns focus to where it was once the menu closes", async () => {
      const field = focusedField();
      snoozeOrWake({ task: task(), project });
      takeFocus();

      closeSnoozeMenu();
      await vi.runAllTimersAsync();

      expect(document.activeElement).toBe(field);
    });

    it("waits for the Custom dialog it led to, then returns focus to where it was", async () => {
      const field = focusedField();
      const target = { task: task(), project };
      snoozeOrWake(target);
      takeFocus();
      leaves(snoozeMenuItems(target, true)).at(-1)!.onSelect();
      closeSnoozeMenu();
      await vi.runAllTimersAsync();
      takeFocus();

      closeCustomSnooze();
      await vi.runAllTimersAsync();

      expect(document.activeElement).toBe(field);
    });
  });

  it("opens the menu on an awake task and wakes a snoozed one instead", () => {
    snoozeOrWake({ task: task(), project });
    expect(getSnoozeMenu()?.target.task.key).toBe("PLA-1");
    closeSnoozeMenu();

    snoozeOrWake({ task: snoozed, project });

    expect(getSnoozeMenu()).toBe(null);
    expect(wakeTask.mock.calls).toEqual([[snoozed, project]]);
  });

  it("says why nothing happens without a task or on a Done task", () => {
    snoozeOrWake(null);
    snoozeOrWake({ task: task({ status: "done" }), project });

    expect(getSnoozeMenu()).toBe(null);
    expect(
      vi.mocked(showToast).mock.calls.map(([options]) => (options as { message: string }).message),
    ).toEqual(["No task is open.", "Done tasks cannot be snoozed."]);
  });

  it("offers Snooze on the task menu, Wake on a snoozed task, and nothing on a Done one", () => {
    const entry = (t: TaskItem) => {
      const item = snoozeMenuEntry({ task: t, project });
      if (!item || "separator" in item) return item;
      return "children" in item ? [item.label, item.children.length] : [item.label];
    };
    expect([entry(task()), entry(snoozed), entry(task({ status: "done" }))]).toEqual([
      ["Snooze", 7],
      ["Wake"],
      null,
    ]);
  });
});
