import { supportsYolo, type RuntimeProvider } from "./plugin-providers";
import { getSettings } from "./settings.svelte";

/**
 * A new session's provider, picked among the configured providers and those running
 * plugins offer. Session and task forms share it, so both list the same providers.
 */
export class ProviderChoice {
  /** Empty until the user picks one: the configured default applies. */
  selected = $state("");
  readonly keys: string[];
  readonly key: string;
  readonly runtimeProvider: RuntimeProvider | null;
  readonly autoApproveSupported: boolean;
  readonly #runtimeProviders: () => RuntimeProvider[];

  constructor(runtimeProviders: () => RuntimeProvider[]) {
    this.#runtimeProviders = runtimeProviders;
    this.keys = $derived([
      ...Object.keys(getSettings().providers ?? {}),
      ...runtimeProviders().map((provider) => provider.key),
    ]);
    this.key = $derived(this.selected || getSettings().default_provider);
    this.runtimeProvider = $derived(
      runtimeProviders().find((provider) => provider.key === this.key) ?? null,
    );
    this.autoApproveSupported = $derived(
      !this.runtimeProvider || supportsYolo(this.runtimeProvider.provider),
    );
  }

  label(key: string): string {
    return this.#runtimeProviders().find((provider) => provider.key === key)?.provider.label ?? key;
  }

  cycle(step: 1 | -1): void {
    const count = this.keys.length;
    if (count === 0) return;
    this.selected = this.keys[(this.keys.indexOf(this.key) + step + count) % count];
  }
}
