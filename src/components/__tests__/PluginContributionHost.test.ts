import { afterEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
const {
  pluginCall,
  localUiSource,
  settingsGet,
  updateSettings,
  dataChanged,
  eventListeners,
  showSnackbar,
} = vi.hoisted(() => ({
  pluginCall: vi.fn(() =>
    Promise.resolve({
      plugin_id: "jira",
      plugin_name: "Jira",
      plugin_version: "0.1.0",
      host_api_version: "planeai.plugin-host.v1",
      runtime_state: "running",
      last_error: null,
    }),
  ),
  localUiSource: vi.fn(),
  settingsGet: vi.fn(() => Promise.resolve({ greeting: "Saved greeting" })),
  updateSettings: vi.fn((_: string, settings: unknown) => Promise.resolve(settings)),
  dataChanged: vi.fn(() => Promise.resolve()),
  eventListeners: new Map<string, (event: { payload: string }) => void>(),
  showSnackbar: vi.fn(),
}));

vi.mock("../../lib/api", () => ({
  plugins: {
    call: pluginCall,
    localUiSource,
    settings: settingsGet,
    updateSettings,
    dataChanged,
  },
  pr: { getPrStatus: vi.fn(), getPrComments: vi.fn() },
}));

vi.mock("../../lib/snackbar.svelte", () => ({
  showSnackbar,
}));

import PluginContributionHost from "../PluginContributionHost.svelte";
import PluginContributionHostHarness from "./PluginContributionHostHarness.svelte";
import PluginContributionHostLocalHarness from "./PluginContributionHostLocalHarness.svelte";
import { getPluginSidebarRows } from "../../lib/plugin-sidebar-navigation.svelte";
import type { PluginInventory, PluginUiContribution } from "../../lib/types";
import { focusTerminal, getActiveZone } from "../../lib/focus.svelte";
import { shouldBypassSidebarKeyboard } from "../../lib/sidebar-nav.svelte";

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn((eventName: string, handler: (event: { payload: string }) => void) => {
    eventListeners.set(eventName, handler);
    return Promise.resolve(() => eventListeners.delete(eventName));
  }),
}));

