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
  import { getThemeById } from "../lib/terminal-themes";

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
  let attaching = false;
  let dataChannel: Channel<ArrayBuffer> | null = $state(null);

  const SCROLLBACK_LINES = 100_000;
  const RESIZE_DEBOUNCE_MS = 50;
  const IS_MAC = typeof navigator !== "undefined" && /Mac/.test(navigator.platform);

  function terminalFontStack(primary: string): string {
    const quoted = `"${primary.replace(/\\/g, "\\\\").replace(/"/g, '\\"')}"`;
    return `${quoted}, "Symbols Nerd Font Mono", monospace`;
  }

  const termBg = $derived(
    getThemeById(
      isDark()
        ? getSettings().appearance.terminal_theme_dark
        : getSettings().appearance.terminal_theme_light
    ).colors.background
  );

  onMount(() => {
    const s = getSettings();
    const themeId = isDark() ? s.appearance.terminal_theme_dark : s.appearance.terminal_theme_light;
    const theme = getThemeById(themeId);

    term = new Terminal({
      cursorBlink: true,
      fontSize: s.terminal.font_size,
      fontFamily: terminalFontStack(s.terminal.font_family),
      theme: theme.colors,
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

      // Cmd+C → copy selection (if any)
      if (IS_MAC && ev.metaKey && !ev.ctrlKey && !ev.shiftKey && ev.key === "c") {
        if (term.hasSelection()) {
          ev.preventDefault();
          navigator.clipboard.writeText(term.getSelection()).catch(() => {});
          return false;
        }
        // No selection: let it pass through as Ctrl+C interrupt
        return true;
      }

      // Cmd+V → let it fall through to native paste event
      if (IS_MAC && ev.metaKey && !ev.ctrlKey && !ev.shiftKey && ev.key === "v") {
        return true;
      }

      // Shift+Enter → Ctrl+J (newline without submit)
      if (ev.shiftKey && !ev.ctrlKey && !ev.metaKey && ev.key === "Enter") {
        ev.preventDefault();
        const bytes = [0x0a]; // Ctrl+J
        invoke("write_to_pty", { sessionId, data: bytes });
        return false;
      }

      // Cmd+Backspace → Ctrl+U (kill line)
      if (IS_MAC && ev.metaKey && !ev.ctrlKey && !ev.shiftKey && ev.key === "Backspace") {
        ev.preventDefault();
        const bytes = [0x15]; // Ctrl+U
        invoke("write_to_pty", { sessionId, data: bytes });
        return false;
      }

      // Cmd+Left → Ctrl+A (beginning of line)
      if (IS_MAC && ev.metaKey && !ev.ctrlKey && !ev.shiftKey && ev.key === "ArrowLeft") {
        ev.preventDefault();
        const bytes = [0x01]; // Ctrl+A
        invoke("write_to_pty", { sessionId, data: bytes });
        return false;
      }

      // Cmd+Right → Ctrl+E (end of line)
      if (IS_MAC && ev.metaKey && !ev.ctrlKey && !ev.shiftKey && ev.key === "ArrowRight") {
        ev.preventDefault();
        const bytes = [0x05]; // Ctrl+E
        invoke("write_to_pty", { sessionId, data: bytes });
        return false;
      }

      // Escape → send Ctrl+C (interrupt)
      if (ev.key === "Escape" && !ev.ctrlKey && !ev.metaKey && !ev.altKey) {
        ev.preventDefault();
        const bytes = [0x03]; // Ctrl+C
        invoke("write_to_pty", { sessionId, data: bytes });
        return false;
      }

      return true;
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
    dataChannel = onData;
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
      invoke("spawn_tab", { sessionId: baseSessionId, tabIndex, onData }).then(() => {
        attached = true;
        onAttached?.();
        const { rows, cols } = term;
        invoke("resize_pty", { sessionId, rows, cols });
      }).catch((e) => {
        showSnackbar(String(e));
      });
    } else if (!exited) {
      // Attach handled by $effect below
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
    if (!exited && !attached && !attaching && dataChannel) {
      attaching = true;
      invoke("attach_session", { sessionId, onData: dataChannel }).then(() => {
        attached = true;
        onAttached?.();
        const { rows, cols } = term;
        invoke("resize_pty", { sessionId, rows, cols });
      }).catch((e) => {
        showSnackbar(String(e));
      }).finally(() => {
        attaching = false;
      });
    }
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
    const themeId = isDark() ? s.appearance.terminal_theme_dark : s.appearance.terminal_theme_light;
    const theme = getThemeById(themeId);
    term.options.theme = theme.colors;
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
