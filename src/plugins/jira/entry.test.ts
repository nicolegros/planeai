import { afterEach, describe, expect, it, vi } from "vitest";
import { jiraSidebarSectionEntrypoint } from "./entry";
import type { PluginUiContext } from "../../lib/plugin-sdk";

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
    root.querySelector<HTMLButtonElement>(".issue")?.click();

    expect(select).toHaveBeenCalledWith("issue:PLA-42");
    await vi.waitFor(() => {
      expect(root.querySelectorAll(".selected")).toHaveLength(1);
      expect(root.querySelector(".issue.selected")?.textContent).toContain("PLA-42");
    });
  });
});
