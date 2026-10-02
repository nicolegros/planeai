/** Characters a shell reads literally; everything else in a path is escaped. */
const SAFE = /[\p{L}\p{N}_\-.,/:@%+=]/u;

/**
 * Paths dropped on a terminal, as typed text: shell-escaped, space-separated and
 * followed by a space, as Terminal.app and iTerm do. A path with a control
 * character is single-quoted instead, since a backslash before a newline would
 * be read as a line continuation.
 */
export function droppedPathsText(paths: string[]): string {
  return paths.map(escapePath).join(" ") + " ";
}

function escapePath(path: string): string {
  if (/[\x00-\x1f\x7f]/.test(path)) return `'${path.replaceAll("'", `'\\''`)}'`;
  return Array.from(path, (char) => (SAFE.test(char) ? char : `\\${char}`)).join("");
}

/**
 * Viewport point of a native drag-drop position. Tauri labels it physical, but
 * only WebView2 (Windows) reports physical pixels; WKWebView and WebKitGTK
 * report logical points.
 */
export function dropPositionToViewport(
  position: { x: number; y: number },
  platform: { windows: boolean; pixelRatio: number },
): { x: number; y: number } {
  const scale = platform.windows ? platform.pixelRatio : 1;
  return { x: position.x / scale, y: position.y / scale };
}
