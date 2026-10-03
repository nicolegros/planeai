import type { AppConfig } from "./settings.svelte";
import { PROVIDER_PRESETS } from "./provider-presets";

/** True once the loaded config has `onboarding_completed: false`; the field is absent before load. */
export function shouldShowOnboarding(config: AppConfig): boolean {
  return config.onboarding_completed === false;
}

export interface AgentSelection {
  /** Preset keys, in preset order. */
  selected: string[];
  /** A selected preset, a custom agent already configured, or `null`. */
  defaultKey: string | null;
}

const PRESET_KEYS = PROVIDER_PRESETS.map((preset) => preset.key);

function isCustom(key: string | null): boolean {
  return key !== null && !PRESET_KEYS.includes(key);
}

/** Keeps the default while it is still valid, otherwise falls back to the first selected preset. */
function withValidDefault(selected: readonly string[], defaultKey: string | null): AgentSelection {
  const valid = defaultKey !== null && (isCustom(defaultKey) || selected.includes(defaultKey));
  return { selected: [...selected], defaultKey: valid ? defaultKey : (selected[0] ?? null) };
}

/** Starts from the presets found on PATH, keeping the configured default when it is still usable. */
export function initialAgentSelection(
  config: AppConfig,
  detected: Record<string, string | null>,
): AgentSelection {
  const configured =
    isCustom(config.default_provider) && !(config.default_provider in config.providers)
      ? null
      : config.default_provider;
  return withValidDefault(
    PRESET_KEYS.filter((key) => !!detected[key]),
    configured,
  );
}

export function toggleAgent(selection: AgentSelection, key: string): AgentSelection {
  const selected = PRESET_KEYS.filter(
    (candidate) => (candidate === key) !== selection.selected.includes(candidate),
  );
  return withValidDefault(selected, selection.defaultKey);
}

/** After re-detection, adds the presets that were not found before and keeps everything else. */
export function mergeDetection(
  selection: AgentSelection,
  previous: Record<string, string | null>,
  next: Record<string, string | null>,
): AgentSelection {
  const selected = PRESET_KEYS.filter(
    (key) => selection.selected.includes(key) || (!!next[key] && !previous[key]),
  );
  return withValidDefault(selected, selection.defaultKey);
}

/** Default-agent choices: the selected presets plus the custom agents already configured. */
export function defaultAgentOptions(
  config: AppConfig,
  selected: readonly string[],
): { value: string; label: string }[] {
  const presets = PROVIDER_PRESETS.filter((preset) => selected.includes(preset.key)).map(
    (preset) => ({ value: preset.key, label: preset.label }),
  );
  const custom = Object.keys(config.providers)
    .filter(isCustom)
    .map((key) => ({ value: key, label: key }));
  return [...presets, ...custom];
}

/**
 * Turns the selected presets on and the others off, keeping edits to selected presets and every
 * custom agent. With no preset selected the config keeps its agents, so only a new custom default is saved.
 */
export function onboardingAgentsPatch(
  config: AppConfig,
  selected: readonly string[],
  defaultKey: string | null,
): Partial<AppConfig> | null {
  if (selected.length === 0) {
    const newCustomDefault =
      isCustom(defaultKey) &&
      defaultKey! in config.providers &&
      defaultKey !== config.default_provider;
    return newCustomDefault ? { default_provider: defaultKey! } : null;
  }
  const providers = Object.fromEntries(
    Object.entries(config.providers).filter(([key]) => isCustom(key) || selected.includes(key)),
  );
  for (const preset of PROVIDER_PRESETS) {
    if (selected.includes(preset.key) && !providers[preset.key])
      providers[preset.key] = { ...preset.provider };
  }
  return {
    providers,
    // Non-empty selection, so a default always exists.
    default_provider: withValidDefault(selected, defaultKey).defaultKey!,
  };
}
