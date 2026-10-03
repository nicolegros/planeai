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
import ProviderSessionFocusHarness from "./ProviderSessionFocusHarness.svelte";
import { PROVIDER_FRAME_ATTRIBUTE } from "../../lib/terminal-focus";
import { providerContribution } from "../../lib/plugin-providers";
import type { PluginSessionContext } from "../../lib/plugin-sdk";
import type { PluginInventory, PluginProvider, PluginUiContribution } from "../../lib/types";

const provider: PluginProvider = {
  id: "echo",
  label: "Echo",
  entrypoint: "ui/chat.js",
  supports: ["yolo"],
};
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

  it("routes terminal handoff to the app only when the provider offers it", async () => {
    const onSessionHandoff = vi.fn(async () => {});
    target = document.createElement("div");
    document.body.append(target);
    component = mount(PluginContributionHost, {
      target,
      props: {
        plugin,
        contribution: providerContribution(provider),
        session,
        onNavigate: () => {},
        onClose: () => {},
        onSessionHandoff,
      },
    });
    const frame = await vi.waitFor(() => {
      const next = target
        .querySelector<HTMLElement>("[data-plugin-ui-contribution]")
        ?.shadowRoot?.querySelector<HTMLIFrameElement>("iframe");
      expect(next).toBeTruthy();
      return next!;
    });
    const postMessage = stubFrameWindow(frame);
    window.dispatchEvent(
      new MessageEvent("message", {
        source: frame.contentWindow,
        data: { type: "session-handoff", requestId: 5 },
      }),
    );
    window.dispatchEvent(
      new MessageEvent("message", {
        source: frame.contentWindow,
        data: { type: "session-handback", requestId: 6 },
      }),
    );
    await vi.waitFor(() =>
      expect(postMessage).toHaveBeenCalledWith(
        { type: "response", requestId: 5, ok: true, value: null },
        "*",
      ),
    );
    expect(onSessionHandoff).toHaveBeenCalledOnce();
    expect(postMessage).toHaveBeenCalledWith(
      {
        type: "response",
        requestId: 6,
        ok: false,
        error: "terminal handoff is not available here",
      },
      "*",
    );
  });

  it("replays app chords from the frame on the host window, where the shortcut router listens", async () => {
    const frame = await mountHost(providerContribution(provider));
    expect(frame.srcdoc).toContain("forwardHostChord");
    const seen: KeyboardEvent[] = [];
    const record = (event: KeyboardEvent) => seen.push(event);
    window.addEventListener("keydown", record);
    window.addEventListener("keyup", record);
    try {
      const fromFrame = (data: Record<string, unknown>) =>
        window.dispatchEvent(
          new MessageEvent("message", {
            source: frame.contentWindow,
            data: { type: "host-key", ...data },
          }),
        );
      fromFrame({ phase: "keydown", key: "{", code: "BracketLeft", metaKey: true, shiftKey: true });
      fromFrame({ phase: "keyup", key: "Control" });
      fromFrame({ phase: "keypress", key: "x", metaKey: true });
      window.dispatchEvent(
        new MessageEvent("message", {
          source: window,
          data: { type: "host-key", phase: "keydown", key: "w", metaKey: true },
        }),
      );
    } finally {
      window.removeEventListener("keydown", record);
      window.removeEventListener("keyup", record);
    }
    expect(seen.map((event) => [event.type, event.key, event.metaKey, event.shiftKey])).toEqual([
      ["keydown", "{", true, true],
      ["keyup", "Control", false, false],
    ]);
    expect(seen[0].cancelable).toBe(true);
  });

  it("marks the chat frame, reports its focus, and takes focus back when its pane owns the keyboard again", async () => {
    target = document.createElement("div");
    document.body.append(target);
    const onFocused = vi.fn();
    const harness = mount(ProviderSessionFocusHarness, {
      target,
      props: { plugin, contribution: providerContribution(provider), session, onFocused },
    });
    component = harness;
    const frame = await vi.waitFor(() => {
      const found = target
        .querySelector<HTMLElement>("[data-plugin-ui-contribution]")
        ?.shadowRoot?.querySelector<HTMLIFrameElement>("iframe");
      expect(found).toBeTruthy();
      return found!;
    });
    expect(frame.hasAttribute(PROVIDER_FRAME_ATTRIBUTE)).toBe(true);

    frame.dispatchEvent(new FocusEvent("focus"));
    expect(onFocused).toHaveBeenCalledOnce();

    const focus = vi.spyOn(frame, "focus");
    harness.setFocused(true);
    await vi.waitFor(() => expect(focus).toHaveBeenCalled());
    // The same frame, not a rebuilt one: rebuilding reloads the chat and loses what it holds.
    expect(
      target
        .querySelector<HTMLElement>("[data-plugin-ui-contribution]")
        ?.shadowRoot?.querySelector("iframe"),
    ).toBe(frame);
  });

  it("leaves frames of other placements unmarked", async () => {
    const frame = await mountHost({
      id: "panel",
      label: "Panel",
      placement: "session.panel",
      entrypoint: "ui/panel.js",
      order: null,
      shortcut: null,
    });
    expect(frame.hasAttribute(PROVIDER_FRAME_ATTRIBUTE)).toBe(false);
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
