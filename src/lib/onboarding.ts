import type { AppConfig } from "./settings.svelte";
import { PROVIDER_PRESETS } from "./provider-presets";

/** `false` only once the backend has loaded a config that still needs first-run setup. */
export function shouldShowOnboarding(config: AppConfig): boolean {
  return config.onboarding_completed === false;
}

export interface AgentSelection {
  /** Preset keys, in preset order. */
  selected: string[];
  defaultKey: string | null;
}

/** Starts from the presets found on PATH, keeping the configured default when it is among them. */
export function initialAgentSelection(
  config: AppConfig,
  detected: Record<string, string | null>,
): AgentSelection {
  const selected = PROVIDER_PRESETS.map((preset) => preset.key).filter((key) => detected[key]);
  const defaultKey = selected.includes(config.default_provider)
    ? config.default_provider
    : (selected[0] ?? null);
  return { selected, defaultKey };
}

/**
 * Turns the selected presets on and the others off, keeping edits to selected presets and every
 * custom agent. `null` when nothing is selected: the config keeps its agents so a default exists.
 */
export function onboardingAgentsPatch(
  config: AppConfig,
  selected: readonly string[],
  defaultKey: string | null,
): Partial<AppConfig> | null {
  if (selected.length === 0) return null;
  const presetKeys = new Set(PROVIDER_PRESETS.map((preset) => preset.key));
  const providers = Object.fromEntries(
    Object.entries(config.providers).filter(
      ([key]) => !presetKeys.has(key) || selected.includes(key),
    ),
  );
  for (const preset of PROVIDER_PRESETS) {
    if (selected.includes(preset.key) && !providers[preset.key])
      providers[preset.key] = { ...preset.provider };
  }
  return {
    providers,
    default_provider: defaultKey && selected.includes(defaultKey) ? defaultKey : selected[0],
  };
}
