<script lang="ts">
  import { onMount } from "svelte";
  import { Channel } from "@tauri-apps/api/core";
  import { pty } from "../lib/api";
  import { Terminal } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import { WebLinksAddon } from "@xterm/addon-web-links";
  import { WebglAddon } from "@xterm/addon-webgl";
  import { Unicode11Addon } from "@xterm/addon-unicode11";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import "@xterm/xterm/css/xterm.css";
  import { showSnackbar } from "../lib/snackbar.svelte";
  import { getSettings, getTerminalSettings, isDark } from "../lib/settings.svelte";
  import { extractTerminalTheme } from "../lib/theme-loader";
  import { matchTerminalKey } from "../lib/terminal-keys";

  interface Props {
    sessionId: string;
    visible: boolean;
    focused: boolean;
    exited?: boolean;
    skipAttach?: boolean;
    paused?: boolean;
    onUserInput?: () => void;
    onAttached?: () => void;
  }

  let { sessionId, visible, focused, exited = false, skipAttach = false, paused = false, onUserInput, onAttached }: Props = $props();

  let containerEl: HTMLDivElement;
  let term: Terminal;
  let fitAddon: FitAddon;
  let webglAddon: WebglAddon | null = null;
  let attached = $state(false);

  const SCROLLBACK_LINES = 20_000;
  const RESIZE_DEBOUNCE_MS = 50;

  // ── Input write — send immediately for responsive typing ──────────────
  function queueWrite(bytes: number[]) {
    pty.write(sessionId, bytes);
  }

  function terminalFontStack(primary: string): string {
    const quoted = `"${primary.replace(/\\/g, "\\\\").replace(/"/g, '\\"')}"`;
    return `${quoted}, "Symbols Nerd Font Mono", monospace`;
  }

  let termBg = $state("#000");

  onMount(() => {
    const s = getSettings();
    const themeColors = extractTerminalTheme();
    termBg = themeColors.background || "#000";

    term = new Terminal({
      cursorBlink: true,
      fontSize: s.terminal.font_size,
      fontFamily: terminalFontStack(s.terminal.font_family),
      theme: themeColors,
      scrollback: s.scrollback_lines ?? SCROLLBACK_LINES,
      convertEol: true,
      scrollOnUserInput: false,
      allowProposedApi: true,
      macOptionIsMeta: s.terminal.option_as_meta,
      linkHandler: {
        activate: (_event, uri) => {
          openUrl(uri).catch(() => {});
        },
      },
    });

    fitAddon = new FitAddon();
    term.loadAddon(fitAddon);

    // Unicode 11 width rules for proper glyph sizing
    term.loadAddon(new Unicode11Addon());
    term.unicode.activeVersion = "11";

    // WebLinksAddon — clickable URLs (optional, can be disabled for perf)
    if (s.web_links !== false) {
      term.loadAddon(
        new WebLinksAddon((event, uri) => {
          event.preventDefault();
          openUrl(uri).catch(() => {});
        })
      );
    }

    term.open(containerEl);

    fitAddon.fit();

    // ── Paste via native event (avoids clipboard permission prompt) ──────
    containerEl.addEventListener("paste", (e: ClipboardEvent) => {
      const text = e.clipboardData?.getData("text");
      if (text) {
        const bytes = Array.from(new TextEncoder().encode(text));
        queueWrite(bytes);
      }
    });

    // ── DECRQM workaround (xterm bug: host queries mode status) ──────────
    try {
      const parser = (
        term as unknown as {
          parser?: {
            registerCsiHandler?: (
              id: { prefix?: string; intermediates?: string; final: string },
              cb: (params: (number | number[])[]) => boolean
            ) => { dispose(): void };
          };
        }
      ).parser;
      if (parser?.registerCsiHandler) {
        parser.registerCsiHandler(
          { intermediates: "$", final: "p" },
          (params) => {
            const mode = (params[0] as number) ?? 0;
            const bytes = Array.from(
              new TextEncoder().encode(`\x1b[${mode};0$y`)
            );
            queueWrite(bytes);
            return true;
          }
        );
        parser.registerCsiHandler(
          { prefix: "?", intermediates: "$", final: "p" },
          (params) => {
            const mode = (params[0] as number) ?? 0;
            const bytes = Array.from(
              new TextEncoder().encode(`\x1b[?${mode};0$y`)
            );
            queueWrite(bytes);
            return true;
          }
        );
      }
    } catch {
      // Ignore if parser API unavailable
    }

    // ── Focus-report filtering ───────────────────────────────────────────
    let ptyStarted = false;

    // ── Keyboard shortcuts ───────────────────────────────────────────────
    term.attachCustomKeyEventHandler((ev) => {
      if (ev.type !== "keydown") return true;

      const action = matchTerminalKey(ev, term.hasSelection());
      if (!action) return true;

      switch (action.type) {
        case "copy": {
          ev.preventDefault();
          const sel = term.getSelection();
          if (sel) navigator.clipboard.writeText(sel).catch(() => {});
          return false;
        }
        case "paste":
          ev.preventDefault();
          navigator.clipboard.readText().then((text) => {
            if (text) queueWrite([...new TextEncoder().encode(text)]);
          }).catch(() => {});
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
          queueWrite(action.bytes);
          return false;
      }
    });

    // ── Terminal input with focus-report filtering ────────────────────────
    term.onData((data) => {
      if (exited) return;
      let filtered = data;
      if (!ptyStarted) {
        filtered = data.replace(/\x1b\[I|\x1b\[O/g, "");
      }
      if (!filtered) return;
      const bytes = Array.from(new TextEncoder().encode(filtered));
      queueWrite(bytes);
      onUserInput?.();
    });

    // ── Listen for PTY output with flow control ─────────────────────────
    const FLOW_HIGH = 32_000;
    const FLOW_LOW = 8_000;
    let pendingBytes = 0;
    let isPaused = false;

    const onData = new Channel<ArrayBuffer>();
    onData.onmessage = (raw: ArrayBuffer) => {
      ptyStarted = true;
      const data = new Uint8Array(raw);
      pendingBytes += data.byteLength;

      if (pendingBytes > FLOW_HIGH && !isPaused) {
        isPaused = true;
        pty.pause(sessionId);
      }

      term.write(data, () => {
        pendingBytes = Math.max(pendingBytes - data.byteLength, 0);
        if (isPaused && pendingBytes < FLOW_LOW) {
          isPaused = false;
          pty.resume(sessionId);
        }
      });
    };

    // ── Attach to the session ─────────────────────────────────────────────
    if (skipAttach) {
      // Non-primary tab: we call spawn_tab ourselves with our data channel
      const parts = sessionId.split(":");
      const baseSessionId = parts[0];
      const tabIndex = parseInt(parts[1] || "0", 10);
      pty.spawnTab(baseSessionId, tabIndex, isDark(), onData).then(() => {
        attached = true;
        onAttached?.();
        const { rows, cols } = term;
        pty.resize(sessionId, rows, cols);
      }).catch((e) => {
        showSnackbar(String(e));
      });
    } else if (!exited) {
      // Attach immediately in onMount to avoid $effect double-fire
      pty.attach(sessionId, isDark(), onData).then(() => {
        attached = true;
        onAttached?.();
        const { rows, cols } = term;
        pty.resize(sessionId, rows, cols);
      }).catch((e) => {
        showSnackbar(String(e));
      });
    }

    // ── Resize observer with debouncing ──────────────────────────────────
    let resizeTimer: ReturnType<typeof setTimeout> | null = null;
    let lastSentDims: { cols: number; rows: number } | null = null;

    const resizeObserver = new ResizeObserver(() => {
      if (!visible || !attached) return;
      if (resizeTimer) clearTimeout(resizeTimer);
      resizeTimer = setTimeout(() => {
        resizeTimer = null;
        fitAddon.fit();
        const { rows, cols } = term;
        if (lastSentDims?.cols === cols && lastSentDims?.rows === rows) return;
        lastSentDims = { cols, rows };
        pty.resize(sessionId, rows, cols);
      }, RESIZE_DEBOUNCE_MS);
    });
    resizeObserver.observe(containerEl);

    return () => {
      if (resizeTimer) clearTimeout(resizeTimer);
      resizeObserver.disconnect();
      term.dispose();
    };
  });

  $effect(() => {
    if (!term) return;
    if (visible) {
      if (!webglAddon) {
        try {
          const addon = new WebglAddon();
          addon.onContextLoss(() => {
            addon.dispose();
            webglAddon = null;
          });
          term.loadAddon(addon);
          webglAddon = addon;
        } catch {
          // WebGL not available, use default canvas renderer
        }
      }
      requestAnimationFrame(() => {
        const proposed = fitAddon.proposeDimensions();
        if (proposed && (proposed.cols !== term.cols || proposed.rows !== term.rows)) {
          fitAddon.fit();
        }
      });
    }
    // WebGL addon stays alive — disposed only on unmount via term.dispose()
  });

  $effect(() => {
    if (!term) return;
    if (focused) {
      term.focus();
    } else {
      term.blur();
    }
  });

  // Only re-run when terminal-relevant settings change
  $effect(() => {
    if (!term) return;
    const { font_size, font_family, option_as_meta } = getTerminalSettings();
    isDark(); // track reactivity on dark mode change
    const theme = extractTerminalTheme();
    term.options.theme = theme;
    termBg = theme.background || "#000";
    term.options.fontSize = font_size;
    term.options.fontFamily = terminalFontStack(font_family);
    term.options.macOptionIsMeta = option_as_meta;
    if (fitAddon) fitAddon.fit();
  });

  // Pause/resume PTY data flow for MRU neighbors
  $effect(() => {
    if (!attached) return;
    if (paused) {
      pty.pause(sessionId);
    } else {
      pty.resume(sessionId);
    }
  });

  // Re-attach when session is restarted (exited → active)
  let prevExited = $state(exited);
  $effect(() => {
    if (prevExited && !exited && term && !skipAttach) {
      const onData = new Channel<ArrayBuffer>();
      onData.onmessage = (raw: ArrayBuffer) => {
        const data = new Uint8Array(raw);
        term.write(data);
      };
      pty.attach(sessionId, isDark(), onData).then(() => {
        attached = true;
        const { rows, cols } = term;
        pty.resize(sessionId, rows, cols);
      }).catch((e) => {
        showSnackbar(String(e));
      });
    }
    prevExited = exited;
  });


</script>

<div
  bind:this={containerEl}
  class="w-full h-full"
  class:hidden={!visible}
  style="background-color: {termBg}"
></div>