describe("PluginContributionHost", () => {
  let target: HTMLElement;
  let component:
    | {
        reload(name?: string): void;
        setState(state: "disabled" | "starting" | "running" | "stopping" | "error"): void;
      }
    | undefined;
  let interactionComponent: ReturnType<typeof mount> | undefined;

  afterEach(() => {
    if (component) unmount(component);
    if (interactionComponent) unmount(interactionComponent);
    component = undefined;
    interactionComponent = undefined;
    target?.remove();
    vi.clearAllMocks();
    eventListeners.clear();
  });

  it("mounts the Jira UI in a host-owned Shadow DOM root", async () => {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(PluginContributionHostHarness, { target }) as typeof component;

    await vi.waitFor(() => {
      const host = target.querySelector<HTMLElement>("[data-plugin-ui-contribution]");
      expect(host?.shadowRoot).toBeTruthy();
      expect(host?.shadowRoot?.querySelector(".page")).toBeTruthy();
      expect(host?.shadowRoot?.textContent).toContain("Jira connection");
      expect(host?.shadowRoot?.textContent).toContain("Not connected");
    });
    expect(pluginCall).toHaveBeenCalledWith("jira", "jira.status", null);
  });

  it("leaves an idle built-in interaction host transparent to pointer input", async () => {
    target = document.createElement("div");
    document.body.append(target);
    const plugin: PluginInventory = {
      id: "jira",
      name: "Jira",
      version: "0.1.0",
      host_api_version: "planeai.plugin-host.v1",
      source_kind: "builtin",
      backend_entrypoint: "planeai-plugin-jira",
      capabilities: [],
      ui_contributions: [],
      installed_hash: null,
      installed_path: null,
      original_display_path: null,
      enabled: true,
      state: "running",
      last_error: null,
      log_path: null,
    };
    const contribution: PluginUiContribution = {
      id: "jira-departed-interaction",
      label: "Departed Jira issues",
      placement: "interaction",
      entrypoint: "jira-departed-interaction",
      order: null,
      shortcut: null,
    };
    interactionComponent = mount(PluginContributionHost, {
      target,
      props: { plugin, contribution, onNavigate: () => {}, onClose: () => {} },
    });

    const host = await vi.waitFor(() => {
      const next = target.querySelector<HTMLElement>("[data-plugin-ui-contribution]");
      expect(next?.shadowRoot?.querySelector("[data-plugin-interaction]")).toBeTruthy();
      return next!;
    });

    expect(host.className).toContain("pointer-events-none");
    expect(host.shadowRoot?.querySelector("style")?.textContent).toContain(".interaction");
    expect(host.shadowRoot?.querySelector("style")?.textContent).toContain("pointer-events:auto");
  });

  it("isolates local UI bundles in an opaque, script-only iframe with a message bridge", async () => {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(PluginContributionHostLocalHarness, { target }) as typeof component;

    await vi.waitFor(() => {
      const host = target.querySelector<HTMLElement>("[data-plugin-ui-contribution]");
      const frame = host?.shadowRoot?.querySelector<HTMLIFrameElement>("iframe");
      expect(frame).toBeTruthy();
      expect(frame?.getAttribute("sandbox")).toBe("allow-scripts");
      expect(frame?.srcdoc).toContain("default-src 'none'");
      expect(frame?.srcdoc).toContain("script-src 'unsafe-inline' blob:");
      expect(frame?.srcdoc).toContain("postMessage");
      expect(frame?.srcdoc).toContain("settings-get");
      expect(frame?.srcdoc).toContain("settings-replace");
      expect(frame?.srcdoc).toContain("sidebar-keydown");
      expect(frame?.srcdoc).toContain("sidebarNavigationKeys");
      expect(frame?.srcdoc).toContain('addEventListener("keydown", forwardSidebarKeydown)');
    });
    // jsdom does not execute iframe srcdoc. The production bridge loads source only after its frame loads.
    expect(localUiSource).not.toHaveBeenCalled();
  });

  it("uses a compact iframe for local sidebar footers while retaining section space", async () => {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(PluginContributionHostLocalHarness, {
      target,
      props: { placement: "sidebar.footer" },
    }) as typeof component;

    const footerFrame = await vi.waitFor(() => {
      const next = target
        .querySelector<HTMLElement>("[data-plugin-ui-contribution]")
        ?.shadowRoot?.querySelector<HTMLIFrameElement>("iframe");
      expect(next).toBeTruthy();
      return next!;
    });

    expect(footerFrame.className).not.toContain("h-full");
    expect(footerFrame.style.height).toBe("34px");

    unmount(component!);
    target.replaceChildren();

    component = mount(PluginContributionHostLocalHarness, { target }) as typeof component;
    const sectionFrame = await vi.waitFor(() => {
      const next = target
        .querySelector<HTMLElement>("[data-plugin-ui-contribution]")
        ?.shadowRoot?.querySelector<HTMLIFrameElement>("iframe");
      expect(next).toBeTruthy();
      return next!;
    });
    expect(sectionFrame.style.height).toBe("160px");
  });

  it("fills the main-pane host with a local plugin iframe", async () => {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(PluginContributionHostLocalHarness, {
      target,
      props: { placement: "main-pane" },
    }) as typeof component;

    const frame = await vi.waitFor(() => {
      const next = target
        .querySelector<HTMLElement>("[data-plugin-ui-contribution]")
        ?.shadowRoot?.querySelector<HTMLIFrameElement>("iframe");
      expect(next).toBeTruthy();
      return next!;
    });
    expect(frame.className).toContain("h-full");
  });

  it("treats a local sidebar footer as sidebar content for keyboard navigation", async () => {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(PluginContributionHostLocalHarness, {
      target,
      props: { placement: "sidebar.footer" },
    }) as typeof component;

    const host = await vi.waitFor(() => {
      const next = target.querySelector<HTMLElement>("[data-plugin-ui-contribution]");
      expect(next).toBeTruthy();
      return next!;
    });

    expect(host.dataset.pluginSidebarContribution).toBe("");
    expect(shouldBypassSidebarKeyboard(host)).toBe(false);
  });

  it("sends only structured-cloneable context when initializing a local UI iframe", async () => {
    localUiSource.mockResolvedValue("export default { mount() { return () => {}; } };");
    target = document.createElement("div");
    document.body.append(target);
    component = mount(PluginContributionHostLocalHarness, { target }) as typeof component;

    const frame = await vi.waitFor(() => {
      const next = target
        .querySelector<HTMLElement>("[data-plugin-ui-contribution]")
        ?.shadowRoot?.querySelector<HTMLIFrameElement>("iframe");
      expect(next).toBeTruthy();
      return next!;
    });
    const postMessage = vi.fn((message: unknown) => {
      structuredClone(message);
    });
    Object.defineProperty(frame, "contentWindow", {
      configurable: true,
      value: { postMessage },
    });

    frame.dispatchEvent(new Event("load"));
    await vi.waitFor(() => expect(localUiSource).toHaveBeenCalledWith("local-fixture", "fixture"));
    await vi.waitFor(() =>
      expect(postMessage).toHaveBeenCalledWith(expect.objectContaining({ type: "init" }), "*"),
    );
    expect(
      target.querySelector<HTMLElement>("[data-plugin-ui-contribution]")?.shadowRoot?.textContent,
    ).not.toContain("Failed to load");
  });

  it("registers local sidebar rows under the contribution key used by UnifiedSidebar", async () => {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(PluginContributionHostLocalHarness, { target }) as typeof component;

    const frame = await vi.waitFor(() => {
      const next = target
        .querySelector<HTMLElement>("[data-plugin-ui-contribution]")
        ?.shadowRoot?.querySelector<HTMLIFrameElement>("iframe");
      expect(next).toBeTruthy();
      return next!;
    });

    window.dispatchEvent(
      new MessageEvent("message", {
        source: frame.contentWindow,
        data: {
          type: "sidebar-register",
          registrationId: "registration:1",
          rows: [{ id: "log-entry" }],
        },
      }),
    );

    await vi.waitFor(() =>
      expect(getPluginSidebarRows("local-fixture:fixture").map((row) => row.id)).toEqual([
        "log-entry",
      ]),
    );
  });

  it("focuses the shared sidebar when a local sidebar iframe is entered", async () => {
    focusTerminal();
    target = document.createElement("div");
    document.body.append(target);
    component = mount(PluginContributionHostLocalHarness, { target }) as typeof component;

    const frame = await vi.waitFor(() => {
      const next = target
        .querySelector<HTMLElement>("[data-plugin-ui-contribution]")
        ?.shadowRoot?.querySelector<HTMLIFrameElement>("iframe");
      expect(next).toBeTruthy();
      return next!;
    });

    frame.dispatchEvent(new FocusEvent("focus"));
    expect(getActiveZone()).toBe("sidebar");
  });

  it("forwards local sidebar keyboard commands to the shared sidebar event boundary", async () => {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(PluginContributionHostLocalHarness, { target }) as typeof component;

    const host = await vi.waitFor(() => {
      const next = target.querySelector<HTMLElement>("[data-plugin-ui-contribution]");
      expect(next?.shadowRoot?.querySelector("iframe")).toBeTruthy();
      return next!;
    });
    const frame = host.shadowRoot!.querySelector<HTMLIFrameElement>("iframe")!;
    const handleSidebarKeydown = vi.fn((event: Event) => {
      const detail = (event as CustomEvent<{ event: KeyboardEvent; handled: boolean }>).detail;
      expect(detail.event.key).toBe("j");
      detail.event.preventDefault();
      detail.handled = detail.event.defaultPrevented;
    });
    host.addEventListener("plugin-sidebar-keydown", handleSidebarKeydown);

    window.dispatchEvent(
      new MessageEvent("message", {
        source: frame.contentWindow,
        data: { type: "sidebar-keydown", key: "j" },
      }),
    );

    await vi.waitFor(() => expect(handleSidebarKeydown).toHaveBeenCalledOnce());
    host.removeEventListener("plugin-sidebar-keydown", handleSidebarKeydown);
  });

  it("ignores blank local plugin notifications", async () => {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(PluginContributionHostLocalHarness, { target }) as typeof component;

    const frame = await vi.waitFor(() => {
      const next = target
        .querySelector<HTMLElement>("[data-plugin-ui-contribution]")
        ?.shadowRoot?.querySelector<HTMLIFrameElement>("iframe");
      expect(next).toBeTruthy();
      return next!;
    });

    window.dispatchEvent(
      new MessageEvent("message", {
        source: frame.contentWindow,
        data: { type: "notify", message: "   ", kind: "error" },
      }),
    );

    expect(showSnackbar).not.toHaveBeenCalled();
  });

  it("disposes the old UI before remounting and on host destruction", async () => {
    target = document.createElement("div");
    document.body.append(target);

    component = mount(PluginContributionHostHarness, { target }) as typeof component;
    await vi.waitFor(() => {
      const host = target.querySelector<HTMLElement>("[data-plugin-ui-contribution]");
      expect(host?.shadowRoot?.textContent).toContain("Jira connection");
    });

    const current = component;
    if (!current) throw new Error("plugin workspace harness did not mount");
    current.reload();
    await vi.waitFor(() => {
      const host = target.querySelector<HTMLElement>("[data-plugin-ui-contribution]");
      expect(host?.shadowRoot?.querySelector(".page")).toBeTruthy();
      expect(host?.shadowRoot?.querySelectorAll("style")).toHaveLength(1);
    });

    const host = target.querySelector<HTMLElement>("[data-plugin-ui-contribution]")!;
    unmount(current);
    component = undefined;
    expect(host.shadowRoot?.childNodes).toHaveLength(0);
  });

  it("disposes without remounting when the runtime leaves running", async () => {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(PluginContributionHostHarness, { target }) as typeof component;
    await vi.waitFor(() => {
      expect(
        target.querySelector<HTMLElement>("[data-plugin-ui-contribution]")?.shadowRoot?.textContent,
      ).toContain("Jira connection");
    });

    const current = component;
    if (!current) throw new Error("plugin workspace harness did not mount");
    current.setState("stopping");

    const host = target.querySelector<HTMLElement>("[data-plugin-ui-contribution]")!;
    await vi.waitFor(() => expect(host.shadowRoot?.childNodes).toHaveLength(0));
    expect(pluginCall).toHaveBeenCalledTimes(2);
  });
});
