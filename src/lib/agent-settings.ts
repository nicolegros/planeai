import type { AppConfig, Provider } from "./settings.svelte";
import { PROVIDER_PRESETS } from "./provider-presets";

/** Result of `config.detectProviders()`: a path, `null` when not found, absent while unchecked. */
export type AgentDetection =
  | { state: "found"; path: string }
  | { state: "missing" }
  | { state: "unknown" };

export interface AgentRow {
  key: string;
  label: string;
  /** `null` for a preset that is turned off (absent from `providers`). */
  provider: Provider | null;
  custom: boolean;
  enabled: boolean;
  isDefault: boolean;
  /** Why the enable switch cannot be turned off, or `null` when it can. */
  lockedReason: string | null;
  detection: AgentDetection;
}

function detectionFor(key: string, detected: Record<string, string | null>): AgentDetection {
  if (!(key in detected)) return { state: "unknown" };
  const path = detected[key];
  return path ? { state: "found", path } : { state: "missing" };
}

/** Presets are always listed (on = present in `providers`), followed by custom agents. */
export function agentRows(config: AppConfig, detected: Record<string, string | null>): AgentRow[] {
  const row = (key: string, label: string, custom: boolean): AgentRow => {
    const provider = config.providers[key] ?? null;
    const isDefault = config.default_provider === key && provider !== null;
    return {
      key,
      label,
      provider,
      custom,
      enabled: provider !== null,
      isDefault,
      lockedReason: isDefault ? "Choose another default first" : null,
      detection: detectionFor(key, detected),
    };
  };
  const presetKeys = new Set(PROVIDER_PRESETS.map((preset) => preset.key));
  const custom = Object.keys(config.providers)
    .filter((key) => !presetKeys.has(key))
    .sort((a, b) => a.localeCompare(b));
  return [
    ...PROVIDER_PRESETS.map((preset) => row(preset.key, preset.label, false)),
    ...custom.map((key) => row(key, key, true)),
  ];
}

export function enableAgentPatch(config: AppConfig, key: string): Partial<AppConfig> {
  const preset = PROVIDER_PRESETS.find((candidate) => candidate.key === key);
  if (!preset) return {};
  const patch: Partial<AppConfig> = {
    providers: { ...config.providers, [key]: { ...preset.provider } },
  };
  if (!(config.default_provider in config.providers)) patch.default_provider = key;
  return patch;
}

/** Removes the agent; `null` when it is the default (which also covers the last remaining agent). */
export function disableAgentPatch(config: AppConfig, key: string): Partial<AppConfig> | null {
  if (config.default_provider === key) return null;
  const { [key]: _removed, ...providers } = config.providers;
  return { providers };
}

export function setDefaultAgentPatch(config: AppConfig, key: string): Partial<AppConfig> | null {
  return key in config.providers ? { default_provider: key } : null;
}

export interface CustomAgentDraft {
  name: string;
  command: string;
  yoloFlag: string;
}

export function addCustomAgentPatch(
  config: AppConfig,
  draft: CustomAgentDraft,
): { ok: true; patch: Partial<AppConfig> } | { ok: false; error: string } {
  const name = draft.name.trim();
  const command = draft.command.trim();
  if (!name) return { ok: false, error: "Name is required." };
  if (!command) return { ok: false, error: "Command is required." };
  if (PROVIDER_PRESETS.some((preset) => preset.key === name)) {
    return { ok: false, error: `${name} is a built-in agent. Turn it on from the list instead.` };
  }
  if (name in config.providers)
    return { ok: false, error: `An agent named ${name} already exists.` };
  return {
    ok: true,
    patch: {
      providers: {
        ...config.providers,
        [name]: { command, yolo_flag: draft.yoloFlag.trim() || null },
      },
    },
  };
}
