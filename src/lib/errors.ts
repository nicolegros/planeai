/** What to show for a rejection: an error's message, or the value itself, as Tauri rejects with strings. */
export function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
