import { afterEach, describe, expect, it, vi } from "vitest";
import { jiraPreferencesEntrypoint, jiraSidebarSectionEntrypoint } from "./entry";
import type { PluginUiContext } from "../../lib/plugin-sdk";

type Settings = {
  site: string;
  sync_interval_ms: number;
  sources: Record<
    string,
    { jql: string; status_map?: Record<string, string>; writeback?: unknown }
  >;
};

const initialSettings = (): Settings => ({
  site: "https://example.atlassian.net",
  sync_interval_ms: 60000,
  sources: {},
});

function preferencesContext(
  settings: Settings,
  status: Record<string, unknown>,
  update?: (next: Settings) => Promise<Settings>,
) {
  const call = vi.fn(async (method: string, params?: Settings) => {
    if (method === "jira.settings.get") return settings;
    if (method === "jira.status") return status;
    if (method === "jira.settings.update") return update ? update(params!) : params;
    throw new Error(`unexpected Jira call: ${method}`);
  });
  return {
    call,
    plugin: { id: "jira" },
    contribution: { id: "jira-preferences" },
    host: {
      call,
      navigation: { open: vi.fn(), close: vi.fn(), openPreferences: vi.fn() },
      sidebar: { register: vi.fn(() => () => {}), select: vi.fn() },
      data: { changed: vi.fn() },
    },
  } as unknown as PluginUiContext;
}

describe("jiraPreferencesEntrypoint", () => {
  let host: HTMLElement | undefined;

  afterEach(() => host?.remove());

  function mount(settings = initialSettings(), status: Record<string, unknown> = {}) {
    host = document.createElement("div");
    const root = host.attachShadow({ mode: "open" });
    const context = preferencesContext(settings, {
      connected: false,
      authorizing: false,
      site: null,
      last_error: null,
      ...status,
    });
    jiraPreferencesEntrypoint.mount(root, context);
    return { root, context };
  }

  it("locks the connected site, labels generated controls, and hides deferred writeback", async () => {
    const settings: Settings = {
      ...initialSettings(),
      sources: {
        primary: {
          jql: "project = PLA",
          status_map: { Open: "todo" },
          writeback: { enabled: true },
        },
      },
    };
    const { root } = mount(settings, { connected: true, site: settings.site });

    await vi.waitFor(() =>
      expect(
        root.querySelector<HTMLInputElement>("input[placeholder='https://company.atlassian.net']"),
      ).toBeTruthy(),
    );
    const site = root.querySelector<HTMLInputElement>(
      "input[placeholder='https://company.atlassian.net']",
    )!;
    expect(site.disabled).toBe(true);
    expect(root.querySelector(`label[for='${site.id}']`)?.textContent).toBe("Site URL");
    expect(root.querySelector<HTMLInputElement>("input[aria-label='Jira status']")).toBeTruthy();
    expect(root.querySelector("select[aria-label='PlaneAI status for Open']")).toBeTruthy();
    expect(root.querySelector("button[aria-label='Remove status mapping for Open']")).toBeTruthy();
    expect(root.textContent).not.toContain("Writeback");
  });

  it("adds a source with a nonblank safe JQL default", async () => {
    const { root, context } = mount();
    await vi.waitFor(() =>
      expect(
        [...root.querySelectorAll("button")].find((button) => button.textContent === "Add source"),
      ).toBeTruthy(),
    );
    [...root.querySelectorAll("button")]
      .find((button) => button.textContent === "Add source")!
      .click();

    await vi.waitFor(() =>
      expect(context.host.call).toHaveBeenCalledWith("jira.settings.update", expect.anything()),
    );
    const updates = vi
      .mocked(context.host.call)
      .mock.calls.filter(([method]) => method === "jira.settings.update");
    const saved = updates.at(-1)?.[1] as Settings;
    expect(saved.sources.source.jql).toBe("key = __PLANEAI_CONFIGURE_SOURCE__");
  });

  it("serializes updates without dropping an earlier optimistic change", async () => {
    host = document.createElement("div");
    const root = host.attachShadow({ mode: "open" });
    let releaseFirstSave: ((settings: Settings) => void) | undefined;
    let updateCount = 0;
    const context = preferencesContext(
      initialSettings(),
      { connected: false, authorizing: false, site: null, last_error: null },
      async (next) => {
        updateCount += 1;
        if (updateCount === 1) {
          return new Promise<Settings>((resolve) => {
            releaseFirstSave = resolve;
          });
        }
        return next;
      },
    );
    jiraPreferencesEntrypoint.mount(root, context);
    await vi.waitFor(() =>
      expect(
        root.querySelector<HTMLInputElement>("input[placeholder='https://company.atlassian.net']"),
      ).toBeTruthy(),
    );
    const site = root.querySelector<HTMLInputElement>(
      "input[placeholder='https://company.atlassian.net']",
    )!;
    site.value = "https://changed.atlassian.net";
    site.dispatchEvent(new Event("change", { bubbles: true }));
    await vi.waitFor(() =>
      expect(
        vi
          .mocked(context.host.call)
          .mock.calls.filter(([method]) => method === "jira.settings.update"),
      ).toHaveLength(1),
    );
    [...root.querySelectorAll("button")]
      .find((button) => button.textContent === "Add source")!
      .click();
    releaseFirstSave!({ ...initialSettings(), site: "https://changed.atlassian.net" });

    await vi.waitFor(() => {
      expect(
        vi
          .mocked(context.host.call)
          .mock.calls.filter(([method]) => method === "jira.settings.update"),
      ).toHaveLength(2);
    });
    const updates = vi
      .mocked(context.host.call)
      .mock.calls.filter(([method]) => method === "jira.settings.update");
    const finalSave = updates[1][1] as Settings;
    expect(finalSave.site).toBe("https://changed.atlassian.net");
    expect(finalSave.sources.source.jql).toBe("key = __PLANEAI_CONFIGURE_SOURCE__");
  });

  it("shows a save error and does not start OAuth after persistence fails", async () => {
    host = document.createElement("div");
    const root = host.attachShadow({ mode: "open" });
    const context = preferencesContext(
      initialSettings(),
      { connected: false, authorizing: false, site: null, last_error: null },
      async () => {
        throw new Error("JQL must not be blank");
      },
    );
    jiraPreferencesEntrypoint.mount(root, context);
    await vi.waitFor(() =>
      expect(
        [...root.querySelectorAll("button")].find((button) => button.textContent === "Connect"),
      ).toBeTruthy(),
    );
    [...root.querySelectorAll("button")]
      .find((button) => button.textContent === "Connect")!
      .click();

    await vi.waitFor(() => expect(root.textContent).toContain("JQL must not be blank"));
    expect(context.host.call).not.toHaveBeenCalledWith("jira.connect.start", expect.anything());
  });
});

