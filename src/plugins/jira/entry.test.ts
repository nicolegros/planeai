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

  afterEach(() => host?.remove());

  it("requests unified selection when an issue is clicked", async () => {
    host = document.createElement("div");
    const root = host.attachShadow({ mode: "open" });
    const select = vi.fn();
    const register = vi.fn(() => () => {});
    const call = vi.fn((method: string) => {
      if (method === "jira.sidebar.items") {
        return Promise.resolve({
          items: [
            { key: "PLA-42", title: "Synchronize the sidebar", status: "todo", child_count: 0 },
          ],
        });
      }
      return Promise.reject(new Error(`unexpected Jira call: ${method}`));
    });

    jiraSidebarSectionEntrypoint.mount(root, {
      plugin: { id: "jira" },
      contribution: { id: "jira-sidebar-section" },
      host: {
        call,
        navigation: { open: vi.fn(), close: vi.fn(), openPreferences: vi.fn() },
        sidebar: { register, select },
        data: { changed: vi.fn() },
      },
    } as unknown as PluginUiContext);

    await vi.waitFor(() => expect(root.querySelector<HTMLButtonElement>(".issue")).toBeTruthy());
    const issue = root.querySelector<HTMLButtonElement>(".issue")!;
    expect(issue.getAttribute("aria-label")).toBe("PLA-42: Synchronize the sidebar. Status: To do");
    const header = root.querySelector<HTMLButtonElement>(".section-header")!;
    expect(header.getAttribute("aria-expanded")).toBe("true");
    expect(root.querySelector("#jira-sidebar-issues")).toBeTruthy();
    header.click();
    await vi.waitFor(() =>
      expect(root.querySelector(".section-header")?.getAttribute("aria-expanded")).toBe("false"),
    );
    header.click();
    await vi.waitFor(() => expect(root.querySelector<HTMLButtonElement>(".issue")).toBeTruthy());
    root.querySelector<HTMLButtonElement>(".issue")?.click();

    expect(select).toHaveBeenCalledWith("issue:PLA-42");
    await vi.waitFor(() => {
      expect(root.querySelectorAll(".selected")).toHaveLength(1);
      expect(root.querySelector(".issue.selected")?.textContent).toContain("PLA-42");
    });
  });
});
