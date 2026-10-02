import { afterEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";

const { localUiSource, localProviderUiSource, ensure, send, interrupt, eventListeners } =
  vi.hoisted(() => ({
    localUiSource: vi.fn(() => Promise.resolve("export default { mount() { return () => {}; } };")),
    localProviderUiSource: vi.fn(() =>
      Promise.resolve("export default { mount() { return () => {}; } };"),
    ),
    ensure: vi.fn(() => Promise.resolve()),
    send: vi.fn(() => Promise.resolve()),
    interrupt: vi.fn(() => Promise.resolve()),
    eventListeners: new Map<string, (event: { payload: unknown }) => void>(),
  }));

vi.mock("../../lib/api", () => ({
  plugins: {
    call: vi.fn(),
    hostCall: vi.fn(),
    localUiSource,
    localProviderUiSource,
    settings: vi.fn(),
    updateSettings: vi.fn(),
    dataChanged: vi.fn(),
  },
  providerSessions: { ensure, send, interrupt },
  projects: { list: vi.fn() },
  tasks: { create: vi.fn() },
}));
vi.mock("../../lib/snackbar.svelte", () => ({ showSnackbar: vi.fn() }));
vi.mock("../../lib/settings.svelte", () => ({ isDark: () => false }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn((eventName: string, handler: (event: { payload: unknown }) => void) => {
    eventListeners.set(eventName, handler);
    return Promise.resolve(() => eventListeners.delete(eventName));
  }),
}));

import PluginContributionHost from "../PluginContributionHost.svelte";
import { providerContribution } from "../../lib/plugin-providers";
import type { PluginSessionContext } from "../../lib/plugin-sdk";
import type { PluginInventory, PluginUiContribution } from "../../lib/types";

const provider = { id: "echo", label: "Echo", entrypoint: "ui/chat.js", supports: ["yolo"] };
const plugin: PluginInventory = {
  id: "local-fixture",
  name: "Local Fixture",
  version: "0.1.0",
  host_api_version: "planeai.plugin-host.v3",
  source_kind: "local",
  backend_entrypoint: "bin/planeai-plugin-fixture",
  capabilities: ["providers"],
  ui_contributions: [],
  providers: [provider],
  installed_hash: "hash",
  installed_path: "/planeai/plugins/packages/sha256/hash",
  original_display_path: "/source/local-fixture",
  enabled: true,
  state: "running",
  last_error: null,
  log_path: null,
};
const session: PluginSessionContext = {
  id: "session-1",
  projectId: "project-1",
  branch: "feat/chat",
  baseBranch: "main",
  status: "active",
  provider: "local-fixture:echo",
  taskKey: "PLA-1",
};

describe("PluginContributionHost provider sessions", () => {
  let target: HTMLElement;
  let component: ReturnType<typeof mount> | undefined;

  afterEach(() => {
    if (component) unmount(component);
    component = undefined;
    target?.remove();
    vi.clearAllMocks();
    eventListeners.clear();
  });

  async function mountHost(contribution: PluginUiContribution): Promise<HTMLIFrameElement> {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(PluginContributionHost, {
      target,
      props: { plugin, contribution, session, onNavigate: () => {}, onClose: () => {} },
    });
    return vi.waitFor(() => {
      const frame = target
        .querySelector<HTMLElement>("[data-plugin-ui-contribution]")
        ?.shadowRoot?.querySelector<HTMLIFrameElement>("iframe");
      expect(frame).toBeTruthy();
      return frame!;
    });
  }

  function stubFrameWindow(frame: HTMLIFrameElement): ReturnType<typeof vi.fn> {
    const postMessage = vi.fn();
    Object.defineProperty(frame, "contentWindow", { configurable: true, value: { postMessage } });
    return postMessage;
  }

  it("resumes the session before loading the provider UI bundle", async () => {
    const frame = await mountHost(providerContribution(provider));
    stubFrameWindow(frame);
    frame.dispatchEvent(new Event("load"));

    await vi.waitFor(() =>
      expect(localProviderUiSource).toHaveBeenCalledWith("local-fixture", "echo"),
    );
    expect(ensure).toHaveBeenCalledWith("session-1");
    expect(ensure.mock.invocationCallOrder[0]).toBeLessThan(
      localProviderUiSource.mock.invocationCallOrder[0],
    );
    expect(localUiSource).not.toHaveBeenCalled();
    expect(frame.srcdoc).toContain("session-send");
  });

  it("routes session input and interrupts through the host for the mounted session", async () => {
    const frame = await mountHost(providerContribution(provider));
    const postMessage = stubFrameWindow(frame);

    window.dispatchEvent(
      new MessageEvent("message", {
        source: frame.contentWindow,
        data: { type: "session-send", requestId: 1, text: "hello" },
      }),
    );
    window.dispatchEvent(
      new MessageEvent("message", {
        source: frame.contentWindow,
        data: { type: "session-interrupt", requestId: 2 },
      }),
    );

    await vi.waitFor(() =>
      expect(postMessage).toHaveBeenCalledWith(
        { type: "response", requestId: 2, ok: true, value: null },
        "*",
      ),
    );
    expect(send).toHaveBeenCalledWith("session-1", "hello");
    expect(interrupt).toHaveBeenCalledWith("session-1");
  });

  it("forwards only this session's provider events to the frame", async () => {
    const frame = await mountHost(providerContribution(provider));
    const postMessage = stubFrameWindow(frame);
    const forward = await vi.waitFor(() => {
      const listener = eventListeners.get("plugin-provider-session-event");
      expect(listener).toBeTruthy();
      return listener!;
    });

    forward({
      payload: { plugin_id: "other-plugin", session_id: "session-1", seq: 1, payload: "spoofed" },
    });
    forward({
      payload: {
        plugin_id: "local-fixture",
        session_id: "session-2",
        seq: 1,
        payload: "elsewhere",
      },
    });
    forward({
      payload: {
        plugin_id: "local-fixture",
        session_id: "session-1",
        seq: 3,
        payload: { type: "assistant", text: "hi" },
      },
    });

    expect(postMessage).toHaveBeenCalledOnce();
    expect(postMessage).toHaveBeenCalledWith(
      { type: "session-event", event: { seq: 3, payload: { type: "assistant", text: "hi" } } },
      "*",
    );
  });

  it("rejects session controls from contributions that are not provider UIs", async () => {
    const frame = await mountHost({
      id: "panel",
      label: "Panel",
      placement: "session.panel",
      entrypoint: "ui/panel.js",
      order: null,
      shortcut: null,
    });
    const postMessage = stubFrameWindow(frame);

    window.dispatchEvent(
      new MessageEvent("message", {
        source: frame.contentWindow,
        data: { type: "session-send", requestId: 7, text: "hello" },
      }),
    );

    await vi.waitFor(() =>
      expect(postMessage).toHaveBeenCalledWith(
        {
          type: "response",
          requestId: 7,
          ok: false,
          error: "session controls are available only to provider session UIs",
        },
        "*",
      ),
    );
    expect(send).not.toHaveBeenCalled();
    expect(eventListeners.has("plugin-provider-session-event")).toBe(false);
  });

  it("stops forwarding events once unmounted", async () => {
    await mountHost(providerContribution(provider));
    await vi.waitFor(() => expect(eventListeners.has("plugin-provider-session-event")).toBe(true));
    unmount(component!);
    component = undefined;
    expect(eventListeners.has("plugin-provider-session-event")).toBe(false);
  });
});
