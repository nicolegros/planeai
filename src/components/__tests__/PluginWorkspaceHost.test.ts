import { afterEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";

const { jiraStatus } = vi.hoisted(() => ({
  jiraStatus: vi.fn(() =>
    Promise.resolve({
      plugin_id: "jira",
      plugin_name: "Jira",
      plugin_version: "0.1.0",
      host_api_version: "planeai.plugin-host.v1",
      runtime_state: "running",
      last_error: null,
    }),
  ),
}));

vi.mock("../../lib/api", () => ({ plugins: { jiraStatus } }));

import PluginWorkspaceHostHarness from "./PluginWorkspaceHostHarness.svelte";

describe("PluginWorkspaceHost", () => {
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
    component = mount(PluginWorkspaceHostHarness, { target }) as typeof component;

    await vi.waitFor(() => {
      const host = target.querySelector<HTMLElement>("[data-plugin-workspace-host]");
      expect(host?.shadowRoot).toBeTruthy();
      expect(host?.shadowRoot?.querySelector(".plugin-page")).toBeTruthy();
      expect(host?.shadowRoot?.textContent).toContain("planeai.plugin-host.v1");
      expect(host?.shadowRoot?.textContent).toContain("running");
    });
    expect(jiraStatus).toHaveBeenCalledWith("jira");
  });

  it("disposes the old UI before remounting and on host destruction", async () => {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(PluginWorkspaceHostHarness, { target }) as typeof component;
    await vi.waitFor(() => {
      const host = target.querySelector<HTMLElement>("[data-plugin-workspace-host]");
      expect(host?.shadowRoot?.textContent).toContain("Jira");
    });

    const current = component;
    if (!current) throw new Error("plugin workspace harness did not mount");
    current.reload();
    await vi.waitFor(() => {
      const host = target.querySelector<HTMLElement>("[data-plugin-workspace-host]");
      expect(host?.shadowRoot?.textContent).toContain("Jira Reloaded");
      expect(host?.shadowRoot?.querySelectorAll("style")).toHaveLength(1);
    });

    const host = target.querySelector<HTMLElement>("[data-plugin-workspace-host]")!;
    unmount(current);
    component = undefined;
    expect(host.shadowRoot?.childNodes).toHaveLength(0);
  });

  it("disposes without remounting when the runtime leaves running", async () => {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(PluginWorkspaceHostHarness, { target }) as typeof component;
    await vi.waitFor(() => {
      expect(
        target.querySelector<HTMLElement>("[data-plugin-workspace-host]")?.shadowRoot?.textContent,
      ).toContain("Jira");
    });

    const current = component;
    if (!current) throw new Error("plugin workspace harness did not mount");
    current.setState("stopping");

    const host = target.querySelector<HTMLElement>("[data-plugin-workspace-host]")!;
    await vi.waitFor(() => expect(host.shadowRoot?.childNodes).toHaveLength(0));
    expect(jiraStatus).toHaveBeenCalledTimes(1);
  });
});
