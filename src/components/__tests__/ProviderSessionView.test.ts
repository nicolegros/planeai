import { afterEach, describe, expect, it, vi } from "vitest";
import { flushSync, mount, unmount } from "svelte";

vi.mock("../../lib/api", () => ({
  plugins: {
    call: vi.fn(),
    hostCall: vi.fn(),
    localUiSource: vi.fn(),
    localProviderUiSource: vi.fn(() =>
      Promise.resolve("export default { mount() { return () => {}; } };"),
    ),
    settings: vi.fn(),
    updateSettings: vi.fn(),
    dataChanged: vi.fn(),
  },
  providerSessions: { ensure: vi.fn(() => Promise.resolve()), send: vi.fn(), interrupt: vi.fn() },
  projects: { list: vi.fn() },
  tasks: { create: vi.fn() },
}));
vi.mock("../../lib/snackbar.svelte", () => ({ showSnackbar: vi.fn() }));
vi.mock("../../lib/settings.svelte", () => ({ isDark: () => false }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));

import Harness from "./ProviderSessionViewHarness.svelte";
import type { PluginInventory, Session } from "../../lib/types";

function plugin(id: string, overrides: Partial<PluginInventory> = {}): PluginInventory {
  return {
    id,
    name: id,
    version: "0.1.0",
    host_api_version: "planeai.plugin-host.v3",
    source_kind: "local",
    backend_entrypoint: "bin/sidecar",
    capabilities: ["providers"],
    ui_contributions: [],
    providers: [{ id: "chat", label: "Chat", entrypoint: "ui/chat.js", supports: [] }],
    installed_hash: "hash",
    installed_path: `/planeai/plugins/packages/sha256/${id}`,
    original_display_path: null,
    enabled: true,
    state: "running",
    last_error: null,
    log_path: null,
    ...overrides,
  };
}

const session = {
  id: "session-1",
  project_id: "project-1",
  branch: "feat/chat",
  base_branch: "main",
  status: "active",
  provider: "chatter:chat",
  task_key: null,
} as unknown as Session;

describe("ProviderSessionView", () => {
  let target: HTMLElement;
  let component: ReturnType<typeof mount> | undefined;
  afterEach(() => {
    if (component) unmount(component);
    component = undefined;
    target?.remove();
  });

  function render(inventory: PluginInventory[], current: Session = session) {
    target = document.createElement("div");
    document.body.append(target);
    const onOpenPreferences = vi.fn();
    const harness = mount(Harness, {
      target,
      props: { initialInventory: inventory, initialSession: current, onOpenPreferences },
    });
    component = harness;
    return { harness, onOpenPreferences };
  }

  const frame = () =>
    target
      .querySelector<HTMLElement>("[data-plugin-ui-contribution]")
      ?.shadowRoot?.querySelector("iframe") ?? null;

  it("keeps the chat mounted when another plugin's inventory entry changes", async () => {
    const { harness } = render([plugin("chatter"), plugin("github", { capabilities: [] })]);
    const mounted = await vi.waitFor(() => {
      expect(frame()).toBeTruthy();
      return frame();
    });
    harness.setInventory([
      plugin("chatter"),
      plugin("github", { capabilities: [], state: "error", last_error: "token expired" }),
    ]);
    flushSync();
    await Promise.resolve();
    expect(frame()).toBe(mounted);
  });

  it("keeps the chat mounted when the session list reloads with an equal session", async () => {
    const { harness } = render([plugin("chatter")]);
    const mounted = await vi.waitFor(() => {
      expect(frame()).toBeTruthy();
      return frame();
    });
    harness.setSession({ ...session });
    flushSync();
    await Promise.resolve();
    expect(frame()).toBe(mounted);
  });

  it("shows that an exited session has ended instead of loading a chat that cannot start", () => {
    render([plugin("chatter")], { ...session, status: "exited" } as Session);
    expect(frame()).toBeNull();
    expect(target.querySelector("[role=status]")?.textContent).toContain("This session has exited");
  });

  it("opens the Plugins page when the provider's plugin is unavailable", () => {
    const { onOpenPreferences } = render([plugin("chatter", { state: "disabled" })]);
    const button = [...target.querySelectorAll("button")].find(
      (candidate) => candidate.textContent?.trim() === "Open Plugins",
    )!;
    button.click();
    expect(onOpenPreferences).toHaveBeenCalledWith({ kind: "category", id: "plugins" });
  });
});