describe("jiraSidebarSectionEntrypoint", () => {
  let host: HTMLElement | undefined;

  afterEach(() => {
    host?.remove();
  });

  function mountSidebar(
    options: {
      projects?: Array<{ id: string; name: string; path: string; hidden: boolean }>;
      createChild?: ReturnType<typeof vi.fn>;
      issue?: { key: string; title: string; description: string };
      newProject?: Promise<{ id: string; name: string; path: string; hidden: boolean } | null>;
    } = {},
  ) {
    host = document.createElement("div");
    const root = host.attachShadow({ mode: "open" });
    const select = vi.fn();
    const register = vi.fn(() => () => {});
    const createChild = options.createChild ?? vi.fn().mockResolvedValue({ key: "PLA-99" });
    const refreshAssignment = vi.fn().mockResolvedValue(undefined);
    const close = vi.fn();
    const setSubmitting = vi.fn();
    const projects = vi
      .fn()
      .mockResolvedValue(
        options.projects ?? [{ id: "project-1", name: "PlaneAI", path: "/planeai", hidden: false }],
      );
    const openProjectForm = vi.fn(() => options.newProject ?? Promise.resolve(null));
    const dialogRoots: ShadowRoot[] = [];
    const call = vi.fn((method: string) => {
      if (method === "jira.sidebar.items") {
        return Promise.resolve({
          items: [
            { key: "PLA-42", title: "Synchronize the sidebar", status: "todo", child_count: 0 },
          ],
        });
      }
      if (method === "jira.issue.get") {
        return Promise.resolve(
          options.issue ?? {
            key: "PLA-42",
            title: "Synchronize the sidebar",
            description: "Keep child counts current.",
          },
        );
      }
      return Promise.reject(new Error(`unexpected Jira call: ${method}`));
    });
    const openModal = vi.fn((modalOptions: import("../../lib/plugin-sdk").PluginModalOptions) => {
      const dialogHost = document.createElement("div");
      const dialogRoot = dialogHost.attachShadow({ mode: "open" });
      dialogRoots.push(dialogRoot);
      modalOptions.mount(dialogRoot, { close, setSubmitting });
      return { close, setSubmitting };
    });

    jiraSidebarSectionEntrypoint.mount(root, {
      plugin: { id: "jira" },
      contribution: { id: "jira-sidebar-section" },
      host: {
        call,
        navigation: { open: vi.fn(), close: vi.fn(), openPreferences: vi.fn() },
        sidebar: { register, select },
        data: { changed: vi.fn(), refreshAssignment, notify: vi.fn() },
        projects: { list: projects },
        tasks: { createChild },
        interaction: { openModal, openProjectForm },
      },
    } as unknown as PluginUiContext);
    return {
      root,
      select,
      call,
      createChild,
      refreshAssignment,
      close,
      setSubmitting,
      projects,
      openProjectForm,
      openModal,
      dialogRoots,
    };
  }

  async function openAndLoad(sidebar: ReturnType<typeof mountSidebar>, expectPicker = true) {
    await vi.waitFor(() =>
      expect(sidebar.root.querySelector<HTMLButtonElement>(".issue")).toBeTruthy(),
    );
    sidebar.root.querySelector<HTMLButtonElement>(".issue")!.click();
    await vi.waitFor(() => expect(sidebar.openModal).toHaveBeenCalled());
    if (!expectPicker) {
      const options = sidebar.openModal.mock.calls.at(-1)?.[0];
      if (!options) throw new Error("assignment modal was not requested");
      const root = document.createElement("div").attachShadow({ mode: "open" });
      options.mount(root, { close: sidebar.close, setSubmitting: sidebar.setSubmitting });
      return root;
    }
    await vi.waitFor(() =>
      expect(sidebar.dialogRoots.at(-1)?.querySelector("select")).toBeTruthy(),
    );
    return sidebar.dialogRoots.at(-1)!;
  }

  it("opens assignment from a pointer click and submits the copied Jira payload with Mod+Enter", async () => {
    const sidebar = mountSidebar();
    const dialog = await openAndLoad(sidebar);
    expect(sidebar.select).toHaveBeenCalledWith("issue:PLA-42");
    expect(sidebar.call).toHaveBeenCalledWith("jira.issue.get", { key: "PLA-42" });
    expect(dialog.textContent).toContain("Keep child counts current.");

    const picker = dialog.querySelector<HTMLSelectElement>("select")!;
    picker.value = "project-1";
    picker.dispatchEvent(new Event("change", { bubbles: true }));
    await vi.waitFor(() =>
      expect(dialog.querySelector<HTMLButtonElement>("button.primary")?.disabled).toBe(false),
    );
    dialog.querySelector("form")!.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Enter",
        metaKey: true,
        bubbles: true,
        cancelable: true,
      }),
    );

    await vi.waitFor(() =>
      expect(sidebar.createChild).toHaveBeenCalledWith({
        project: { id: "project-1", name: "PlaneAI", path: "/planeai", hidden: false },
        title: "Synchronize the sidebar",
        description: "Keep child counts current.",
        parentKey: "PLA-42",
      }),
    );
    expect(sidebar.refreshAssignment).toHaveBeenCalledOnce();
    expect(sidebar.setSubmitting).toHaveBeenCalledWith(true);
    await vi.waitFor(() => expect(sidebar.close).toHaveBeenCalledOnce());
  });

  it("shows submission errors without closing the assignment dialog", async () => {
    const createChild = vi.fn().mockRejectedValue(new Error("Task database unavailable"));
    const sidebar = mountSidebar({ createChild });
    const dialog = await openAndLoad(sidebar);
    const picker = dialog.querySelector<HTMLSelectElement>("select")!;
    picker.value = "project-1";
    picker.dispatchEvent(new Event("change", { bubbles: true }));
    await vi.waitFor(() =>
      expect(dialog.querySelector<HTMLButtonElement>("button.primary")?.disabled).toBe(false),
    );
    dialog.querySelector("form")!.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Enter",
        ctrlKey: true,
        bubbles: true,
        cancelable: true,
      }),
    );

    await vi.waitFor(() => expect(dialog.textContent).toContain("Task database unavailable"));
    expect(sidebar.close).not.toHaveBeenCalled();
    expect(sidebar.setSubmitting).toHaveBeenLastCalledWith(false);
  });

  it("opens New Project with N, selects its returned identity, and allows a repeated assignment", async () => {
    const created = { id: "project-2", name: "New Project", path: "/new", hidden: false };
    const sidebar = mountSidebar({
      projects: [],
      newProject: Promise.resolve(created),
    });
    let dialog = await openAndLoad(sidebar, false);
    await vi.waitFor(() => expect(dialog.textContent).toContain("No PlaneAI projects available"));
    sidebar.projects.mockResolvedValueOnce([created]);
    dialog.querySelector("form")!.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "n",
        bubbles: true,
        cancelable: true,
      }),
    );
    await vi.waitFor(() => expect(sidebar.openProjectForm).toHaveBeenCalledOnce());
    await vi.waitFor(() =>
      expect(dialog.querySelector<HTMLSelectElement>("select")?.value).toBe("project-2"),
    );
    dialog.querySelector("form")!.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Enter",
        ctrlKey: true,
        bubbles: true,
        cancelable: true,
      }),
    );
    await vi.waitFor(() => expect(sidebar.createChild).toHaveBeenCalledTimes(1));

    sidebar.projects.mockResolvedValue([created]);
    const repeatOptions = sidebar.openModal.mock.calls.at(-1)?.[0];
    if (!repeatOptions) throw new Error("assignment modal was not requested");
    const second = document.createElement("div").attachShadow({ mode: "open" });
    repeatOptions.mount(second, { close: sidebar.close, setSubmitting: sidebar.setSubmitting });
    await vi.waitFor(() => expect(second.querySelector<HTMLSelectElement>("select")).toBeTruthy());
    const picker = second.querySelector<HTMLSelectElement>("select")!;
    picker.value = "project-2";
    picker.dispatchEvent(new Event("change", { bubbles: true }));
    await vi.waitFor(() =>
      expect(second.querySelector<HTMLButtonElement>("button.primary")?.disabled).toBe(false),
    );
    second.querySelector("form")!.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Enter",
        ctrlKey: true,
        bubbles: true,
        cancelable: true,
      }),
    );
    await vi.waitFor(() => expect(sidebar.createChild).toHaveBeenCalledTimes(2));
  });
});
