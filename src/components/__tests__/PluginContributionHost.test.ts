import { afterEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
const {
  pluginCall,
  localUiSource,
  settingsGet,
  updateSettings,
  dataChanged,
  hostCall,
  eventListeners,
  showToast,
  dismissToast,
  openProjectForm,
  listAll,
  appearance,
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
  hostCall: vi.fn(() => Promise.resolve({ projects: [] })),
  eventListeners: new Map<string, (event: { payload: string }) => void>(),
  showToast: vi.fn(),
  dismissToast: vi.fn(),
  openProjectForm: vi.fn(),
  listAll: vi.fn(),
  appearance: { dark: false },
}));

vi.mock("../../lib/api", () => ({
  plugins: {
    call: pluginCall,
    hostCall,
    localUiSource,
    settings: settingsGet,
    updateSettings,
    dataChanged,
  },
  tasks: { listAll },
}));

vi.mock("../../lib/toasts.svelte", () => ({
  showToast,
  dismissToast,
}));

vi.mock("../../lib/plugin-modal-manager", () => ({
  openPluginModal: vi.fn(),
  openProjectForm,
}));

vi.mock("../../lib/settings.svelte", () => ({
  isDark: () => appearance.dark,
}));

import PluginContributionHost from "../PluginContributionHost.svelte";
import PluginContributionHostHarness from "./PluginContributionHostHarness.svelte";
import PluginContributionHostLocalHarness from "./PluginContributionHostLocalHarness.svelte";
import { getPluginSidebarRows } from "../../lib/plugin-sidebar-navigation.svelte";
import type { PluginInventory, PluginUiContribution } from "../../lib/types";
import { focusTerminal, getActiveZone } from "../../lib/focus.svelte";
import { shouldBypassSidebarKeyboard } from "../../lib/sidebar-nav.svelte";
import * as taskStore from "../../lib/task-store.svelte";
import type { PluginFrameInteraction, PluginUiHost } from "../../lib/plugin-sdk";
import type { ToastOptions } from "../../lib/toasts.svelte";

