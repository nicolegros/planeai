import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flushSync, mount, unmount } from "svelte";
import type { McpStatus } from "../../../lib/api";

const reads: Array<(status: McpStatus) => void> = [];
const settingsChanged: Array<() => void> = [];

vi.mock("../../../lib/api", () => ({
  preferences: {
    getMcpStatus: vi.fn(() => new Promise<McpStatus>((resolve) => reads.push(resolve))),
  },
}));
vi.mock("../../../lib/settings.svelte", () => ({
  getSettings: () => ({ mcp_enabled: null, mcp_port: null }),
}));
vi.mock("../../../lib/save-settings", () => ({ saveSettings: vi.fn(() => Promise.resolve(true)) }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn((event: string, handler: () => void) => {
    if (event === "settings-changed") settingsChanged.push(handler);
    return Promise.resolve(() => {});
  }),
}));

import McpServerSettings from "../McpServerSettings.svelte";

const listening = (port: number): McpStatus => ({
  default_port: 47437,
  state: "listening",
  port,
  url: `http://127.0.0.1:${port}/mcp`,
});

let component: ReturnType<typeof mount> | undefined;
beforeEach(() => {
  reads.length = 0;
  settingsChanged.length = 0;
  component = mount(McpServerSettings, { target: document.body });
  flushSync();
});
afterEach(() => {
  if (component) unmount(component);
  document.body.innerHTML = "";
});

async function settle() {
  await Promise.resolve();
  await Promise.resolve();
  flushSync();
}

describe("McpServerSettings", () => {
  it("reads the status once on mount and once per settings change", async () => {
    await settle();
    expect(reads.length).toBe(1);
    settingsChanged.forEach((handler) => handler());
    await settle();
    expect(reads.length).toBe(2);
  });

  it("shows the latest read even when an earlier one answers last", async () => {
    await settle();
    settingsChanged.forEach((handler) => handler());
    await settle();
    reads[1](listening(50000));
    await settle();
    reads[0](listening(47437));
    await settle();
    expect(document.body.textContent).toContain("http://127.0.0.1:50000/mcp");
  });
});
