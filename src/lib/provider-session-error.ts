import { errorMessage } from "./errors";
import { PLUGIN_SESSION_ERROR_CODES, type PluginSessionErrorCode } from "./plugin-sdk";

/** A failed provider session request, with why it failed. */
export class ProviderSessionError extends Error {
  readonly code: PluginSessionErrorCode;

  constructor(code: PluginSessionErrorCode, message: string) {
    super(message);
    this.name = "ProviderSessionError";
    this.code = code;
  }
}

/** The host's `{ code, message }` rejection as a {@link ProviderSessionError}; anything else as is. */
export function toProviderSessionError(error: unknown): unknown {
  if (typeof error !== "object" || error === null) return error;
  const { code, message } = error as Record<string, unknown>;
  const known = PLUGIN_SESSION_ERROR_CODES.find((candidate) => candidate === code);
  return known && typeof message === "string" ? new ProviderSessionError(known, message) : error;
}

/** How a failed request reaches a plugin frame: its message, and its code when it has one. */
export function frameFailure(error: unknown): { error: string; code?: PluginSessionErrorCode } {
  return error instanceof ProviderSessionError
    ? { error: error.message, code: error.code }
    : { error: errorMessage(error) };
}
