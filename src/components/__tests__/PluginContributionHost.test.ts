import { afterEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";

const { pluginCall, localUiSource } = vi.hoisted(() => ({
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
}));

vi.mock("../../lib/api", () => ({
  plugins: { call: pluginCall, localUiSource },
}));

import PluginContributionHostHarness from "./PluginContributionHostHarness.svelte";
import PluginContributionHostLocalHarness from "./PluginContributionHostLocalHarness.svelte";

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
  });

  it("mounts the Jira UI in a host-owned Shadow DOM root", async () => {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(PluginContributionHostHarness, { target }) as typeof component;

    await vi.waitFor(() => {
      const host = target.querySelector<HTMLElement>("[data-plugin-ui-contribution]");
      expect(host?.shadowRoot).toBeTruthy();
      expect(host?.shadowRoot?.querySelector(".plugin-page")).toBeTruthy();
      expect(host?.shadowRoot?.textContent).toContain("planeai.plugin-host.v1");
      expect(host?.shadowRoot?.textContent).toContain("running");
    });
    expect(pluginCall).toHaveBeenCalledWith("jira", "jira.status", null);
  });

  it("mounts an imported local ESM bundle and scopes calls to its sidecar", async () => {
    localUiSource.mockResolvedValue(`
      export default {
        mount(root, context) {
          const page = document.createElement("p");
          page.className = "fixture-page";
          page.textContent = "Loading…";
          root.replaceChildren(page);
          context.host.call("fixture.status").then((value) => { page.textContent = value.runtime_state; });
          return () => root.replaceChildren();
        }
      };
    `);
    pluginCall.mockResolvedValueOnce({ runtime_state: "running" } as never);
    target = document.createElement("div");
    document.body.append(target);
    component = mount(PluginContributionHostLocalHarness, { target }) as typeof component;

    await vi.waitFor(() => {
      const host = target.querySelector<HTMLElement>("[data-plugin-ui-contribution]");
      expect(host?.shadowRoot?.querySelector(".fixture-page")?.textContent).toBe("running");
    });
    expect(localUiSource).toHaveBeenCalledWith("local-fixture", "fixture");
    expect(pluginCall).toHaveBeenCalledWith("local-fixture", "fixture.status", null);
  });

  it("disposes the old UI before remounting and on host destruction", async () => {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(PluginContributionHostHarness, { target }) as typeof component;
    await vi.waitFor(() => {
      const host = target.querySelector<HTMLElement>("[data-plugin-ui-contribution]");
      expect(host?.shadowRoot?.textContent).toContain("Jira");
    });

    const current = component;
    if (!current) throw new Error("plugin workspace harness did not mount");
    current.reload();
    await vi.waitFor(() => {
      const host = target.querySelector<HTMLElement>("[data-plugin-ui-contribution]");
      expect(host?.shadowRoot?.textContent).toContain("Jira Reloaded");
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
      ).toContain("Jira");
    });

    const current = component;
    if (!current) throw new Error("plugin workspace harness did not mount");
    current.setState("stopping");

    const host = target.querySelector<HTMLElement>("[data-plugin-ui-contribution]")!;
    await vi.waitFor(() => expect(host.shadowRoot?.childNodes).toHaveLength(0));
    expect(pluginCall).toHaveBeenCalledTimes(1);
  });
});
