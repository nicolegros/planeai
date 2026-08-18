import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";

const { pluginCall, projectList, sessionList } = vi.hoisted(() => ({
  pluginCall: vi.fn(),
  projectList: vi.fn(),
  sessionList: vi.fn(),
}));

vi.mock("../../lib/api", () => ({
  plugins: {
    call: pluginCall,
    dataChanged: vi.fn().mockResolvedValue(undefined),
  },
  pr: {
    getPrStatus: vi.fn().mockResolvedValue({ checks: [], conflicting: false }),
    getPrComments: vi.fn().mockResolvedValue(0),
  },
  projects: {
    list: projectList,
    getAutoMode: vi.fn().mockResolvedValue(false),
    setAutoMode: vi.fn().mockResolvedValue(undefined),
  },
  sessions: {
    list: sessionList,
    acknowledge: vi.fn().mockResolvedValue(undefined),
    saveMruOrder: vi.fn().mockResolvedValue(undefined),
  },
  tasks: {
    listAll: vi.fn().mockResolvedValue([]),
  },
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: vi.fn(() => ({ onCloseRequested: vi.fn().mockResolvedValue(() => {}) })),
}));

vi.mock("@tauri-apps/plugin-opener", () => ({
  openUrl: vi.fn(),
}));

import UnifiedSidebarJiraHarness from "./UnifiedSidebarJiraHarness.svelte";
import { focusSidebar, focusTerminal } from "../../lib/focus.svelte";
import { loadProjects } from "../../lib/project-store.svelte";
import { _resetForTests, loadSessions } from "../../lib/session-orchestrator.svelte";
import { getSelectedIndex, setSelectedIndex } from "../../lib/sidebar-nav.svelte";

