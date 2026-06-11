<script lang="ts">
  import { onMount } from "svelte";
  import { invoke, Channel } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { Terminal } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import { WebLinksAddon } from "@xterm/addon-web-links";
  import { WebglAddon } from "@xterm/addon-webgl";
  import { Unicode11Addon } from "@xterm/addon-unicode11";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import "@xterm/xterm/css/xterm.css";
  import { showSnackbar } from "../lib/snackbar.svelte";
  import { getSettings, isDark } from "../lib/settings.svelte";
  import { extractTerminalTheme } from "../lib/theme-loader";
  import { matchTerminalKey } from "../lib/terminal-keys";

  interface Props {
    sessionId: string;
    visible: boolean;
    focused: boolean;
    exited?: boolean;
    skipAttach?: boolean;
    onUserInput?: () => void;
    onAttached?: () => void;
  }

  let { sessionId, visible, focused, exited = false, skipAttach = false, onUserInput, onAttached }: Props = $props();

  let containerEl: HTMLDivElement;
  let term: Terminal;
  let fitAddon: FitAddon;
  let attached = $state(false);

  const SCROLLBACK_LINES = 100_000;
  const RESIZE_DEBOUNCE_MS = 50;

  function terminalFontStack(primary: string): string {
    const quoted = `"${primary.replace(/\\/g, "\\\\").replace(/"/g, '\\"')}"`;
    return `${quoted}, "Symbols Nerd Font Mono", monospace`;
  }

  const termBg = $derived(
    extractTerminalTheme().background || "#000"
  );

  onMount(() => {
    const s = getSettings();
    const themeColors = extractTerminalTheme();

    term = new Terminal({
      cursorBlink: true,
      fontSize: s.terminal.font_size,
      fontFamily: terminalFontStack(s.terminal.font_family),
      theme: themeColors,
      scrollback: SCROLLBACK_LINES,
      convertEol: true,
      scrollOnUserInput: false,
      allowProposedApi: true,
      macOptionIsMeta: s.terminal.option_as_meta,
    });

    fitAddon = new FitAddon();
    term.loadAddon(fitAddon);

    // Unicode 11 width rules for proper glyph sizing
    term.loadAddon(new Unicode11Addon());
    term.unicode.activeVersion = "11";

    // WebLinksAddon — clickable URLs
    term.loadAddon(
      new WebLinksAddon((_event, uri) => {
        openUrl(uri).catch(() => {});
      })
    );

    term.open(containerEl);

    try {
      term.loadAddon(new WebglAddon());
    } catch {
      // WebGL not available, fall back to canvas
    }

    fitAddon.fit();

    // ── Paste via native event (avoids clipboard permission prompt) ──────
    containerEl.addEventListener("paste", (e: ClipboardEvent) => {
      const text = e.clipboardData?.getData("text");
      if (text) {
        const bytes = Array.from(new TextEncoder().encode(text));
        invoke("write_to_pty", { sessionId, data: bytes });
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
            invoke("write_to_pty", { sessionId, data: bytes });
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
            invoke("write_to_pty", { sessionId, data: bytes });
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
        case "copy":
          ev.preventDefault();
          navigator.clipboard.writeText(term.getSelection()).catch(() => {});
          return false;
        case "paste":
          ev.preventDefault();
          navigator.clipboard.readText().then((text) => {
            if (text) invoke("write_to_pty", { sessionId, data: [...new TextEncoder().encode(text)] });
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
          invoke("write_to_pty", { sessionId, data: action.bytes });
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
      invoke("write_to_pty", { sessionId, data: bytes });
      onUserInput?.();
    });

    // ── Listen for PTY output with flow control ─────────────────────────
    const FLOW_HIGH = 100_000;
    const FLOW_LOW = 10_000;
    let pendingBytes = 0;
    let isPaused = false;

    const onData = new Channel<ArrayBuffer>();
    onData.onmessage = (raw: ArrayBuffer) => {
      ptyStarted = true;
      const data = new Uint8Array(raw);
      pendingBytes += data.byteLength;

      if (pendingBytes > FLOW_HIGH && !isPaused) {
        isPaused = true;
        invoke("pause_pty", { sessionId });
      }

      term.write(data, () => {
        pendingBytes = Math.max(pendingBytes - data.byteLength, 0);
        if (isPaused && pendingBytes < FLOW_LOW) {
          isPaused = false;
          invoke("resume_pty", { sessionId });
        }
      });
    };

    // Listen for exit event
    const unlisten = listen<void>(`pty-exited-${sessionId}`, () => {});

    // ── Attach to the session ─────────────────────────────────────────────
    if (skipAttach) {
      // Non-primary tab: we call spawn_tab ourselves with our data channel
      const parts = sessionId.split(":");
      const baseSessionId = parts[0];
      const tabIndex = parseInt(parts[1] || "0", 10);
      invoke("spawn_tab", { sessionId: baseSessionId, tabIndex, darkMode: isDark(), onData }).then(() => {
        attached = true;
        onAttached?.();
        const { rows, cols } = term;
        invoke("resize_pty", { sessionId, rows, cols });
      }).catch((e) => {
        showSnackbar(String(e));
      });
    } else if (!exited) {
      // Attach immediately in onMount to avoid $effect double-fire
      invoke("attach_session", { sessionId, darkMode: isDark(), onData }).then(() => {
        attached = true;
        onAttached?.();
        const { rows, cols } = term;
        invoke("resize_pty", { sessionId, rows, cols });
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
        invoke("resize_pty", { sessionId, rows, cols });
      }, RESIZE_DEBOUNCE_MS);
    });
    resizeObserver.observe(containerEl);

    return () => {
      if (resizeTimer) clearTimeout(resizeTimer);
      resizeObserver.disconnect();
      unlisten.then((fn) => fn());
      term.dispose();
    };
  });

  $effect(() => {
    if (visible && fitAddon) {
      requestAnimationFrame(() => fitAddon.fit());
    }
  });

  $effect(() => {
    if (focused && term) {
      term.focus();
    }
  });

  $effect(() => {
    if (!term) return;
    const s = getSettings();
    isDark(); // track reactivity on dark mode change
    term.options.theme = extractTerminalTheme();
    term.options.fontSize = s.terminal.font_size;
    term.options.fontFamily = terminalFontStack(s.terminal.font_family);
    term.options.macOptionIsMeta = s.terminal.option_as_meta;
    if (fitAddon) fitAddon.fit();
  });


</script>

<div
  bind:this={containerEl}
  class="w-full h-full"
  class:hidden={!visible}
  style="background-color: {termBg}"
></div>
