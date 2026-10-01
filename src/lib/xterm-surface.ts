import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { WebglAddon } from "@xterm/addon-webgl";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import { openUrl } from "@tauri-apps/plugin-opener";
import "@xterm/xterm/css/xterm.css";
import { getSettings, getTerminalSettings, isDark } from "./settings.svelte";
import { extractTerminalTheme } from "./theme-loader";
import { matchTerminalKey } from "./terminal-keys";
import type { TerminalGeometry, TerminalSurface, TerminalSurfaceHandlers } from "./terminal-view";

const SCROLLBACK_LINES = 20_000;
const FONT_TIMEOUT_MS = 3000;
const encoder = new TextEncoder();
// Contour's color scheme protocol: apps subscribe to dark/light change reports.
const COLOR_SCHEME_MODE = 2031;
const COLOR_SCHEME_QUERY = 996;

function fontStack(primary: string): string {
  const quoted = `"${primary.replace(/\\/g, "\\\\").replace(/"/g, '\\"')}"`;
  return `${quoted}, "Symbols Nerd Font Mono", monospace`;
}

/**
 * xterm measures cells when it opens; opening before the configured font has
 * loaded bakes fallback metrics into the renderer.
 */
export async function waitForFont(family: string, size: number): Promise<void> {
  const timeout = new Promise<void>((resolve) => setTimeout(resolve, FONT_TIMEOUT_MS));
  const fontLoad = (async () => {
    try {
      await document.fonts.load(`${size}px "${family}"`);
    } catch {
      await document.fonts.ready;
    }
  })();
  await Promise.race([fontLoad, timeout]);
}

type CsiParser = {
  registerCsiHandler?: (
    id: { prefix?: string; intermediates?: string; final: string },
    cb: (params: (number | number[])[]) => boolean,
  ) => { dispose(): void };
  registerEscHandler?: (id: { final: string }, cb: () => boolean) => { dispose(): void };
};

