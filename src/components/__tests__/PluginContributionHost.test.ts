import { afterEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
const { pluginCall, localUiSource, settingsGet, updateSettings, dataChanged, eventListeners } =
  vi.hoisted(() => ({
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

import PluginContributionHostHarness from "./PluginContributionHostHarness.svelte";
import PluginContributionHostLocalHarness from "./PluginContributionHostLocalHarness.svelte";

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

  afterEach(() => {
    if (component) unmount(component);
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
      expect(frame?.srcdoc).not.toContain("sidebar-keydown");
    });
    // jsdom does not execute iframe srcdoc. The production bridge loads source only after its frame loads.
    expect(localUiSource).not.toHaveBeenCalled();
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
