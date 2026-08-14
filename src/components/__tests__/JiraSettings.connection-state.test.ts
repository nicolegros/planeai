import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, tick, unmount } from "svelte";

const jiraStatus = vi.hoisted(() => vi.fn());
const tauriEvent = vi.hoisted(() => ({
  handler: undefined as ((event: unknown) => void) | undefined,
  listen: vi.fn(),
}));

vi.mock("../../lib/api", () => ({
  jira: {
    status: jiraStatus,
    connect: vi.fn(),
    disconnect: vi.fn(),
    syncNow: vi.fn(),
  },
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: tauriEvent.listen,
}));

vi.mock("../../lib/settings.svelte", () => ({
  getSettings: () => ({
    integrations: { jira: { site: "https://example.atlassian.net", sources: {} } },
  }),
  updateSettings: vi.fn(),
}));

vi.mock("../../lib/snackbar.svelte", () => ({
  showSnackbar: vi.fn(),
}));

import JiraSettings from "../JiraSettings.svelte";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((resolvePromise) => {
    resolve = resolvePromise;
  });
  return { promise, resolve };
}

describe("JiraSettings connection state", () => {
  let component: Record<string, unknown> | null = null;
  let target: HTMLElement;

  beforeEach(() => {
    vi.clearAllMocks();
    tauriEvent.handler = undefined;
    tauriEvent.listen.mockImplementation((_event, handler) => {
      tauriEvent.handler = handler;
      return Promise.resolve(() => {});
    });
    target = document.createElement("div");
    document.body.appendChild(target);
  });

  afterEach(() => {
    if (component) unmount(component);
    target.remove();
  });

  it("keeps reconnect-required state when an earlier status request finishes late", async () => {
    const initial = deferred<{ connected: boolean; site: string | null }>();
    jiraStatus
      .mockImplementationOnce(() => initial.promise)
      .mockResolvedValueOnce({
        connected: false,
        site: null,
      });

    component = mount(JiraSettings, { target });
    await tick();
    await Promise.resolve();

    expect(tauriEvent.handler).toBeDefined();
    expect(jiraStatus).toHaveBeenCalledTimes(1);

    tauriEvent.handler?.({});
    await Promise.resolve();
    await tick();

    initial.resolve({ connected: true, site: "https://example.atlassian.net" });
    await Promise.resolve();
    await tick();

    expect(target.querySelector("[role='status']")?.textContent).toContain("Not connected");
  });

  it("keeps user disconnect state when an earlier status request finishes late", async () => {
    const delayedStatus = deferred<{ connected: boolean; site: string | null }>();
    jiraStatus
      .mockResolvedValueOnce({
        connected: true,
        site: "https://example.atlassian.net",
      })
      .mockImplementationOnce(() => delayedStatus.promise);

    component = mount(JiraSettings, { target });
    await tick();
    await Promise.resolve();
    await tick();

    tauriEvent.handler?.({});
    await Promise.resolve();

    (
      target.querySelector("button[aria-label='Disconnect from Jira']") as HTMLButtonElement
    ).click();
    await tick();

    delayedStatus.resolve({ connected: true, site: "https://example.atlassian.net" });
    await Promise.resolve();
    await tick();

    expect(target.querySelector("[role='status']")?.textContent).toContain("Not connected");
  });
});