/** Production surface: xterm.js with fit, links, unicode 11, and WebGL while visible. */
export function createXtermSurface(handlers: TerminalSurfaceHandlers): TerminalSurface {
  const settings = getSettings();
  const term = new Terminal({
    cursorBlink: true,
    fontSize: settings.terminal.font_size,
    fontFamily: fontStack(settings.terminal.font_family),
    theme: extractTerminalTheme(),
    scrollback: settings.scrollback_lines ?? SCROLLBACK_LINES,
    scrollOnUserInput: false,
    allowProposedApi: true,
    macOptionIsMeta: settings.terminal.option_as_meta,
    linkHandler: {
      activate: (_event, uri) => {
        openUrl(uri).catch(() => {});
      },
    },
  });
  const fitAddon = new FitAddon();
  term.loadAddon(fitAddon);
  term.loadAddon(new Unicode11Addon());
  term.unicode.activeVersion = "11";
  if (settings.web_links !== false) {
    term.loadAddon(
      new WebLinksAddon((event, uri) => {
        event.preventDefault();
        openUrl(uri).catch(() => {});
      }),
    );
  }
  let webgl: WebglAddon | null = null;
  let host: HTMLElement | null = null;

  let colorSchemeReports = false;
  // Scheme the app last received; null when a replayed subscription has not been reported yet.
  let reportedDark: boolean | null = null;
  // Output parsed after the attach sync comes from apps running now, not from the replay.
  let attached = false;
  const reply = (text: string) => handlers.onReply(Array.from(encoder.encode(text)));
  const reportColorScheme = () => {
    const dark = isDark();
    if (reply(`\x1b[?997;${dark ? 1 : 2}n`)) reportedDark = dark;
  };
  const syncColorScheme = () => {
    if (attached && colorSchemeReports && isDark() !== reportedDark) reportColorScheme();
  };
  const resetColorScheme = () => {
    colorSchemeReports = false;
    reportedDark = null;
  };

  // xterm answers DECRQM incorrectly; report every mode but 2031 as "not recognized".
  const parser = (term as unknown as { parser?: CsiParser }).parser;
  if (parser?.registerCsiHandler) {
    for (const prefix of [undefined, "?"]) {
      parser.registerCsiHandler({ prefix, intermediates: "$", final: "p" }, (params) => {
        const mode = (params[0] as number) ?? 0;
        const state =
          prefix === "?" && mode === COLOR_SCHEME_MODE ? (colorSchemeReports ? 1 : 2) : 0;
        reply(`\x1b[${prefix ?? ""}${mode};${state}$y`);
        return true;
      });
    }
    // Returning false lets xterm still apply the other modes in the sequence.
    for (const [final, enabled] of [
      ["h", true],
      ["l", false],
    ] as const) {
      parser.registerCsiHandler({ prefix: "?", final }, (params) => {
        if (params.includes(COLOR_SCHEME_MODE)) {
          colorSchemeReports = enabled;
          // A starting app reads the scheme itself (OSC 11, 996); a replayed one waits for the sync.
          if (enabled && attached) reportedDark = isDark();
        }
        return false;
      });
    }
    parser.registerCsiHandler({ prefix: "?", final: "n" }, (params) => {
      if (params[0] !== COLOR_SCHEME_QUERY) return false;
      reportColorScheme();
      return true;
    });
    parser.registerEscHandler?.({ final: "c" }, () => {
      resetColorScheme();
      return false;
    });
  }

  term.attachCustomKeyEventHandler((ev) => {
    if (ev.type !== "keydown") return true;
    const action = matchTerminalKey(ev, term.hasSelection());
    if (!action) return true;
    switch (action.type) {
      case "copy": {
        ev.preventDefault();
        const selection = term.getSelection();
        if (selection) navigator.clipboard.writeText(selection).catch(() => {});
        return false;
      }
      case "paste":
        ev.preventDefault();
        navigator.clipboard
          .readText()
          .then((text) => {
            if (text) handlers.onUserBytes(Array.from(encoder.encode(text)));
          })
          .catch(() => {});
        return false;
      case "scroll_page_up":
        ev.preventDefault();
        term.scrollPages(-1);
        return false;
      case "scroll_page_down":
        ev.preventDefault();
        term.scrollPages(1);
        return false;
      case "scroll_line_up":
        ev.preventDefault();
        term.scrollLines(-1);
        return false;
      case "scroll_line_down":
        ev.preventDefault();
        term.scrollLines(1);
        return false;
      case "passthrough":
        return true;
      case "send_bytes":
        ev.preventDefault();
        handlers.onUserBytes(action.bytes);
        return false;
    }
  });
  term.onData(handlers.onData);
  term.onTitleChange(handlers.onTitle);

  // Native paste event avoids the clipboard permission prompt.
  const onPaste = (event: ClipboardEvent) => {
    const text = event.clipboardData?.getData("text");
    if (text) handlers.onUserBytes(Array.from(encoder.encode(text)));
  };

  function disposeWebgl(): void {
    webgl?.dispose();
    webgl = null;
  }

  return {
    get cols() {
      return term.cols;
    },
    get rows() {
      return term.rows;
    },

    ready() {
      const { font_family, font_size } = getTerminalSettings();
      return waitForFont(font_family, font_size);
    },

    open(target) {
      host = target;
      host.style.backgroundColor = extractTerminalTheme().background || "#000";
      host.addEventListener("paste", onPaste);
      term.open(target);
    },

    measure(): TerminalGeometry | null {
      const proposed = fitAddon.proposeDimensions();
      if (!proposed || !Number.isFinite(proposed.cols) || !Number.isFinite(proposed.rows))
        return null;
      return { cols: proposed.cols, rows: proposed.rows };
    },

    resize({ cols, rows }) {
      term.resize(cols, rows);
    },

    write(data, done) {
      term.write(data, done);
    },

    reset() {
      resetColorScheme();
      attached = false;
      term.reset();
    },

    // Browsers cap live WebGL contexts (~16); only visible views hold one.
    setGpu(enabled) {
      if (!enabled) {
        disposeWebgl();
        return;
      }
      if (webgl) return;
      try {
        const addon = new WebglAddon();
        addon.onContextLoss(() => {
          if (webgl === addon) disposeWebgl();
        });
        term.loadAddon(addon);
        webgl = addon;
      } catch {
        // WebGL unavailable: xterm keeps its DOM renderer.
      }
    },

    refreshAppearance() {
      const { font_size, font_family, option_as_meta } = getTerminalSettings();
      const theme = extractTerminalTheme();
      term.options.theme = theme;
      term.options.fontSize = font_size;
      term.options.fontFamily = fontStack(font_family);
      term.options.macOptionIsMeta = option_as_meta;
      if (host) host.style.backgroundColor = theme.background || "#000";
      webgl?.clearTextureAtlas();
      syncColorScheme();
    },

    connected() {
      // Queued behind the replay, so it sees the subscription state the replay ends with.
      term.write("", () => {
        attached = true;
        syncColorScheme();
      });
    },

    focus() {
      term.focus();
    },

    blur() {
      term.blur();
    },

    dispose() {
      host?.removeEventListener("paste", onPaste);
      disposeWebgl();
      term.dispose();
    },
  };
}
