import { describe, expect, it, vi } from "vitest";
import { sidebarViewMenuItems } from "../sidebar-view-menu";

type Leaf = { label: string; checked?: boolean; disabled?: boolean; onSelect: () => void };

function leaves(items: ReturnType<typeof sidebarViewMenuItems>): Leaf[] {
  return items.filter((item): item is Leaf => "onSelect" in item);
}

describe("sidebarViewMenuItems", () => {
  it("checks project grouping and both labels by default, with project labels disabled", () => {
    const items = leaves(sidebarViewMenuItems({}, vi.fn()));
    expect(items.map((i) => [i.label, !!i.checked, !!i.disabled])).toEqual([
      ["Group by project", true, false],
      ["Group by status", false, false],
      ["Show task keys", true, false],
      ["Show project labels", true, true],
    ]);
  });

  it("enables project labels when grouped by status and reflects hidden options", () => {
    const items = leaves(
      sidebarViewMenuItems(
        { sidebar_group_by: "status", hide_task_keys: true, hide_project_labels: true },
        vi.fn(),
      ),
    );
    expect(items.map((i) => [i.label, !!i.checked, !!i.disabled])).toEqual([
      ["Group by project", false, false],
      ["Group by status", true, false],
      ["Show task keys", false, false],
      ["Show project labels", false, false],
    ]);
  });

  it("sends the matching settings patch for each item", () => {
    const update = vi.fn();
    const items = leaves(
      sidebarViewMenuItems({ sidebar_group_by: "status", hide_task_keys: true }, update),
    );
    items.forEach((i) => i.onSelect());
    expect(update.mock.calls.map(([patch]) => patch)).toEqual([
      { sidebar_group_by: "project" },
      { sidebar_group_by: "status" },
      { hide_task_keys: false },
      { hide_project_labels: true },
    ]);
  });
});
