import { afterEach, describe, expect, it, vi } from "vitest";
import {
  jiraDepartedInteractionEntrypoint,
  jiraPreferencesEntrypoint,
  jiraSidebarSectionEntrypoint,
} from "./entry";
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

  it("uses the app preference layout and styling hooks", async () => {
    const { root } = mount({
      ...initialSettings(),
      sources: { primary: { jql: "project = PLA", status_map: { Open: "todo" } } },
    });

    await vi.waitFor(() => expect(root.querySelector(".page > .card")).toBeTruthy());
    expect(root.querySelectorAll(".page > .card")).toHaveLength(2);
    expect(root.querySelector(".page .field")).toBeTruthy();
    expect(root.querySelector(".page .source")).toBeTruthy();
    expect(root.querySelector(".page .mapping")).toBeTruthy();
    expect(root.querySelector(".page .add-source")).toBeTruthy();
    expect(root.querySelector(".page .add-mapping")).toBeTruthy();
    expect(root.querySelector("style")?.textContent).toContain("max-width:564px");
    expect(root.querySelector("style")?.textContent).toContain("var(--color-panel)");
  });

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
  it("allows an in-flight OAuth attempt to be cancelled with its correlated attempt ID", async () => {
    host = document.createElement("div");
    const root = host.attachShadow({ mode: "open" });
    let authorizing = false;
    const call = vi.fn(async (method: string, _params?: unknown) => {
      if (method === "jira.settings.get") return initialSettings();
      if (method === "jira.settings.update") return initialSettings();
      if (method === "jira.status") {
        return { connected: false, authorizing, site: null, last_error: null };
      }
      if (method === "jira.connect.start")
        return { authorization_url: "https://auth.atlassian.com/authorize" };
      if (method === "jira.open_browser") return { opened: true };
      if (method === "jira.connect.complete") {
        authorizing = true;
        return { authorizing: true };
      }
      if (method === "jira.connect.cancel") {
        authorizing = false;
        return { cancelled: true };
      }
      throw new Error(`unexpected Jira call: ${method}`);
    });
    const context = {
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
    jiraPreferencesEntrypoint.mount(root, context);

    await vi.waitFor(() =>
      expect(
        [...root.querySelectorAll("button")].find((button) => button.textContent === "Connect"),
      ).toBeTruthy(),
    );
    [...root.querySelectorAll("button")]
      .find((button) => button.textContent === "Connect")!
      .click();
    await vi.waitFor(() =>
      expect(
        [...root.querySelectorAll("button")].find(
          (button) => button.textContent === "Cancel authorization",
        ),
      ).toBeTruthy(),
    );
    const start = vi
      .mocked(context.host.call)
      .mock.calls.find(([method]) => method === "jira.connect.start")!;
    const complete = vi
      .mocked(context.host.call)
      .mock.calls.find(([method]) => method === "jira.connect.complete")!;
    expect(complete[1]).toEqual(start[1]);

    [...root.querySelectorAll("button")]
      .find((button) => button.textContent === "Cancel authorization")!
      .click();
    await vi.waitFor(() =>
      expect(context.host.call).toHaveBeenCalledWith("jira.connect.cancel", start[1]),
    );
  });
  it("cancels the backend OAuth attempt when launching the browser fails", async () => {
    host = document.createElement("div");
    const root = host.attachShadow({ mode: "open" });
    const call = vi.fn(async (method: string, _params?: unknown) => {
      if (method === "jira.settings.get" || method === "jira.settings.update")
        return initialSettings();
      if (method === "jira.status")
        return { connected: false, authorizing: false, site: null, last_error: null };
      if (method === "jira.connect.start")
        return { authorization_url: "https://auth.atlassian.com/authorize" };
      if (method === "jira.open_browser") throw new Error("browser unavailable");
      if (method === "jira.connect.cancel") return { cancelled: true };
      throw new Error(`unexpected Jira call: ${method}`);
    });
    const context = {
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
    jiraPreferencesEntrypoint.mount(root, context);

    await vi.waitFor(() =>
      expect(
        [...root.querySelectorAll("button")].find((button) => button.textContent === "Connect"),
      ).toBeTruthy(),
    );
    [...root.querySelectorAll("button")]
      .find((button) => button.textContent === "Connect")!
      .click();
    await vi.waitFor(() => expect(root.textContent).toContain("browser unavailable"));
    const start = vi
      .mocked(context.host.call)
      .mock.calls.find(([method]) => method === "jira.connect.start")!;
    expect(context.host.call).toHaveBeenCalledWith("jira.connect.cancel", start[1]);
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
    const dispose = vi.fn();
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
      modalOptions.mount(dialogRoot, { close, dispose, setSubmitting });
      return { close, dispose, setSubmitting };
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
      dispose,
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
      options.mount(root, {
        close: sidebar.close,
        dispose: sidebar.dispose,
        setSubmitting: sidebar.setSubmitting,
      });
      return root;
    }
    await vi.waitFor(() =>
      expect(
        sidebar.dialogRoots
          .at(-1)
          ?.querySelector<HTMLInputElement>("input[aria-label='PlaneAI project']"),
      ).toBeTruthy(),
    );
    return sidebar.dialogRoots.at(-1)!;
  }

  async function chooseProject(dialog: ShadowRoot, name: string) {
    const picker = dialog.querySelector<HTMLInputElement>("input[aria-label='PlaneAI project']")!;
    picker.dispatchEvent(new FocusEvent("focus", { bubbles: true }));
    picker.value = name;
    picker.dispatchEvent(new Event("input", { bubbles: true }));
    await vi.waitFor(() =>
      expect(
        [...dialog.querySelectorAll<HTMLButtonElement>("[role='option']")].find(
          (option) => option.textContent === name,
        ),
      ).toBeTruthy(),
    );
    [...dialog.querySelectorAll<HTMLButtonElement>("[role='option']")]
      .find((option) => option.textContent === name)!
      .click();
    await vi.waitFor(() =>
      expect(
        dialog.querySelector<HTMLInputElement>("input[aria-label='PlaneAI project']")?.value,
      ).toBe(name),
    );
  }

  it("keeps Jira selection styling while suppressing native button focus outlines", async () => {
    const sidebar = mountSidebar();

    await vi.waitFor(() => expect(sidebar.root.querySelector(".issue")).toBeTruthy());

    const stylesheet = sidebar.root.querySelector("style")?.textContent;
    expect(stylesheet).toContain(
      ".section-header.selected,.issue.selected,.section-header:focus-visible,.issue:focus-visible { outline:2px solid var(--color-accent);",
    );
    expect(stylesheet).toContain(".section-header:focus,.issue:focus { outline:none; }");
  });

  it("uses PlaneAI normal and insert modes for assignment shortcuts", async () => {
    const sidebar = mountSidebar();
    const dialog = await openAndLoad(sidebar);
    const wrapper = dialog.querySelector<HTMLElement>("[data-form-keyboard]")!;
    const picker = dialog.querySelector<HTMLInputElement>("input[aria-label='PlaneAI project']")!;

    expect(dialog.textContent).toContain("NORMAL");

    const focusPicker = vi.spyOn(picker, "focus");
    wrapper.dispatchEvent(new KeyboardEvent("keydown", { key: "p", bubbles: true }));
    expect(focusPicker).toHaveBeenCalledOnce();
    picker.dispatchEvent(new FocusEvent("focusin", { bubbles: true }));
    expect(dialog.textContent).toContain("INSERT");

    picker.dispatchEvent(new FocusEvent("focus", { bubbles: true }));
    expect(dialog.textContent).toContain("INSERT");

    picker.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    expect(dialog.textContent).toContain("INSERT");

    picker.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    expect(dialog.textContent).toContain("NORMAL");

    wrapper.dispatchEvent(new KeyboardEvent("keydown", { key: "n", bubbles: true }));
    await vi.waitFor(() => expect(sidebar.openProjectForm).toHaveBeenCalledOnce());
  });

  it("opens assignment from a pointer click and submits the copied Jira payload with Mod+Enter", async () => {
    const sidebar = mountSidebar();
    const dialog = await openAndLoad(sidebar);
    expect(sidebar.select).toHaveBeenCalledWith("issue:PLA-42");
    expect(sidebar.call).toHaveBeenCalledWith("jira.issue.get", { key: "PLA-42" });
    expect(dialog.textContent).toContain("Keep child counts current.");
    expect(dialog.querySelector<HTMLSpanElement>(".submit-hint")?.textContent).toMatch(
      /^(⌘↵|Ctrl\+↵)$/,
    );

    await chooseProject(dialog, "PlaneAI");
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

  it("filters and selects a project through the searchable combobox", async () => {
    const sidebar = mountSidebar({
      projects: [
        { id: "project-1", name: "PlaneAI", path: "/planeai", hidden: false },
        { id: "project-2", name: "Archive cleanup", path: "/archive", hidden: false },
      ],
    });
    await vi.waitFor(() =>
      expect(sidebar.root.querySelector<HTMLButtonElement>(".issue")).toBeTruthy(),
    );
    sidebar.root.querySelector<HTMLButtonElement>(".issue")!.click();
    await vi.waitFor(() => expect(sidebar.openModal).toHaveBeenCalled());
    const dialog = sidebar.dialogRoots.at(-1)!;
    await vi.waitFor(() =>
      expect(
        dialog.querySelector<HTMLInputElement>("input[aria-label='PlaneAI project']"),
      ).toBeTruthy(),
    );

    const picker = dialog.querySelector<HTMLInputElement>("input[aria-label='PlaneAI project']")!;
    picker.dispatchEvent(new FocusEvent("focus", { bubbles: true }));
    picker.dispatchEvent(new FocusEvent("focusin", { bubbles: true }));
    picker.dispatchEvent(new KeyboardEvent("keydown", { key: "n", bubbles: true }));
    expect(sidebar.openProjectForm).not.toHaveBeenCalled();
    picker.value = "archive";
    picker.dispatchEvent(new Event("input", { bubbles: true }));
    await vi.waitFor(() => {
      const results = dialog.querySelector<HTMLElement>("[role='listbox']")!;
      expect(results.dataset.layout).toBe("flow");
      expect(dialog.querySelectorAll<HTMLButtonElement>("[role='option']")).toHaveLength(1);
    });
    const option = dialog.querySelector<HTMLButtonElement>("[role='option']")!;
    expect(option.textContent).toContain("Archive cleanup");
    option.click();

    await vi.waitFor(() =>
      expect(
        dialog.querySelector<HTMLInputElement>("input[aria-label='PlaneAI project']")?.value,
      ).toBe("Archive cleanup"),
    );
    expect(dialog.querySelector<HTMLButtonElement>("button.primary")?.disabled).toBe(false);
  });

  it("keeps Enter project selection inside the assignment modal and restores picker focus", async () => {
    const sidebar = mountSidebar({
      projects: [
        { id: "project-1", name: "PlaneAI", path: "/planeai", hidden: false },
        { id: "project-2", name: "Archive cleanup", path: "/archive", hidden: false },
      ],
    });
    const dialog = await openAndLoad(sidebar);
    const picker = dialog.querySelector<HTMLInputElement>("input[aria-label='PlaneAI project']")!;
    const outsideKeydown = vi.fn();
    dialog.host.addEventListener("keydown", outsideKeydown);
    const focus = vi.spyOn(HTMLElement.prototype, "focus");

    picker.dispatchEvent(new FocusEvent("focus", { bubbles: true }));
    picker.value = "archive";
    picker.dispatchEvent(new Event("input", { bubbles: true }));
    await vi.waitFor(() => expect(dialog.querySelectorAll("[role='option']")).toHaveLength(1));
    picker.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Enter",
        bubbles: true,
        cancelable: true,
        composed: true,
      }),
    );

    await vi.waitFor(() =>
      expect(
        dialog.querySelector<HTMLInputElement>("input[aria-label='PlaneAI project']")?.value,
      ).toBe("Archive cleanup"),
    );
    const replacementPicker = dialog.querySelector<HTMLInputElement>(
      "input[aria-label='PlaneAI project']",
    )!;
    replacementPicker.dispatchEvent(new FocusEvent("focus", { bubbles: true }));
    expect(replacementPicker.value).toBe("Archive cleanup");
    expect(replacementPicker.getAttribute("aria-expanded")).toBe("false");
    expect(outsideKeydown).not.toHaveBeenCalled();
    expect(focus).toHaveBeenCalledWith();
  });

  it("keeps unbound normal-mode keys inside the assignment modal", async () => {
    const sidebar = mountSidebar();
    const dialog = await openAndLoad(sidebar);
    const wrapper = dialog.querySelector<HTMLElement>("[data-form-keyboard]")!;
    const outsideKeydown = vi.fn();
    dialog.host.addEventListener("keydown", outsideKeydown);

    wrapper.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "j",
        bubbles: true,
        cancelable: true,
        composed: true,
      }),
    );

    expect(outsideKeydown).not.toHaveBeenCalled();
  });

  it("allows normal-mode action button activation while containing its key events", async () => {
    const sidebar = mountSidebar();
    const dialog = await openAndLoad(sidebar);
    await chooseProject(dialog, "PlaneAI");
    const outsideKeydown = vi.fn();
    dialog.host.addEventListener("keydown", outsideKeydown);
    const newProject = [...dialog.querySelectorAll<HTMLButtonElement>("button")].find(
      (button) => button.textContent === "New Project (N)",
    )!;
    const assign = dialog.querySelector<HTMLButtonElement>("button.primary")!;

    for (const button of [newProject, assign]) {
      for (const key of ["Enter", " "]) {
        const event = new KeyboardEvent("keydown", {
          key,
          bubbles: true,
          cancelable: true,
          composed: true,
        });
        button.dispatchEvent(event);
        expect(event.defaultPrevented).toBe(false);
      }
    }
    expect(outsideKeydown).not.toHaveBeenCalled();
  });

  it("keeps combobox options out of tab order and announces the highlighted project", async () => {
    const sidebar = mountSidebar({
      projects: [
        { id: "project-1", name: "PlaneAI", path: "/planeai", hidden: false },
        { id: "project-2", name: "Archive cleanup", path: "/archive", hidden: false },
      ],
    });
    const dialog = await openAndLoad(sidebar);
    const picker = dialog.querySelector<HTMLInputElement>("input[aria-label='PlaneAI project']")!;

    picker.dispatchEvent(new FocusEvent("focus", { bubbles: true }));
    expect(
      [...dialog.querySelectorAll<HTMLButtonElement>("[role='option']")].every(
        (option) => option.tabIndex === -1,
      ),
    ).toBe(true);
    expect(picker.getAttribute("aria-activedescendant")).toBe(
      dialog.querySelector<HTMLElement>("[role='option'][data-highlighted='true']")?.id,
    );

    const originalScrollIntoView = HTMLElement.prototype.scrollIntoView;
    HTMLElement.prototype.scrollIntoView = vi.fn();
    try {
      picker.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }));
      expect(picker.getAttribute("aria-activedescendant")).toBe(
        dialog.querySelector<HTMLElement>("[role='option'][data-highlighted='true']")?.id,
      );
    } finally {
      HTMLElement.prototype.scrollIntoView = originalScrollIntoView;
    }
  });

  it("scrolls the keyboard-highlighted project option into view", async () => {
    const scrollIntoView = vi.fn();
    const originalScrollIntoView = HTMLElement.prototype.scrollIntoView;
    HTMLElement.prototype.scrollIntoView = scrollIntoView;
    try {
      const sidebar = mountSidebar({
        projects: Array.from({ length: 20 }, (_, index) => ({
          id: `project-${index + 1}`,
          name: `Project ${index + 1}`,
          path: `/project-${index + 1}`,
          hidden: false,
        })),
      });
      const dialog = await openAndLoad(sidebar);
      const picker = dialog.querySelector<HTMLInputElement>("input[aria-label='PlaneAI project']")!;
      picker.dispatchEvent(new FocusEvent("focus", { bubbles: true }));
      picker.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }));

      expect(scrollIntoView).toHaveBeenCalledWith({ block: "nearest" });
      expect(
        dialog.querySelector<HTMLButtonElement>("[role='option'][data-highlighted='true']")
          ?.textContent,
      ).toBe("Project 2");
    } finally {
      HTMLElement.prototype.scrollIntoView = originalScrollIntoView;
    }
  });

  it("shows submission errors without closing the assignment dialog", async () => {
    const createChild = vi.fn().mockRejectedValue(new Error("Task database unavailable"));
    const sidebar = mountSidebar({ createChild });
    const dialog = await openAndLoad(sidebar);
    await chooseProject(dialog, "PlaneAI");
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
      expect(
        dialog.querySelector<HTMLInputElement>("input[aria-label='PlaneAI project']"),
      ).toBeTruthy(),
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
    repeatOptions.mount(second, {
      close: sidebar.close,
      dispose: sidebar.dispose,
      setSubmitting: sidebar.setSubmitting,
    });
    await vi.waitFor(() =>
      expect(
        second.querySelector<HTMLInputElement>("input[aria-label='PlaneAI project']"),
      ).toBeTruthy(),
    );
    await chooseProject(second, "New Project");
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

describe("jiraDepartedInteractionEntrypoint", () => {
  let host: HTMLElement | undefined;

  afterEach(() => host?.remove());

  function mountInteraction(
    resolve: () => Promise<unknown> = () => Promise.resolve({ dequeued: true }),
  ) {
    host = document.createElement("div");
    const root = host.attachShadow({ mode: "open" });
    let items = [{ key: "PLA-42", summary: "A departed issue" }];
    const notify = vi.fn();
    const call = vi.fn(async (method: string) => {
      if (method === "jira.departures.list") return { items };
      if (method === "jira.departures.resolve") {
        await resolve();
        items = [];
        return { dequeued: true };
      }
      if (method === "jira.departures.dequeue") {
        items = [];
        return { dequeued: true };
      }
      throw new Error(`unexpected Jira call: ${method}`);
    });
    jiraDepartedInteractionEntrypoint.mount(root, {
      plugin: { id: "jira" },
      contribution: { id: "departed-interaction" },
      host: {
        call,
        navigation: { open: vi.fn(), close: vi.fn(), openPreferences: vi.fn() },
        sidebar: { register: vi.fn(() => () => {}), select: vi.fn() },
        data: { changed: vi.fn(), notify },
      },
    } as unknown as PluginUiContext);
    return { root, call, notify };
  }

  it("does not steal focus and resolves D through the sidecar", async () => {
    const before = document.activeElement;
    const interaction = mountInteraction();
    await vi.waitFor(() => expect(interaction.root.textContent).toContain("PLA-42"));
    expect(document.activeElement).toBe(before);
    const surface = interaction.root.querySelector<HTMLElement>(".interaction")!;
    surface.dispatchEvent(new KeyboardEvent("keydown", { key: "d", bubbles: true }));
    await vi.waitFor(() =>
      expect(interaction.call).toHaveBeenCalledWith("jira.departures.resolve", { key: "PLA-42" }),
    );
    await vi.waitFor(() => expect(interaction.root.textContent).not.toContain("PLA-42"));
  });

  it("keeps the item queued when Done fails, while N and Escape dismiss it", async () => {
    const failed = mountInteraction(() => Promise.reject(new Error("task update failed")));
    await vi.waitFor(() => expect(failed.root.textContent).toContain("PLA-42"));
    const surface = failed.root.querySelector<HTMLElement>(".interaction")!;
    surface.dispatchEvent(new KeyboardEvent("keydown", { key: "d", bubbles: true }));
    await vi.waitFor(() =>
      expect(failed.notify).toHaveBeenCalledWith(expect.stringContaining("task update failed")),
    );
    expect(failed.call).not.toHaveBeenCalledWith("jira.departures.dequeue", expect.anything());
    surface.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    await vi.waitFor(() =>
      expect(failed.call).toHaveBeenCalledWith("jira.departures.dequeue", { key: "PLA-42" }),
    );
  });
});
