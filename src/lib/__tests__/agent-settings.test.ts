import { describe, expect, it } from "vitest";
import type { AppConfig } from "../settings.svelte";
import {
  addCustomAgentPatch,
  agentRows,
  disableAgentPatch,
  enableAgentPatch,
  setDefaultAgentPatch,
} from "../agent-settings";
import { PROVIDER_PRESETS } from "../provider-presets";

function config(overrides: Partial<AppConfig> = {}): AppConfig {
  return {
    appearance: { mode: "system", theme: "default" },
    terminal: { font_family: "Menlo", font_size: 14, option_as_meta: true },
    providers: {
      claude: { command: "claude --model opus", yolo_flag: "--dangerously-skip-permissions" },
      aider: { command: "aider", yolo_flag: "--yes" },
    },
    default_provider: "claude",
    ...overrides,
  };
}

describe("agentRows", () => {
  it("always lists every preset first, then custom agents", () => {
    const rows = agentRows(config(), {});
    expect(rows.map((row) => row.key)).toEqual([
      ...PROVIDER_PRESETS.map((preset) => preset.key),
      "aider",
    ]);
  });

  it("marks presets present in the config as enabled and keeps their edits", () => {
    const claude = agentRows(config(), {}).find((row) => row.key === "claude")!;
    expect(claude.enabled).toBe(true);
    expect(claude.provider?.command).toBe("claude --model opus");
    expect(claude.custom).toBe(false);
    const kiro = agentRows(config(), {}).find((row) => row.key === "kiro")!;
    expect(kiro.enabled).toBe(false);
    expect(kiro.provider).toBeNull();
  });

  it("labels presets by name and custom agents by key", () => {
    const rows = agentRows(config(), {});
    expect(rows.find((row) => row.key === "claude")!.label).toBe("Claude Code");
    expect(rows.find((row) => row.key === "aider")!).toMatchObject({
      label: "aider",
      custom: true,
      enabled: true,
    });
  });

  it("locks the default agent's switch", () => {
    const claude = agentRows(config(), {}).find((row) => row.key === "claude")!;
    expect(claude.isDefault).toBe(true);
    expect(claude.lockedReason).toBe("Choose another default first");
    expect(agentRows(config(), {}).find((row) => row.key === "aider")!.lockedReason).toBeNull();
  });

  it("reports detection, distinguishing not found from not checked yet", () => {
    const rows = agentRows(config(), { claude: "/opt/homebrew/bin/claude", kiro: null });
    expect(rows.find((row) => row.key === "claude")!.detection).toEqual({
      state: "found",
      path: "/opt/homebrew/bin/claude",
    });
    expect(rows.find((row) => row.key === "kiro")!.detection).toEqual({ state: "missing" });
    expect(rows.find((row) => row.key === "aider")!.detection).toEqual({ state: "unknown" });
  });
});

describe("enable and disable", () => {
  it("enabling a preset writes its full preset entry", () => {
    const codex = PROVIDER_PRESETS.find((preset) => preset.key === "codex")!;
    expect(enableAgentPatch(config(), "codex")).toEqual({
      providers: { ...config().providers, codex: codex.provider },
    });
  });

  it("enabling makes the agent the default when the current default is gone", () => {
    const broken = config({ providers: {}, default_provider: "kiro" });
    expect(enableAgentPatch(broken, "codex")).toMatchObject({ default_provider: "codex" });
  });

  it("disabling removes the entry", () => {
    const { aider: _removed, ...rest } = config().providers;
    expect(disableAgentPatch(config(), "aider")).toEqual({ providers: rest });
  });

  it("refuses to disable the default agent", () => {
    expect(disableAgentPatch(config(), "claude")).toBeNull();
  });
});

describe("custom agents", () => {
  it("adds a trimmed custom agent", () => {
    expect(
      addCustomAgentPatch(config(), { name: " goose ", command: " goose session ", yoloFlag: "" }),
    ).toEqual({
      ok: true,
      patch: {
        providers: { ...config().providers, goose: { command: "goose session", yolo_flag: null } },
      },
    });
  });

  it("rejects a missing name or command and duplicate or preset names", () => {
    expect(addCustomAgentPatch(config(), { name: "", command: "x", yoloFlag: "" })).toEqual({
      ok: false,
      error: "Name is required.",
    });
    expect(addCustomAgentPatch(config(), { name: "x", command: " ", yoloFlag: "" })).toEqual({
      ok: false,
      error: "Command is required.",
    });
    expect(addCustomAgentPatch(config(), { name: "aider", command: "x", yoloFlag: "" })).toEqual({
      ok: false,
      error: "An agent named aider already exists.",
    });
    expect(addCustomAgentPatch(config(), { name: "kiro", command: "x", yoloFlag: "" })).toEqual({
      ok: false,
      error: "kiro is a built-in agent. Turn it on from the list instead.",
    });
  });
});

describe("setDefaultAgentPatch", () => {
  it("only accepts enabled agents", () => {
    expect(setDefaultAgentPatch(config(), "aider")).toEqual({ default_provider: "aider" });
    expect(setDefaultAgentPatch(config(), "kiro")).toBeNull();
  });
});
