import type { PluginSessionErrorCode } from "./plugin-sdk";

/** A failed provider session request; it reads as its message, as the host's string errors do. */
export class ProviderSessionError extends Error {
  readonly code: PluginSessionErrorCode;

  constructor(code: PluginSessionErrorCode, message: string) {
    super(message);
    this.name = "ProviderSessionError";
    this.code = code;
  }

  override toString(): string {
    return this.message;
  }
}

/** The host's `{ code, message }` rejection as a {@link ProviderSessionError}; anything else as is. */
export function providerSessionError(error: unknown): unknown {
  const { code, message } = (error ?? {}) as { code?: unknown; message?: unknown };
  return typeof code === "string" && typeof message === "string"
    ? new ProviderSessionError(code as PluginSessionErrorCode, message)
    : error;
}
