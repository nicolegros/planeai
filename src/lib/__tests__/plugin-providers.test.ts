import { describe, expect, it } from "vitest";
import {
  findRuntimeProvider,
  providerContribution,
  runtimeProviders,
  supportsYolo,
} from "../plugin-providers";
import type { PluginInventory } from "../types";

function plugin(overrides: Partial<PluginInventory>): PluginInventory {
  return {
    id: "claude-headless",
    name: "Claude Headless",
    version: "0.1.0",
    host_api_version: "planeai.plugin-host.v3",
    source_kind: "local",
    backend_entrypoint: "bin/plugin",
    capabilities: ["providers"],
    ui_contributions: [],
    providers: [
      { id: "claude", label: "Claude (chat)", entrypoint: "ui/chat.js", supports: ["yolo"] },
    ],
    installed_hash: null,
    installed_path: null,
    original_display_path: null,
    enabled: true,
    state: "running",
    last_error: null,
    log_path: null,
    ...overrides,
  };
}

describe("plugin providers", () => {
  it("offers providers only from running plugins with the providers capability", () => {
    const offered = runtimeProviders([
      plugin({}),
      plugin({ id: "stopped", state: "disabled" }),
      plugin({ id: "ungranted", capabilities: [] }),
    ]);
    expect(offered.map((provider) => provider.key)).toEqual(["claude-headless:claude"]);
  });

  it("resolves a session's provider even when its plugin is not running", () => {
    const stopped = plugin({ state: "error", last_error: "crashed" });
    expect(findRuntimeProvider([stopped], "claude-headless:claude")?.plugin).toBe(stopped);
    expect(findRuntimeProvider([stopped], "claude-headless:other")).toBeNull();
    expect(findRuntimeProvider([stopped], "claude")).toBeNull();
    expect(findRuntimeProvider([stopped], null)).toBeNull();
  });

  it("synthesizes a session.main contribution from the provider declaration", () => {
    const provider = plugin({}).providers[0];
    expect(providerContribution(provider)).toEqual({
      id: "claude",
      label: "Claude (chat)",
      placement: "session.main",
      entrypoint: "ui/chat.js",
      order: null,
      shortcut: null,
    });
    expect(supportsYolo(provider)).toBe(true);
    expect(supportsYolo({ ...provider, supports: [] })).toBe(false);
  });
});