describe("UnifiedSidebar Jira sidebar integration", () => {
  let target: HTMLElement;
  let component: ReturnType<typeof mount> | undefined;

  beforeEach(() => {
    _resetForTests();
    projectList.mockResolvedValue([]);
    sessionList.mockResolvedValue([]);
    pluginCall.mockResolvedValue({
      items: [
        { key: "PLA-42", title: "Selected Jira issue", status: "todo", child_count: 0 },
        { key: "PLA-43", title: "Next Jira issue", status: "todo", child_count: 0 },
      ],
    });
    HTMLElement.prototype.scrollIntoView = vi.fn();
    focusSidebar();
    setSelectedIndex(0);
    target = document.createElement("div");
    document.body.append(target);
  });

  afterEach(async () => {
    if (component) unmount(component);
    component = undefined;
    target?.remove();
    _resetForTests();
    projectList.mockResolvedValue([]);
    await loadProjects();
    focusTerminal();
    setSelectedIndex(0);
    vi.clearAllMocks();
  });

  it("keeps pointer and keyboard selection synchronized with Jira issue rows", async () => {
    component = mount(UnifiedSidebarJiraHarness, { target });

    await vi.waitFor(() => {
      expect(
        target
          .querySelector("[data-plugin-ui-contribution='jira:section']")
          ?.shadowRoot?.querySelectorAll(".issue"),
      ).toHaveLength(2);
    });

    const host = target.querySelector<HTMLElement>("[data-plugin-ui-contribution='jira:section']")!;
    const issue = host.shadowRoot!.querySelector<HTMLButtonElement>(".issue")!;
    issue.click();

    await vi.waitFor(() => {
      expect(getSelectedIndex()).toBe(2);
      expect(host.shadowRoot?.querySelectorAll(".selected")).toHaveLength(1);
      expect(host.shadowRoot?.querySelector(".issue.selected")?.textContent).toContain("PLA-42");
    });

    const selectedIssue = host.shadowRoot!.querySelector<HTMLButtonElement>(".issue.selected")!;
    expect(host.shadowRoot?.activeElement).toBe(selectedIssue);
    selectedIssue.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "ArrowDown",
        bubbles: true,
        composed: true,
        cancelable: true,
      }),
    );

    await vi.waitFor(() => {
      expect(getSelectedIndex()).toBe(3);
      expect(host.shadowRoot?.querySelector(".issue.selected")?.textContent).toContain("PLA-43");
    });
  });

  it("enters and traverses Jira rows from focused sidebar keyboard navigation", async () => {
    component = mount(UnifiedSidebarJiraHarness, { target });

    await vi.waitFor(() => {
      expect(
        target
          .querySelector("[data-plugin-ui-contribution='jira:section']")
          ?.shadowRoot?.querySelectorAll(".issue"),
      ).toHaveLength(2);
    });

    window.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "j",
        bubbles: true,
        cancelable: true,
      }),
    );

    const host = target.querySelector<HTMLElement>("[data-plugin-ui-contribution='jira:section']")!;
    await vi.waitFor(() => {
      const header = host.shadowRoot?.querySelector<HTMLButtonElement>(".section-header.selected");
      expect(getSelectedIndex()).toBe(1);
      expect(host.shadowRoot?.activeElement).toBe(header);
    });

    const header = host.shadowRoot!.querySelector<HTMLButtonElement>(".section-header.selected")!;
    header.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "j",
        bubbles: true,
        composed: true,
        cancelable: true,
      }),
    );

    await vi.waitFor(() => {
      const firstIssue = host.shadowRoot?.querySelector<HTMLButtonElement>(".issue.selected");
      expect(getSelectedIndex()).toBe(2);
      expect(firstIssue?.textContent).toContain("PLA-42");
      expect(host.shadowRoot?.activeElement).toBe(firstIssue);
    });

    const firstIssue = host.shadowRoot!.querySelector<HTMLButtonElement>(".issue.selected")!;
    firstIssue.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "j",
        bubbles: true,
        composed: true,
        cancelable: true,
      }),
    );

    await vi.waitFor(() => {
      expect(getSelectedIndex()).toBe(3);
      expect(host.shadowRoot?.querySelector(".issue.selected")?.textContent).toContain("PLA-43");
    });

    const secondIssue = host.shadowRoot!.querySelector<HTMLButtonElement>(".issue.selected")!;
    secondIssue.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "k",
        bubbles: true,
        composed: true,
        cancelable: true,
      }),
    );

    await vi.waitFor(() => {
      expect(getSelectedIndex()).toBe(2);
      expect(host.shadowRoot?.querySelector(".issue.selected")?.textContent).toContain("PLA-42");
    });
  });

  it("does not reset to the active session when Jira focus rerenders its rows", async () => {
    projectList.mockResolvedValue([{ id: "p1", name: "Project", path: "/project", hidden: false }]);
    sessionList.mockResolvedValue([
      {
        id: "s1",
        project_id: "p1",
        name: "Active session",
        tmux_name: null,
        branch: "main",
        status: "active",
        created_at: "2026-08-18T00:00:00Z",
        worktree_path: null,
        provider: "kiro",
        backend: "tmux",
        tab_count: 1,
        base_branch: null,
        task_key: null,
        pr_url: null,
        pr_state: null,
      },
    ]);
    await loadProjects();
    await loadSessions();
    component = mount(UnifiedSidebarJiraHarness, { target });

    await vi.waitFor(() => {
      expect(getSelectedIndex()).toBe(1);
      expect(
        target
          .querySelector("[data-plugin-ui-contribution='jira:section']")
          ?.shadowRoot?.querySelectorAll(".issue"),
      ).toHaveLength(2);
    });

    window.dispatchEvent(
      new KeyboardEvent("keydown", { key: "j", bubbles: true, cancelable: true }),
    );
    window.dispatchEvent(
      new KeyboardEvent("keydown", { key: "j", bubbles: true, cancelable: true }),
    );

    const jiraHost = target.querySelector<HTMLElement>(
      "[data-plugin-ui-contribution='jira:section']",
    )!;
    await vi.waitFor(() => {
      expect(getSelectedIndex()).toBe(3);
      expect(jiraHost.shadowRoot?.querySelector(".section-header.selected")).toBeTruthy();
    });

    const header = jiraHost.shadowRoot!.querySelector<HTMLButtonElement>(
      ".section-header.selected",
    )!;
    header.dispatchEvent(
      new KeyboardEvent("keydown", { key: "j", bubbles: true, composed: true, cancelable: true }),
    );

    await vi.waitFor(() => {
      expect(getSelectedIndex()).toBe(4);
      expect(jiraHost.shadowRoot?.querySelector(".issue.selected")?.textContent).toContain(
        "PLA-42",
      );
    });

    const firstIssue = jiraHost.shadowRoot!.querySelector<HTMLButtonElement>(".issue.selected")!;
    firstIssue.dispatchEvent(
      new KeyboardEvent("keydown", { key: "j", bubbles: true, composed: true, cancelable: true }),
    );

    await vi.waitFor(() => {
      expect(getSelectedIndex()).toBe(5);
      expect(jiraHost.shadowRoot?.querySelector(".issue.selected")?.textContent).toContain(
        "PLA-43",
      );
    });
  });
});
