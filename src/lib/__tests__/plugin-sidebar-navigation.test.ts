import { describe, expect, it, vi } from "vitest";
import {
  getAllPluginSidebarContributions,
  getPluginSidebarRows,
  registerPluginSidebarContribution,
} from "../plugin-sidebar-navigation.svelte";

describe("plugin sidebar navigation registry", () => {
  it("registers ordered custom rows with keyboard callbacks and removes them on disposal", () => {
    const onSelect = vi.fn();
    const onCollapse = vi.fn();
    const onFocus = vi.fn();
    const dispose = registerPluginSidebarContribution("jira:jira-sidebar-section", [
      { id: "header", onCollapse, onFocus },
      { id: "issue:ABC-1", onSelect, onFocus },
    ]);

    expect(getPluginSidebarRows("jira:jira-sidebar-section").map((row) => row.id)).toEqual([
      "header",
      "issue:ABC-1",
    ]);
    expect(getAllPluginSidebarContributions().map((contribution) => contribution.key)).toContain(
      "jira:jira-sidebar-section",
    );

    const [, issue] = getPluginSidebarRows("jira:jira-sidebar-section");
    issue?.onSelect?.();
    issue?.onFocus?.(true);
    getPluginSidebarRows("jira:jira-sidebar-section")[0]?.onCollapse?.();
    expect(onSelect).toHaveBeenCalledOnce();
    expect(onFocus).toHaveBeenCalledWith(true);
    expect(onCollapse).toHaveBeenCalledOnce();

    dispose();
    expect(getPluginSidebarRows("jira:jira-sidebar-section")).toEqual([]);
  });
});
