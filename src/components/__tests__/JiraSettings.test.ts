import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";

const { call, enable, list, openJiraAuthorizationUrl, settings, updateSettings } = vi.hoisted(
  () => ({
    call: vi.fn(),
    enable: vi.fn(),
    list: vi.fn(),
    openJiraAuthorizationUrl: vi.fn(),
    settings: vi.fn(),
    updateSettings: vi.fn(),
  }),
);

vi.mock("../../lib/api", () => ({
  plugins: { call, enable, list, openJiraAuthorizationUrl, settings, updateSettings },
}));

vi.mock("../../lib/snackbar.svelte", () => ({ showSnackbar: vi.fn() }));

import JiraSettings from "../JiraSettings.svelte";

describe("JiraSettings", () => {
  let target: HTMLElement;
  let component: ReturnType<typeof mount> | undefined;

  beforeEach(() => {
    vi.stubGlobal("crypto", { randomUUID: () => "attempt-1" });
    list.mockResolvedValue([{ id: "jira", state: "running" }]);
    settings.mockResolvedValue({ site: "https://example.atlassian.net", sync_interval_ms: 60000 });
    updateSettings.mockResolvedValue({
      site: "https://example.atlassian.net",
      sync_interval_ms: 60000,
    });
    call.mockImplementation((_pluginId: string, method: string) => {
      if (method === "jira.status") {
        return Promise.resolve({
          connected: false,
          authorizing: false,
          site: null,
          last_error: null,
        });
      }
      if (method === "jira.connect.cancel") return Promise.resolve({ cancelled: true });
      return Promise.resolve({});
    });
    target = document.createElement("div");
    document.body.append(target);
  });

  afterEach(() => {
    if (component) unmount(component);
    target?.remove();
    vi.unstubAllGlobals();
    vi.clearAllMocks();
  });

  it("cancels the same attempt when cancellation races an in-flight start", async () => {
    let resolveStart: ((value: { authorization_url: string }) => void) | undefined;
    call.mockImplementation((_pluginId: string, method: string) => {
      if (method === "jira.status") {
        return Promise.resolve({
          connected: false,
          authorizing: false,
          site: null,
          last_error: null,
        });
      }
      if (method === "jira.connect.start") {
        return new Promise((resolve) => {
          resolveStart = resolve;
        });
      }
      if (method === "jira.connect.cancel") return Promise.resolve({ cancelled: true });
      return Promise.resolve({});
    });
    component = mount(JiraSettings, { target });

    await vi.waitFor(() => {
      const connect = Array.from(target.querySelectorAll("button")).find(
        (button) => button.textContent === "Connect",
      ) as HTMLButtonElement | undefined;
      expect(connect?.disabled).toBe(false);
    });
    (
      Array.from(target.querySelectorAll("button")).find(
        (button) => button.textContent === "Connect",
      ) as HTMLButtonElement
    ).click();
    await vi.waitFor(() =>
      expect(call).toHaveBeenCalledWith("jira", "jira.connect.start", { attempt_id: "attempt-1" }),
    );

    (
      Array.from(target.querySelectorAll("button")).find(
        (button) => button.textContent === "Cancel authorization",
      ) as HTMLButtonElement
    ).click();
    await vi.waitFor(() =>
      expect(call).toHaveBeenCalledWith("jira", "jira.connect.cancel", { attempt_id: "attempt-1" }),
    );
    resolveStart?.({ authorization_url: "https://auth.example.test" });

    await vi.waitFor(() => {
      const cancellations = call.mock.calls.filter(
        ([, method]) => method === "jira.connect.cancel",
      );
      expect(cancellations).toHaveLength(2);
      expect(
        cancellations.every(
          ([pluginId, _method, params]) =>
            pluginId === "jira" && params?.attempt_id === "attempt-1",
        ),
      ).toBe(true);
    });
    expect(openJiraAuthorizationUrl).not.toHaveBeenCalled();
    expect(call).not.toHaveBeenCalledWith("jira", "jira.connect.complete", expect.anything());
  });
});