// Each bundled font answers with its own file name, so a test can tell the faces apart.
vi.stubGlobal(
  "fetch",
  vi.fn(
    async (url: string) =>
      new Response(new TextEncoder().encode(String(url).split("/").pop()!.split("?")[0])),
  ),
);

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
    appearance.dark = false;
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
      providers: [],
      installed_hash: null,
      installed_path: null,
      original_display_path: null,
      remote_origin: null,
      tools: [],
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
      expect(next?.shadowRoot).toBeTruthy();
      return next!;
    });

    expect(host.className).toContain("pointer-events-none");
    expect(host.shadowRoot?.childNodes).toHaveLength(0);
  });

  it("isolates local UI bundles in an opaque, script-only iframe with a message bridge", async () => {
    localUiSource.mockReturnValue(new Promise(() => {}));
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
      expect(frame?.srcdoc).toContain("host-rpc");
      expect(frame?.srcdoc).toContain("focused-agent-session");
      expect(frame?.srcdoc).toContain("getFocusedAgentSession");
      expect(frame?.srcdoc).toContain("sidebar-keydown");
      expect(frame?.srcdoc).toContain("sidebarNavigationKeys");
      expect(frame?.srcdoc).toContain('addEventListener("keydown", forwardSidebarKeydown)');
      expect(frame?.srcdoc).toContain("session: message.session");
    });
    await vi.waitFor(() => expect(localUiSource).toHaveBeenCalledWith("local-fixture", "fixture"));
    expect(localUiSource).toHaveBeenCalledOnce();
  });

  it("focuses a local session-panel iframe when the modal requests autofocus", async () => {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(PluginContributionHostLocalHarness, {
      target,
      props: { placement: "session.panel", autofocus: true },
    }) as typeof component;

    const frame = await vi.waitFor(() => {
      const next = target
        .querySelector<HTMLElement>("[data-plugin-ui-contribution]")
        ?.shadowRoot?.querySelector<HTMLIFrameElement>("iframe");
      expect(next).toBeTruthy();
      const root = next!.getRootNode();
      expect(root).toBeInstanceOf(ShadowRoot);
      expect((root as ShadowRoot).activeElement).toBe(next);
      return next!;
    });
    expect(frame.title).toBe("Fixture");
    expect(frame.style.outline).toBe("none");
  });

  it.each(["session.panel", "dialog"] as const)(
    "sizes a local %s iframe from its reported content height",
    async (placement) => {
      target = document.createElement("div");
      document.body.append(target);
      component = mount(PluginContributionHostLocalHarness, {
        target,
        props: { placement },
      }) as typeof component;

      const frame = await vi.waitFor(() => {
        const next = target
          .querySelector<HTMLElement>("[data-plugin-ui-contribution]")
          ?.shadowRoot?.querySelector<HTMLIFrameElement>("iframe");
        expect(next).toBeTruthy();
        return next!;
      });
      expect(frame.style.height).toBe("360px");
      window.dispatchEvent(
        new MessageEvent("message", {
          source: frame.contentWindow,
          data: { type: "content-height", height: 384.2 },
        }),
      );
      expect(frame.style.height).toBe("385px");
    },
  );

  it.each([
    ["sidebar.header", "planeai.plugin-host.v1"],
    ["main-pane", "planeai.plugin-host.v1"],
    ["dialog", "planeai.plugin-host.v1"],
    ["titlebar", "planeai.plugin-host.v1"],
    ["interaction", "planeai.plugin-host.v1"],
    ["session.panel", "planeai.plugin-host.v5"],
    ["sidebar.section", "planeai.plugin-host.v7"],
  ] as const)(
    "tells a local %s frame on %s its plugin's data changed without remounting it",
    async (placement, hostApiVersion) => {
      target = document.createElement("div");
      document.body.append(target);
      component = mount(PluginContributionHostLocalHarness, {
        target,
        props: { placement, hostApiVersion },
      }) as typeof component;

      const frame = await vi.waitFor(() => {
        const next = target
          .querySelector<HTMLElement>("[data-plugin-ui-contribution]")
          ?.shadowRoot?.querySelector<HTMLIFrameElement>("iframe");
        expect(next).toBeTruthy();
        return next!;
      });
      const posted = vi.spyOn(frame.contentWindow!, "postMessage");
      const changed = await vi.waitFor(() => {
        const listener = eventListeners.get("plugin-data-changed");
        expect(listener).toBeTruthy();
        return listener!;
      });

      changed({ payload: "local-fixture" });

      expect(posted).toHaveBeenCalledWith({ type: "data-changed" }, "*");
      expect(
        target.querySelector("[data-plugin-ui-contribution]")?.shadowRoot?.querySelector("iframe"),
      ).toBe(frame);
    },
  );

  it("remounts a local session panel from before host API v5 when its plugin's data changed", async () => {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(PluginContributionHostLocalHarness, {
      target,
      props: { placement: "session.panel", hostApiVersion: "planeai.plugin-host.v4" },
    }) as typeof component;
    const frameOf = () =>
      target
        .querySelector("[data-plugin-ui-contribution]")
        ?.shadowRoot?.querySelector<HTMLIFrameElement>("iframe");
    const frame = await vi.waitFor(() => {
      expect(frameOf()).toBeTruthy();
      return frameOf()!;
    });
    const changed = await vi.waitFor(() => {
      const listener = eventListeners.get("plugin-data-changed");
      expect(listener).toBeTruthy();
      return listener!;
    });

    changed({ payload: "local-fixture" });

    await vi.waitFor(() => expect(frameOf()).not.toBe(frame));
  });

  it("remounts an older local sidebar section at its last content height", async () => {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(PluginContributionHostLocalHarness, { target }) as typeof component;
    const frameOf = () =>
      target
        .querySelector("[data-plugin-ui-contribution]")
        ?.shadowRoot?.querySelector<HTMLIFrameElement>("iframe");
    const frame = await vi.waitFor(() => {
      expect(frameOf()).toBeTruthy();
      return frameOf()!;
    });
    window.dispatchEvent(
      new MessageEvent("message", {
        source: frame.contentWindow,
        data: { type: "content-height", height: 48 },
      }),
    );
    const changed = await vi.waitFor(() => {
      const listener = eventListeners.get("plugin-data-changed");
      expect(listener).toBeTruthy();
      return listener!;
    });

    changed({ payload: "local-fixture" });

    const remounted = await vi.waitFor(() => {
      expect(frameOf()).not.toBe(frame);
      return frameOf()!;
    });
    expect(remounted.style.height).toBe("48px");
  });

  it.each([
    ["sidebar.section", "0px"],
    ["dialog", "1px"],
  ] as const)("shrinks a local %s with empty content to %s", async (placement, height) => {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(PluginContributionHostLocalHarness, {
      target,
      props: { placement },
    }) as typeof component;
    const frame = await vi.waitFor(() => {
      const next = target
        .querySelector<HTMLElement>("[data-plugin-ui-contribution]")
        ?.shadowRoot?.querySelector<HTMLIFrameElement>("iframe");
      expect(next).toBeTruthy();
      return next!;
    });
    const report = (height: number) =>
      window.dispatchEvent(
        new MessageEvent("message", {
          source: frame.contentWindow,
          data: { type: "content-height", height },
        }),
      );

    report(48);
    expect(frame.style.height).toBe("48px");
    report(0);

    expect(frame.style.height).toBe(height);
  });

  it("scrolls the sidebar to the row a local section focuses", async () => {
    target = document.createElement("div");
    target.style.overflowY = "auto";
    document.body.append(target);
    let scrollTop = 0;
    Object.defineProperty(target, "scrollTop", {
      get: () => scrollTop,
      set: (value: number) => {
        scrollTop = value;
      },
    });
    target.getBoundingClientRect = () => ({ top: 0, bottom: 100 }) as DOMRect;
    component = mount(PluginContributionHostLocalHarness, { target }) as typeof component;
    const frame = await vi.waitFor(() => {
      const next = target
        .querySelector<HTMLElement>("[data-plugin-ui-contribution]")
        ?.shadowRoot?.querySelector<HTMLIFrameElement>("iframe");
      expect(next).toBeTruthy();
      return next!;
    });
    frame.getBoundingClientRect = () => ({ top: 50 }) as DOMRect;
    const reveal = (top: number, bottom: number) =>
      window.dispatchEvent(
        new MessageEvent("message", {
          source: frame.contentWindow,
          data: { type: "reveal", top, bottom },
        }),
      );

    reveal(80, 110);
    expect(scrollTop).toBe(0);
    frame.focus();
    reveal(80, 110);
    expect(scrollTop).toBe(60);
    reveal(-70, -40);
    expect(scrollTop).toBe(40);
    reveal(0, 30);
    expect(scrollTop).toBe(40);
  });

  it.each(["sidebar.header", "sidebar.section"] as const)(
    "sizes a local %s iframe to its content instead of a fixed block",
    async (placement) => {
      target = document.createElement("div");
      document.body.append(target);
      component = mount(PluginContributionHostLocalHarness, {
        target,
        props: { placement },
      }) as typeof component;

      const frame = await vi.waitFor(() => {
        const next = target
          .querySelector<HTMLElement>("[data-plugin-ui-contribution]")
          ?.shadowRoot?.querySelector<HTMLIFrameElement>("iframe");
        expect(next).toBeTruthy();
        return next!;
      });
      expect(frame.style.height).toBe("0px");
      window.dispatchEvent(
        new MessageEvent("message", {
          source: frame.contentWindow,
          data: { type: "content-height", height: 32 },
        }),
      );
      expect(frame.style.height).toBe("32px");
      expect(frame.srcdoc).toContain(`"${placement}"`);
    },
  );

  it("reports local content heights from the host iframe bridge", async () => {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(PluginContributionHostLocalHarness, {
      target,
      props: { placement: "session.panel" },
    }) as typeof component;

    const frame = await vi.waitFor(() => {
      const next = target
        .querySelector<HTMLElement>("[data-plugin-ui-contribution]")
        ?.shadowRoot?.querySelector<HTMLIFrameElement>("iframe");
      expect(next).toBeTruthy();
      return next!;
    });
    expect(frame.srcdoc).toContain("reportContentHeight");
    expect(frame.srcdoc).toContain("child.scrollHeight");
    expect(frame.srcdoc).toContain('send({ type: "content-height", height })');
  });

  it("forwards Escape from a local plugin modal iframe through navigation.close when requested", async () => {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(PluginContributionHostLocalHarness, {
      target,
      props: { placement: "session.panel", closeOnEscape: true },
    }) as typeof component;

    const frame = await vi.waitFor(() => {
      const next = target
        .querySelector<HTMLElement>("[data-plugin-ui-contribution]")
        ?.shadowRoot?.querySelector<HTMLIFrameElement>("iframe");
      expect(next).toBeTruthy();
      return next!;
    });
    expect(frame.srcdoc).toContain("const closeOnEscape = true");
    expect(frame.srcdoc).toContain("forwardEscapeToHost");
    expect(frame.srcdoc).toContain('send({ type: "navigation", action: "close" })');
  });

  it("uses a compact iframe for local sidebar footers", async () => {
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
  });

  it.each(["main-pane", "preferences"] as const)(
    "fills the %s host with a local plugin iframe",
    async (placement) => {
      target = document.createElement("div");
      document.body.append(target);
      component = mount(PluginContributionHostLocalHarness, {
        target,
        props: { placement },
      }) as typeof component;

      const frame = await vi.waitFor(() => {
        const next = target
          .querySelector<HTMLElement>("[data-plugin-ui-contribution]")
          ?.shadowRoot?.querySelector<HTMLIFrameElement>("iframe");
        expect(next).toBeTruthy();
        return next!;
      });
      expect(frame.className).toContain("h-full");
      expect(frame.style.display).toBe("block");
      expect(frame.style.width).toBe("100%");
      expect(frame.style.height).toBe("100%");
      expect(frame.style.border).toBe("0px");
      expect(frame.srcdoc).toContain("html,body{margin:0;height:100%;min-height:100%");
    },
  );

  it("uses transparent iframe chrome for local titlebar controls", async () => {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(PluginContributionHostLocalHarness, {
      target,
      props: { placement: "titlebar" },
    }) as typeof component;

    const frame = await vi.waitFor(() => {
      const next = target
        .querySelector<HTMLElement>("[data-plugin-ui-contribution]")
        ?.shadowRoot?.querySelector<HTMLIFrameElement>("iframe");
      expect(next).toBeTruthy();
      return next!;
    });
    expect(frame.style.width).toBe("88px");
    expect(frame.style.backgroundColor).toBe("transparent");
    expect(frame.srcdoc).toContain('id="planeai-plugin-titlebar"');
    expect(frame.srcdoc).toContain("html,body{background:transparent}");
  });

  it.each([
    ["dialog", true],
    ["session.panel", true],
    ["sidebar.header", true],
    ["sidebar.section", true],
    ["main-pane", false],
  ] as const)(
    "lets a local %s frame show the host surface and shrink to its content: %s",
    async (placement, contentSized) => {
      target = document.createElement("div");
      document.body.append(target);
      component = mount(PluginContributionHostLocalHarness, {
        target,
        props: { placement },
      }) as typeof component;

      const frame = await vi.waitFor(() => {
        const next = target
          .querySelector<HTMLElement>("[data-plugin-ui-contribution]")
          ?.shadowRoot?.querySelector<HTMLIFrameElement>("iframe");
        expect(next).toBeTruthy();
        return next!;
      });
      expect(frame.style.backgroundColor).toBe(contentSized ? "transparent" : "");
      expect(
        frame.srcdoc.includes("html,body{height:auto;min-height:0;background:transparent}"),
      ).toBe(contentSized);
    },
  );

  it("uses an unfocusable, pointer-inert transparent iframe for local session indicators", async () => {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(PluginContributionHostLocalHarness, {
      target,
      props: { placement: "session.indicator" },
    }) as typeof component;

    const host = await vi.waitFor(() => {
      const next = target.querySelector<HTMLElement>("[data-plugin-ui-contribution]");
      expect(next).toBeTruthy();
      return next!;
    });
    const frame = host.shadowRoot?.querySelector<HTMLIFrameElement>("iframe");
    expect(host.className).toContain("pointer-events-none");
    expect(host.getAttribute("role")).toBeNull();
    expect(frame?.style.width).toBe("16px");
    expect(frame?.style.height).toBe("16px");
    expect(frame?.style.pointerEvents).toBe("none");
    expect(frame?.tabIndex).toBe(-1);
    expect(frame?.srcdoc).toContain('id="planeai-plugin-indicator"');
  });

  it("uses indicator content width to toggle the visible-row reservation", async () => {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(PluginContributionHostLocalHarness, {
      target,
      props: { placement: "session.indicator" },
    }) as typeof component;

    const host = await vi.waitFor(() => {
      const next = target.querySelector<HTMLElement>("[data-plugin-ui-contribution]");
      expect(next).toBeTruthy();
      return next!;
    });
    const frame = host.shadowRoot?.querySelector<HTMLIFrameElement>("iframe");
    const contentWindow = { postMessage: vi.fn() } as unknown as Window;
    Object.defineProperty(frame, "contentWindow", { configurable: true, value: contentWindow });

    window.dispatchEvent(
      new MessageEvent("message", {
        source: contentWindow,
        data: { type: "content-width", width: 16 },
      }),
    );
    expect(host.dataset.pluginIndicatorVisible).toBe("true");

    window.dispatchEvent(
      new MessageEvent("message", {
        source: contentWindow,
        data: { type: "content-width", width: 0 },
      }),
    );
    expect(host.dataset.pluginIndicatorVisible).toBe("false");
  });

  it("sizes a local session meta slot to its reported content width, up to 160px", async () => {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(PluginContributionHostLocalHarness, {
      target,
      props: { placement: "session.meta" },
    }) as typeof component;

    const host = await vi.waitFor(() => {
      const next = target.querySelector<HTMLElement>("[data-plugin-ui-contribution]");
      expect(next).toBeTruthy();
      return next!;
    });
    const frame = host.shadowRoot!.querySelector<HTMLIFrameElement>("iframe")!;
    const contentWindow = { postMessage: vi.fn() } as unknown as Window;
    Object.defineProperty(frame, "contentWindow", { configurable: true, value: contentWindow });
    const reportWidth = (width: number) =>
      window.dispatchEvent(
        new MessageEvent("message", {
          source: contentWindow,
          data: { type: "content-width", width },
        }),
      );
    const widths = () => [host.style.width, frame.style.width, frame.style.height];

    expect(host.getAttribute("role")).toBeNull();
    expect(host.className).toContain("pointer-events-none");
    expect(frame.tabIndex).toBe(-1);
    expect(widths()).toEqual(["0px", "0px", "16px"]);

    reportWidth(42.4);
    expect(widths()).toEqual(["43px", "43px", "16px"]);

    reportWidth(400);
    expect(widths()).toEqual(["160px", "160px", "16px"]);

    reportWidth(0);
    expect(widths()).toEqual(["0px", "0px", "16px"]);
  });

  it("provides local iframes with semantic PlaneAI tokens and live theme updates", async () => {
    document.documentElement.style.setProperty("--color-main", "#123456");
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
    expect(frame.srcdoc).toContain('id="planeai-plugin-theme"');
    expect(frame.srcdoc).toContain("--planeai-main:#123456");
    expect(frame.srcdoc).toContain("button,input,select,textarea{font:inherit");
    expect(frame.srcdoc).toContain("font-size:13px");
    expect(frame.srcdoc).toContain("h1,h2,h3,p{margin:0}");
    expect(frame.srcdoc).toContain("select{appearance:none");

    const postMessage = vi.fn();
    Object.defineProperty(frame, "contentWindow", { configurable: true, value: { postMessage } });
    window.dispatchEvent(new Event("planeai-theme-changed"));
    await vi.waitFor(() =>
      expect(postMessage).toHaveBeenCalledWith(
        expect.objectContaining({
          type: "theme",
          css: expect.stringContaining("--planeai-main:#123456"),
        }),
        "*",
      ),
    );
    document.documentElement.style.removeProperty("--color-main");
  });

  it("renders local iframes in the host color scheme so transparent placements blend in", async () => {
    appearance.dark = true;
    target = document.createElement("div");
    document.body.append(target);
    component = mount(PluginContributionHostLocalHarness, {
      target,
      props: { placement: "titlebar" },
    }) as typeof component;

    const frame = await vi.waitFor(() => {
      const next = target
        .querySelector<HTMLElement>("[data-plugin-ui-contribution]")
        ?.shadowRoot?.querySelector<HTMLIFrameElement>("iframe");
      expect(next).toBeTruthy();
      return next!;
    });
    expect(frame.srcdoc).toContain(":root{color-scheme:dark;");

    const postMessage = vi.fn();
    Object.defineProperty(frame, "contentWindow", { configurable: true, value: { postMessage } });
    appearance.dark = false;
    window.dispatchEvent(new Event("planeai-theme-changed"));
    await vi.waitFor(() =>
      expect(postMessage).toHaveBeenCalledWith(
        { type: "theme", css: expect.stringContaining(":root{color-scheme:light;") },
        "*",
      ),
    );
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
    const source = Promise.withResolvers<string>();
    localUiSource.mockReturnValue(source.promise);
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
    await vi.waitFor(() => expect(localUiSource).toHaveBeenCalledWith("local-fixture", "fixture"));
    const postMessage = vi.fn((message: unknown) => {
      structuredClone(message);
    });
    Object.defineProperty(frame, "contentWindow", {
      configurable: true,
      value: { postMessage },
    });

    source.resolve("export default { mount() { return () => {}; } };");
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

  it.each([
    [
      "an Escape the sidebar leaves reaches the app's router",
      { key: "Escape" },
      false,
      true,
      ["Escape"],
    ],
    [
      "a chord pressed on a row reaches the app once",
      { key: "Tab", ctrlKey: true },
      false,
      true,
      ["ctrl+Tab"],
    ],
    ["a key the sidebar handles stays with the sidebar", { key: "j" }, true, true, []],
    ["a frame without the keyboard replays nothing", { key: "Escape" }, false, false, []],
  ] as const)("%s", async (_name, pressed, handled, focused, replayed) => {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(PluginContributionHostLocalHarness, { target }) as typeof component;
    const host = await vi.waitFor(() => {
      const next = target.querySelector<HTMLElement>("[data-plugin-ui-contribution]");
      expect(next?.shadowRoot?.querySelector("iframe")).toBeTruthy();
      return next!;
    });
    const frame = host.shadowRoot!.querySelector<HTMLIFrameElement>("iframe")!;
    if (focused) frame.focus();
    const handleSidebarKeydown = (event: Event) => {
      if (handled) (event as CustomEvent<{ event: KeyboardEvent }>).detail.event.preventDefault();
    };
    host.addEventListener("plugin-sidebar-keydown", handleSidebarKeydown);
    const keys: string[] = [];
    const recordKey = (event: KeyboardEvent) =>
      keys.push(`${event.ctrlKey ? "ctrl+" : ""}${event.key}`);
    window.addEventListener("keydown", recordKey);

    window.dispatchEvent(
      new MessageEvent("message", {
        source: frame.contentWindow,
        data: { type: "sidebar-keydown", ...pressed },
      }),
    );

    expect(keys).toEqual(replayed);
    window.removeEventListener("keydown", recordKey);
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

    expect(showToast).not.toHaveBeenCalled();
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
  it("registers the host's theme fonts in a local frame before its UI mounts", async () => {
    const source = Promise.withResolvers<string>();
    localUiSource.mockReturnValue(source.promise);
    target = document.createElement("div");
    document.body.append(target);
    // The stub document has no real body to observe, so the frame must not be content-sized.
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
    await vi.waitFor(() => expect(localUiSource).toHaveBeenCalled());
    const hostPostMessage = vi.fn();
    Object.defineProperty(frame, "contentWindow", {
      configurable: true,
      value: { postMessage: hostPostMessage },
    });
    source.resolve("export default {};");
    await vi.waitFor(() =>
      expect(hostPostMessage).toHaveBeenCalledWith(expect.objectContaining({ type: "init" }), "*"),
    );
    const init = structuredClone(
      hostPostMessage.mock.calls.find(([message]) => message.type === "init")![0],
    );

    const frameListeners: Array<(event: { source: unknown; data: unknown }) => void> = [];
    const parent = { postMessage: vi.fn() };
    const registered: Array<{ family: string; source: string; descriptors: FontFaceDescriptors }> =
      [];
    const fontsLoaded = Promise.withResolvers<void>();
    class FakeFontFace {
      loaded: Promise<unknown>;
      constructor(
        readonly family: string,
        readonly source: ArrayBuffer,
        readonly descriptors: FontFaceDescriptors,
      ) {
        this.loaded = fontsLoaded.promise;
      }
    }
    const frameDocument = {
      body: {},
      fonts: {
        add: (face: FakeFontFace) =>
          registered.push({
            family: face.family,
            source: new TextDecoder().decode(face.source),
            descriptors: face.descriptors,
          }),
      },
    };
    const frameUrl = { createObjectURL: () => "blob:bundle", revokeObjectURL: () => {} };
    let mounted = false;
    // A Function body cannot reach Node's module loader, so the bundle import is stubbed.
    const importBundle = async () => ({ default: { mount: () => ((mounted = true), () => {}) } });
    const bridge = frame.srcdoc
      .match(/<script>([\s\S]*)<\/script>/)![1]
      .replace("await import(url)", "await importBundle(url)");
    new Function(
      "parent",
      "addEventListener",
      "removeEventListener",
      "document",
      "FontFace",
      "URL",
      "importBundle",
      bridge,
    )(
      parent,
      (type: string, listener: (event: { source: unknown; data: unknown }) => void) => {
        if (type === "message") frameListeners.push(listener);
      },
      () => {},
      frameDocument,
      FakeFontFace,
      frameUrl,
      importBundle,
    );
    for (const listener of frameListeners) listener({ source: parent, data: init });

    const latin =
      "U+0000-00FF,U+0131,U+0152-0153,U+02BB-02BC,U+02C6,U+02DA,U+02DC,U+0304,U+0308,U+0329,U+2000-206F,U+20AC,U+2122,U+2191,U+2193,U+2212,U+2215,U+FEFF,U+FFFD";
    await vi.waitFor(() =>
      expect(registered).toEqual([
        {
          family: "IBM Plex Sans Variable",
          source: "ibm-plex-sans-latin-wght-normal.woff2",
          descriptors: { style: "normal", weight: "100 700", unicodeRange: latin, display: "swap" },
        },
        {
          family: "IBM Plex Mono",
          source: "ibm-plex-mono-latin-400-normal.woff2",
          descriptors: { style: "normal", weight: "400", unicodeRange: latin, display: "swap" },
        },
        {
          family: "IBM Plex Mono",
          source: "ibm-plex-mono-latin-500-normal.woff2",
          descriptors: { style: "normal", weight: "500", unicodeRange: latin, display: "swap" },
        },
      ]),
    );
    expect(mounted).toBe(false);

    fontsLoaded.resolve();
    await vi.waitFor(() =>
      expect(parent.postMessage).toHaveBeenCalledWith({ type: "mounted" }, "*"),
    );
    expect(mounted).toBe(true);
  });

  /** The host surface a local frame's bundle receives, with the frame bridge running against the real host component. */
  type FrameHost = Omit<PluginUiHost, "interaction" | "data"> & {
    interaction?: PluginFrameInteraction;
    data: PluginUiHost["data"];
  };

  async function bootLocalFrame(props: {
    placement: PluginUiContribution["placement"];
    hostApiVersion: string;
    capabilities?: string[];
  }): Promise<{ host: FrameHost; frameWindow: Window }> {
    const source = Promise.withResolvers<string>();
    localUiSource.mockReturnValue(source.promise);
    target = document.createElement("div");
    document.body.append(target);
    component = mount(PluginContributionHostLocalHarness, { target, props }) as typeof component;
    const frame = await vi.waitFor(() => {
      const next = target
        .querySelector<HTMLElement>("[data-plugin-ui-contribution]")
        ?.shadowRoot?.querySelector<HTMLIFrameElement>("iframe");
      expect(next).toBeTruthy();
      return next!;
    });
    await vi.waitFor(() => expect(localUiSource).toHaveBeenCalled());
    const frameWindow = frame.contentWindow!;
    const frameListeners: Array<(event: { source: unknown; data: unknown }) => void> = [];
    const parent = {
      postMessage: (data: unknown) =>
        window.dispatchEvent(
          new MessageEvent("message", { source: frameWindow, data: structuredClone(data) }),
        ),
    };
    vi.spyOn(frameWindow, "postMessage").mockImplementation(((data: unknown) => {
      for (const listener of frameListeners)
        listener({ source: parent, data: structuredClone(data) });
    }) as Window["postMessage"]);
    let host: FrameHost | undefined;
    // A Function body cannot reach Node's module loader, so the bundle import is stubbed.
    const importBundle = async () => ({
      default: {
        mount: (_root: unknown, context: { host: FrameHost }) => {
          host = context.host;
          return () => {};
        },
      },
    });
    const bridge = frame.srcdoc
      .match(/<script>([\s\S]*)<\/script>/)![1]
      .replace("await import(url)", "await importBundle(url)");
    new Function(
      "parent",
      "addEventListener",
      "removeEventListener",
      "document",
      "FontFace",
      "URL",
      "importBundle",
      bridge,
    )(
      parent,
      (type: string, listener: (event: { source: unknown; data: unknown }) => void) => {
        if (type === "message") frameListeners.push(listener);
      },
      () => {},
      { body: {}, fonts: { add: () => {} } },
      class {
        loaded = Promise.resolve();
      },
      { createObjectURL: () => "blob:bundle", revokeObjectURL: () => {} },
      importBundle,
    );
    source.resolve("export default {};");
    await vi.waitFor(() => expect(host).toBeTruthy());
    return { host: host!, frameWindow };
  }

  const SYNC_HOST_API = "planeai.plugin-host.v7";

  function lastToast(): ToastOptions {
    return (showToast.mock.calls.at(-1) as unknown as [ToastOptions])[0];
  }

  it("round-trips a local frame's toast actions and dismissal to the frame's callbacks", async () => {
    const { host } = await bootLocalFrame({
      placement: "interaction",
      hostApiVersion: SYNC_HOST_API,
    });
    const ran: string[] = [];

    const id = await host.interaction!.showToast({
      id: "departed",
      kind: "warning",
      message: "ENG-1 left your Linear view",
      detail: "Fix login",
      actions: [{ key: "Enter", label: "Done", run: () => void ran.push("done ENG-1") }],
      dismiss: () => void ran.push("dismissed ENG-1"),
    });
    const first = lastToast();
    first.actions![0].run();
    await host.interaction!.showToast({
      id: "departed",
      kind: "warning",
      message: "ENG-2 left your Linear view",
      actions: [{ key: "Enter", label: "Done", run: () => void ran.push("done ENG-2") }],
      dismiss: () => void ran.push("dismissed ENG-2"),
    });
    (lastToast().dismiss as () => void)();

    expect(id).toBe("departed");
    expect({
      ...first,
      actions: first.actions!.map(({ key, label }) => ({ key, label })),
      dismiss: typeof first.dismiss,
    }).toEqual({
      id: "plugin:local-fixture:fixture:departed",
      kind: "warning",
      message: "ENG-1 left your Linear view",
      detail: "Fix login",
      actions: [{ key: "Enter", label: "Done" }],
      dismiss: "function",
    });
    await vi.waitFor(() => expect(ran).toEqual(["done ENG-1", "dismissed ENG-2"]));
    unmount(component!);
    component = undefined;
    expect(dismissToast).not.toHaveBeenCalled();
  });

  it("maps a local frame's dismiss setting and dismisses its toasts when asked and when it goes away", async () => {
    const { host } = await bootLocalFrame({
      placement: "interaction",
      hostApiVersion: SYNC_HOST_API,
    });

    await host.interaction!.showToast({
      id: "resolving",
      message: "Marking ENG-1 done",
      dismiss: "disabled",
    });
    const resolving = lastToast();
    await host.interaction!.showToast({ id: "plain", message: "Synced" });
    const plain = lastToast();
    await host.interaction!.dismissToast("resolving");
    const dismissedByFrame = dismissToast.mock.calls.map(([toastId]) => toastId);
    unmount(component!);
    component = undefined;

    expect([resolving.dismiss, resolving.kind, plain.dismiss, plain.kind]).toEqual([
      "disabled",
      "info",
      "default",
      "info",
    ]);
    expect(dismissedByFrame).toEqual(["plugin:local-fixture:fixture:resolving"]);
    expect(dismissToast.mock.calls.map(([toastId]) => toastId)).toEqual([
      "plugin:local-fixture:fixture:resolving",
      "plugin:local-fixture:fixture:plain",
    ]);
  });

  it("opens PlaneAI's project form for a local frame and answers with the new project", async () => {
    const { host } = await bootLocalFrame({ placement: "dialog", hostApiVersion: SYNC_HOST_API });
    openProjectForm
      .mockResolvedValueOnce({
        id: "p1",
        name: "Demo",
        path: "/demo",
        hidden: false,
        path_missing: false,
      })
      .mockResolvedValueOnce(null);

    const created = await host.interaction!.openProjectForm();
    const cancelled = await host.interaction!.openProjectForm();

    expect([created, cancelled]).toEqual([{ id: "p1", name: "Demo", path: "/demo" }, null]);
  });

  it("keeps the interaction bridge from local frames before host API v7", async () => {
    const { host, frameWindow } = await bootLocalFrame({
      placement: "interaction",
      hostApiVersion: "planeai.plugin-host.v6",
    });
    const answers: unknown[] = [];
    vi.mocked(frameWindow.postMessage).mockImplementation(((data: unknown) => {
      answers.push(data);
    }) as Window["postMessage"]);

    window.dispatchEvent(
      new MessageEvent("message", {
        source: frameWindow,
        data: { type: "toast-show", requestId: 41, toast: { id: "x", message: "Hello" } },
      }),
    );
    window.dispatchEvent(
      new MessageEvent("message", {
        source: frameWindow,
        data: { type: "project-form", requestId: 42 },
      }),
    );

    expect(host.interaction).toBeUndefined();
    expect(answers).toEqual([
      {
        type: "response",
        requestId: 41,
        ok: false,
        error: "host.interaction requires host API planeai.plugin-host.v7",
      },
      {
        type: "response",
        requestId: 42,
        ok: false,
        error: "host.interaction requires host API planeai.plugin-host.v7",
      },
    ]);
    expect(showToast.mock.calls).toEqual([]);
  });

  it("tells a local tasks.sync plugin's frame when PlaneAI task data changes", async () => {
    const synced = await bootLocalFrame({
      placement: "sidebar.section",
      hostApiVersion: SYNC_HOST_API,
      capabilities: ["tasks.read", "tasks.create", "tasks.sync"],
    });
    const changes: string[] = [];
    synced.host.data.onTaskDataChanged!(() => changes.push("tasks.sync"));
    listAll.mockResolvedValue([
      {
        key: "PLA-1",
        title: "Child",
        status: "todo",
        description: "",
        priority: 0,
        parent_key: "ENG-1",
      },
    ]);

    await taskStore.loadTasks(["/repo"]);

    await vi.waitFor(() => expect(changes).toEqual(["tasks.sync"]));
    await taskStore.loadTasks([]);
  });

  it("offers task data changes only to a frame whose plugin holds tasks.sync", async () => {
    const { host } = await bootLocalFrame({
      placement: "sidebar.section",
      hostApiVersion: SYNC_HOST_API,
      capabilities: ["tasks.read"],
    });

    expect(host.data.onTaskDataChanged).toBeUndefined();
  });

  it("keeps a local interaction frame invisible and out of the way of pointer input", async () => {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(PluginContributionHostLocalHarness, {
      target,
      props: { placement: "interaction", hostApiVersion: SYNC_HOST_API },
    }) as typeof component;

    const host = await vi.waitFor(() => {
      const next = target.querySelector<HTMLElement>("[data-plugin-ui-contribution]");
      expect(next?.shadowRoot?.querySelector("iframe")).toBeTruthy();
      return next!;
    });
    const frame = host.shadowRoot!.querySelector<HTMLIFrameElement>("iframe")!;

    expect(host.className).toBe("pointer-events-none");
    expect({
      width: frame.style.width,
      height: frame.style.height,
      pointerEvents: frame.style.pointerEvents,
      tabIndex: frame.tabIndex,
      hidden: frame.getAttribute("aria-hidden"),
    }).toEqual({
      width: "0px",
      height: "0px",
      pointerEvents: "none",
      tabIndex: -1,
      hidden: "true",
    });
  });
});
