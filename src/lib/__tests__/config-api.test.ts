import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...args: unknown[]) => invoke(...args) }));

const { config } = await import("../api");

describe("config api", () => {
  beforeEach(() => invoke.mockReset());

  it("fetches backend defaults", async () => {
    invoke.mockResolvedValue({ default_provider: "kiro" });

    await expect(config.defaults()).resolves.toEqual({ default_provider: "kiro" });
    expect(invoke).toHaveBeenCalledWith("get_config_defaults");
  });

  it("detects agent binaries", async () => {
    invoke.mockResolvedValue({ claude: "/opt/homebrew/bin/claude", kiro: null });

    await expect(config.detectProviders()).resolves.toEqual({
      claude: "/opt/homebrew/bin/claude",
      kiro: null,
    });
    expect(invoke).toHaveBeenCalledWith("detect_providers");
  });
});
