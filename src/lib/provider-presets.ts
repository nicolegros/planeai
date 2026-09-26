import type { Provider } from "./settings.svelte";

export interface ProviderPreset {
  key: string;
  label: string;
  provider: Provider;
}

/// Mirrors the backend's default providers (`Config::default` in config.rs) so
/// existing configs can add a supported agent in one click.
export const PROVIDER_PRESETS: readonly ProviderPreset[] = [
  {
    key: "kiro",
    label: "Kiro",
    provider: {
      command: "kiro-cli chat",
      yolo_flag: "--trust-all-tools",
      resume_command: "kiro-cli chat --resume",
      prompt_command: "{prompt}",
    },
  },
  {
    key: "claude",
    label: "Claude Code",
    provider: {
      command: "claude",
      yolo_flag: "--dangerously-skip-permissions",
      resume_command: "claude --resume",
      prompt_command: "-p {prompt}",
    },
  },
  {
    key: "copilot",
    label: "Copilot",
    provider: {
      command: "copilot --resume",
      yolo_flag: "--allow-all-tools",
      prompt_command: "{prompt}",
    },
  },
  {
    key: "codex",
    label: "Codex",
    provider: {
      command: "codex",
      yolo_flag: "--dangerously-bypass-approvals-and-sandbox",
      resume_command: "codex resume --last",
      prompt_command: "{prompt}",
    },
  },
];
