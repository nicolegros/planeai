import { isReservedProviderKey, supportsYolo, type RuntimeProvider } from "./plugin-providers";
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
  /** Why auto-approve is off for this provider, or `undefined` when it can be used. */
  readonly autoApproveBlocked: string | undefined;
  readonly #runtimeProviders: () => RuntimeProvider[];

  constructor(runtimeProviders: () => RuntimeProvider[]) {
    this.#runtimeProviders = runtimeProviders;
    this.keys = $derived([
      // Saving the config refuses these anyway; a plugin's provider is listed below.
      ...Object.keys(getSettings().providers ?? {}).filter((key) => !isReservedProviderKey(key)),
      ...runtimeProviders().map((provider) => provider.key),
    ]);
    this.key = $derived(this.selected || getSettings().default_provider);
    this.autoApproveBlocked = $derived.by(() => {
      const chosen = runtimeProviders().find((provider) => provider.key === this.key);
      return chosen && !supportsYolo(chosen.provider)
        ? `${chosen.provider.label} does not support auto-approve`
        : undefined;
    });
  }

  /** Auto-approve as the session gets it: the user's choice, when the provider supports it. */
  autoApprove(requested: boolean): boolean {
    return requested && !this.autoApproveBlocked;
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
