/** Characters a POSIX shell reads literally; everything else in a path is escaped. */
const POSIX_SAFE = /[\p{L}\p{N}_\-.,/:@%+=]/u;
/**
 * Typed bytes reach the terminal's line editor before any shell quoting is
 * parsed, so a control character (Ctrl-U, a newline, ESC) in a file name could
 * rewrite or submit the line. Such paths are never typed.
 */
const CONTROL = /[\x00-\x1f\x7f-\x9f]/;
/**
 * Inside double quotes cmd still expands `%VAR%` and PowerShell `$var`, `$(...)`
 * and backtick escapes, so a Windows path holding any of these is never typed.
 */
const WINDOWS_UNQUOTABLE = /["%$`]/;

export interface DroppedPathsText {
  /** Paths as typed: quoted, space-separated and followed by a space. Empty when none can be typed. */
  text: string;
  /** Paths that cannot be typed safely. */
  skipped: string[];
}

/**
 * Paths dropped on a terminal, as typed text, the way Terminal.app and iTerm do.
 * POSIX shells get backslash escapes; Windows shells (cmd, PowerShell) get
 * double quotes.
 */
export function droppedPathsText(
  paths: string[],
  platform: { windows: boolean },
): DroppedPathsText {
  const typeable = (path: string) =>
    !CONTROL.test(path) && !(platform.windows && WINDOWS_UNQUOTABLE.test(path));
  const typed = paths.filter(typeable);
  const quote = platform.windows ? (path: string) => `"${path}"` : escapePosix;
  return {
    text: typed.length > 0 ? typed.map(quote).join(" ") + " " : "",
    skipped: paths.filter((path) => !typeable(path)),
  };
}

function escapePosix(path: string): string {
  return Array.from(path, (char) => (POSIX_SAFE.test(char) ? char : `\\${char}`)).join("");
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
