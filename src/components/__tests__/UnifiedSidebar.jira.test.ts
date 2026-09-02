import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";

const { pluginCall, localUiSource, projectList, sessionList } = vi.hoisted(() => ({
  pluginCall: vi.fn(),
  localUiSource: vi.fn(),
  projectList: vi.fn(),
  sessionList: vi.fn(),
}));

vi.mock("../../lib/api", () => ({
  plugins: {
    call: pluginCall,
    localUiSource,
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
import { focusSidebar, focusTerminal, getActiveZone } from "../../lib/focus.svelte";
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
    localUiSource.mockReset();
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

  it("activates sidebar keyboard navigation when a built-in sidebar control is clicked", async () => {
    focusTerminal();
    component = mount(UnifiedSidebarJiraHarness, { target });

    await vi.waitFor(() => {
      expect(
        target
          .querySelector("[data-plugin-ui-contribution='jira:section']")
          ?.shadowRoot?.querySelectorAll(".issue"),
      ).toHaveLength(2);
    });

    const addProject = await vi.waitFor(() => {
      const button = target.querySelector<HTMLButtonElement>("button[aria-label='New project']");
      expect(button).toBeTruthy();
      return button!;
    });
    addProject.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));

    expect(getActiveZone()).toBe("sidebar");
    window.dispatchEvent(
      new KeyboardEvent("keydown", { key: "j", bubbles: true, cancelable: true }),
    );
    await vi.waitFor(() => expect(getSelectedIndex()).toBe(1));
  });

  it("opens a host-managed assignment modal when a Jira issue is clicked", async () => {
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
      expect(document.querySelector("[data-plugin-modal]")).toBeTruthy();
      expect(host.shadowRoot?.querySelector(".issue.selected")?.textContent).toContain("PLA-42");
    });
    expect(host.shadowRoot?.activeElement).not.toBe(issue);
  });

  it("opens assignment when Enter activates a focused Jira issue", async () => {
    component = mount(UnifiedSidebarJiraHarness, { target });

    await vi.waitFor(() => {
      expect(
        target
          .querySelector("[data-plugin-ui-contribution='jira:section']")
          ?.shadowRoot?.querySelectorAll(".issue"),
      ).toHaveLength(2);
    });

    window.dispatchEvent(
      new KeyboardEvent("keydown", { key: "j", bubbles: true, cancelable: true }),
    );
    const host = target.querySelector<HTMLElement>("[data-plugin-ui-contribution='jira:section']")!;
    await vi.waitFor(() =>
      expect(
        host.shadowRoot?.querySelector<HTMLButtonElement>(".section-header.selected"),
      ).toBeTruthy(),
    );
    const header = host.shadowRoot!.querySelector<HTMLButtonElement>(".section-header.selected")!;
    header.dispatchEvent(
      new KeyboardEvent("keydown", { key: "j", bubbles: true, composed: true, cancelable: true }),
    );
    await vi.waitFor(() =>
      expect(host.shadowRoot?.querySelector<HTMLButtonElement>(".issue.selected")).toBeTruthy(),
    );

    const issue = host.shadowRoot!.querySelector<HTMLButtonElement>(".issue.selected")!;
    issue.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Enter",
        bubbles: true,
        composed: true,
        cancelable: true,
      }),
    );

    await vi.waitFor(() => expect(document.querySelector("[data-plugin-modal]")).toBeTruthy());
    expect(host.shadowRoot?.querySelector(".issue.selected")?.textContent).toContain("PLA-42");
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

    const selectedFirstIssue =
      host.shadowRoot!.querySelector<HTMLButtonElement>(".issue.selected")!;
    selectedFirstIssue.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "k",
        bubbles: true,
        composed: true,
        cancelable: true,
      }),
    );

    await vi.waitFor(() => {
      expect(getSelectedIndex()).toBe(1);
      expect(host.shadowRoot?.querySelector(".section-header.selected")).toBeTruthy();
    });

    const selectedHeader = host.shadowRoot!.querySelector<HTMLButtonElement>(
      ".section-header.selected",
    )!;
    selectedHeader.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "k",
        bubbles: true,
        composed: true,
        cancelable: true,
      }),
    );

    await vi.waitFor(() => {
      expect(getSelectedIndex()).toBe(0);
      expect(host.shadowRoot?.querySelectorAll(".selected")).toHaveLength(0);
    });
  });

  it("removes a failed local contribution without blocking sidebar navigation", async () => {
    localUiSource.mockRejectedValue(new Error("DataCloneError: object could not be cloned"));
    component = mount(UnifiedSidebarJiraHarness, {
      target,
      props: { includeLocalContribution: true },
    });

    const localHost = await vi.waitFor(() => {
      const host = target.querySelector<HTMLElement>(
        "[data-plugin-ui-contribution='local-fixture:log']",
      );
      expect(host).toBeTruthy();
      return host!;
    });
    const frame = localHost.shadowRoot?.querySelector<HTMLIFrameElement>("iframe");
    expect(frame).toBeTruthy();
    frame!.dispatchEvent(new Event("load"));

    await vi.waitFor(() => expect(localUiSource).toHaveBeenCalledWith("local-fixture", "log"));
    await vi.waitFor(() =>
      expect(target.querySelector("[data-plugin-ui-contribution='local-fixture:log']")).toBeNull(),
    );

    window.dispatchEvent(
      new KeyboardEvent("keydown", { key: "j", bubbles: true, cancelable: true }),
    );
    const jiraHost = target.querySelector<HTMLElement>(
      "[data-plugin-ui-contribution='jira:section']",
    )!;
    await vi.waitFor(() => {
      expect(getSelectedIndex()).toBe(1);
      expect(jiraHost.shadowRoot?.querySelector(".section-header.selected")).toBeTruthy();
    });
  });

  it("clears Jira row selection when focus leaves the sidebar", async () => {
    component = mount(UnifiedSidebarJiraHarness, { target });

    await vi.waitFor(() => {
      expect(
        target
          .querySelector("[data-plugin-ui-contribution='jira:section']")
          ?.shadowRoot?.querySelectorAll(".issue"),
      ).toHaveLength(2);
    });
    window.dispatchEvent(
      new KeyboardEvent("keydown", { key: "j", bubbles: true, cancelable: true }),
    );

    const jiraHost = target.querySelector<HTMLElement>(
      "[data-plugin-ui-contribution='jira:section']",
    )!;
    await vi.waitFor(() =>
      expect(jiraHost.shadowRoot?.querySelector(".section-header.selected")).toBeTruthy(),
    );

    focusTerminal();

    await vi.waitFor(() =>
      expect(jiraHost.shadowRoot?.querySelectorAll(".selected")).toHaveLength(0),
    );
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
