import { afterEach, describe, expect, it, vi } from "vitest";
import { flushSync, mount, unmount } from "svelte";

vi.mock("../../lib/api", () => ({
  plugins: {
    call: vi.fn(() => Promise.resolve(null)),
    hostCall: vi.fn(() => Promise.resolve(null)),
    localUiSource: vi.fn(() => Promise.resolve("")),
    settings: vi.fn(() => Promise.resolve({})),
    updateSettings: vi.fn(() => Promise.resolve({})),
    dataChanged: vi.fn(() => Promise.resolve()),
  },
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));
vi.mock("../../lib/plugin-frame-fonts", () => ({ pluginFrameFonts: () => Promise.resolve([]) }));
vi.mock("../../lib/toasts.svelte", () => ({ showToast: vi.fn() }));
vi.mock("../../lib/settings.svelte", () => ({ isDark: () => false }));

import SessionMetaSlot from "./SessionMetaSlotHarness.svelte";
import type { PluginSessionContext } from "../../lib/plugin-sdk";
import type { PluginInventory, PluginUiContribution } from "../../lib/types";

const contribution = (
  id: string,
  placement: PluginUiContribution["placement"],
): PluginUiContribution => ({
  id,
  label: id === "pill" ? "Pull requests" : id,
  placement,
  entrypoint: `ui/${id}.js`,
  order: null,
  shortcut: null,
});

function github(
  contributions: PluginUiContribution[],
  hostApiVersion = "planeai.plugin-host.v5",
): PluginInventory {
  return {
    id: "github",
    name: "GitHub",
    version: "0.1.0",
    host_api_version: hostApiVersion,
    source_kind: "local",
    backend_entrypoint: "bin/plugin",
    capabilities: [],
    ui_contributions: contributions,
    providers: [],
    installed_hash: "hash",
    installed_path: "/plugins/github",
    original_display_path: null,
    remote_origin: null,
    enabled: true,
    state: "running",
    last_error: null,
    log_path: null,
  };
}

const session: PluginSessionContext = {
  id: "s1",
  projectId: "p1",
  branch: "feat/login",
  baseBranch: "main",
  status: "active",
  provider: null,
  taskKey: "T-1",
};

let target: HTMLElement;
let component: ReturnType<typeof mount> | undefined;

afterEach(() => {
  if (component) unmount(component);
  component = undefined;
  target?.remove();
});

async function mountSlot(
  contributions: PluginUiContribution[],
  onOpenPanel = vi.fn(),
  hostApiVersion?: string,
) {
  target = document.createElement("div");
  document.body.append(target);
  const plugin = github(contributions, hostApiVersion);
  component = mount(SessionMetaSlot, {
    target,
    props: {
      item: { plugin, contribution: contributions[0] },
      session,
      onNavigate: () => {},
      onClose: () => {},
      onOpenPanel,
    },
  });
  const host = await vi.waitFor(() => {
    const next = target.querySelector<HTMLElement>("[data-plugin-ui-contribution]");
    expect(next?.shadowRoot?.querySelector("iframe")).toBeTruthy();
    return next!;
  });
  const frame = host.shadowRoot!.querySelector<HTMLIFrameElement>("iframe")!;
  const contentWindow = { postMessage: vi.fn() } as unknown as Window;
  Object.defineProperty(frame, "contentWindow", { configurable: true, value: contentWindow });
  const post = (data: unknown) => {
    window.dispatchEvent(new MessageEvent("message", { source: contentWindow, data }));
    flushSync();
  };
  return { onOpenPanel, post };
}

describe("SessionMetaSlot", () => {
  it("opens the plugin's session panel for its session without selecting the row", async () => {
    const { onOpenPanel } = await mountSlot([
      contribution("pill", "session.meta"),
      contribution("pull-request", "session.panel"),
    ]);
    const row = vi.fn();
    document.body.addEventListener("click", row);

    target.querySelector<HTMLButtonElement>("button")!.click();

    expect(onOpenPanel).toHaveBeenCalledWith("s1", "github", "pull-request");
    expect(row).not.toHaveBeenCalled();
    document.body.removeEventListener("click", row);
  });

  it("names the pill with the tooltip lines its plugin reports", async () => {
    const { post } = await mountSlot([
      contribution("pill", "session.meta"),
      contribution("pull-request", "session.panel"),
    ]);
    const button = target.querySelector<HTMLButtonElement>("button")!;
    expect(button.getAttribute("aria-label")).toBe("Pull requests");

    post({
      type: "tooltip",
      lines: [{ text: "#12 Fix login", tone: "success" }, { text: "#13 Add tests" }],
    });
    expect(button.getAttribute("aria-label")).toBe("Pull requests, #12 Fix login, #13 Add tests");

    post({ type: "tooltip", lines: "not lines" });
    expect(button.getAttribute("aria-label")).toBe("Pull requests, #12 Fix login, #13 Add tests");

    post({ type: "tooltip", lines: [] });
    expect(button.getAttribute("aria-label")).toBe("Pull requests");
  });

  it("closes its tooltip when the pill is activated", async () => {
    const { post } = await mountSlot([
      contribution("pill", "session.meta"),
      contribution("pull-request", "session.panel"),
    ]);
    post({ type: "tooltip", lines: [{ text: "#12 Fix login" }] });
    const button = target.querySelector<HTMLButtonElement>("button")!;
    const tooltipText = () =>
      [...document.querySelectorAll("[data-tooltip-content]")].map((node) => node.textContent);

    button.focus();
    await vi.waitFor(() => expect(tooltipText()).toEqual(["#12 Fix login"]));

    button.click();
    await vi.waitFor(() => expect(tooltipText()).toEqual([]));
  });

  it("stays a visual-only slot for a plugin on a host API older than v5", async () => {
    await mountSlot(
      [contribution("pill", "session.meta"), contribution("pull-request", "session.panel")],
      vi.fn(),
      "planeai.plugin-host.v4",
    );
    expect(target.querySelector("button")).toBeNull();
  });

  it("stays a visual-only slot when its plugin has no session panel", async () => {
    await mountSlot([contribution("pill", "session.meta")]);
    expect(target.querySelector("button")).toBeNull();
  });
});
