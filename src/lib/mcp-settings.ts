/** Mirrors `DEFAULT_MCP_PORT` in the backend config. */
export const DEFAULT_MCP_PORT = 47437;

/** A typed port, or `null` when it is not a whole number from 1 to 65535. */
export function parseMcpPort(raw: string): number | null {
  const port = Number(raw.trim());
  return Number.isInteger(port) && port >= 1 && port <= 65535 ? port : null;
}
