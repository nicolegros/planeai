import { describe, expect, it } from "vitest";
import type { AppConfig } from "../settings.svelte";
import { PROVIDER_PRESETS } from "../provider-presets";
import { initialAgentSelection, onboardingAgentsPatch, shouldShowOnboarding } from "../onboarding";

const preset = (key: string) =>
  PROVIDER_PRESETS.find((candidate) => candidate.key === key)!.provider;

function config(overrides: Partial<AppConfig> = {}): AppConfig {
  return {
    appearance: { mode: "system", theme: "default" },
    terminal: { font_family: "Menlo", font_size: 14, option_as_meta: true },
    providers: {
      kiro: preset("kiro"),
      claude: preset("claude"),
      copilot: preset("copilot"),
      codex: preset("codex"),
    },
    default_provider: "kiro",
    ...overrides,
  };
}

describe("shouldShowOnboarding", () => {
  it("shows only when the backend says setup has not been done", () => {
    expect(shouldShowOnboarding(config({ onboarding_completed: false }))).toBe(true);
    expect(shouldShowOnboarding(config({ onboarding_completed: true }))).toBe(false);
    // Before the config has loaded the field is absent; never flash the wizard then.
    expect(shouldShowOnboarding(config())).toBe(false);
  });
});

describe("initialAgentSelection", () => {
  it("selects the presets found on PATH, in preset order", () => {
    const selection = initialAgentSelection(config(), {
      codex: "/bin/codex",
      claude: "/bin/claude",
      kiro: null,
    });
    expect(selection.selected).toEqual(["claude", "codex"]);
  });

  it("keeps the configured default when it was found, otherwise the first found agent", () => {
    expect(
      initialAgentSelection(config({ default_provider: "codex" }), { claude: "/c", codex: "/x" })
        .defaultKey,
    ).toBe("codex");
    expect(initialAgentSelection(config(), { claude: "/c", codex: "/x" }).defaultKey).toBe(
      "claude",
    );
  });

  it("selects nothing when no agent is installed", () => {
    expect(initialAgentSelection(config(), { kiro: null, claude: null })).toEqual({
      selected: [],
      defaultKey: null,
    });
  });
});

describe("onboardingAgentsPatch", () => {
  it("keeps only the selected presets and sets the default", () => {
    expect(onboardingAgentsPatch(config(), ["claude", "codex"], "codex")).toEqual({
      providers: { claude: preset("claude"), codex: preset("codex") },
      default_provider: "codex",
    });
  });

  it("keeps edits to selected presets and leaves custom agents alone", () => {
    const edited = { ...preset("claude"), command: "claude --model opus" };
    const aider = { command: "aider", yolo_flag: "--yes" };
    const current = config({ providers: { claude: edited, aider } });
    expect(onboardingAgentsPatch(current, ["claude", "codex"], "claude")).toEqual({
      providers: { claude: edited, aider, codex: preset("codex") },
      default_provider: "claude",
    });
  });

  it("falls back to the first selected agent when the chosen default is not selected", () => {
    expect(onboardingAgentsPatch(config(), ["copilot"], "kiro")?.default_provider).toBe("copilot");
  });

  it("changes nothing when no agent is selected, so a default always exists", () => {
    expect(onboardingAgentsPatch(config(), [], null)).toBeNull();
  });
});
