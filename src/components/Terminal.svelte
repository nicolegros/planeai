<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { Terminal } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import { WebLinksAddon } from "@xterm/addon-web-links";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import "@xterm/xterm/css/xterm.css";
  import { getSettings, isDark } from "../lib/settings.svelte";
  import { getThemeById } from "../lib/terminal-themes";

  interface Props {
    sessionId: string;
    visible: boolean;
    focused: boolean;
    exited?: boolean;
    onUserInput?: () => void;
  }

  let { sessionId, visible, focused, exited = false, onUserInput }: Props = $props();

  let containerEl: HTMLDivElement;
  let term: Terminal;
  let fitAddon: FitAddon;
  let attached = false;

  const SCROLLBACK_LINES = 100_000;
  const RESIZE_DEBOUNCE_MS = 120;
  const IS_MAC = typeof navigator !== "undefined" && /Mac/.test(navigator.platform);

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
      fontFamily: `'${s.terminal.font_family}', monospace`,
      theme: theme.colors,
      scrollback: SCROLLBACK_LINES,
      convertEol: true,
      scrollOnUserInput: false,
      allowProposedApi: true,
      macOptionIsMeta: s.terminal.option_as_meta,
    });

    fitAddon = new FitAddon();
    term.loadAddon(fitAddon);

    // WebLinksAddon — clickable URLs
    term.loadAddon(
      new WebLinksAddon((_event, uri) => {
        openUrl(uri).catch(() => {});
      })
    );

    term.open(containerEl);

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

      // Cmd+K → clear terminal and force redraw
      if (IS_MAC && ev.metaKey && !ev.ctrlKey && !ev.shiftKey && ev.key === "k") {
        ev.preventDefault();
        term.clear();
        // Send clear screen + tmux refresh
        const clear = Array.from(new TextEncoder().encode("\x0c"));
        invoke("write_to_pty", { sessionId, data: clear });
        return false;
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

    // ── Listen for PTY output ────────────────────────────────────────────
    const unlisten = listen<string>(`pty-output-${sessionId}`, (event) => {
      ptyStarted = true;
      const bytes = base64Decode(event.payload);
      term.write(bytes);
    });

    // ── Attach to the session ─────────────────────────────────────────────
    invoke("attach_session", { sessionId }).then(() => {
      attached = true;
      const { rows, cols } = term;
      invoke("resize_pty", { sessionId, rows, cols });
    });

    // ── Resize observer with debouncing ──────────────────────────────────
    let resizeTimer: ReturnType<typeof setTimeout> | null = null;
    let lastSentDims: { cols: number; rows: number } | null = null;

    const resizeObserver = new ResizeObserver(() => {
      if (!visible) return;
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
    const themeId = isDark() ? s.appearance.terminal_theme_dark : s.appearance.terminal_theme_light;
    const theme = getThemeById(themeId);
    term.options.theme = theme.colors;
    term.options.fontSize = s.terminal.font_size;
    term.options.fontFamily = `'${s.terminal.font_family}', monospace`;
    term.options.macOptionIsMeta = s.terminal.option_as_meta;
    if (fitAddon) fitAddon.fit();
  });

  function base64Decode(str: string): Uint8Array {
    const binary = atob(str);
    const bytes = new Uint8Array(binary.length);
    for (let i = 0; i < binary.length; i++) {
      bytes[i] = binary.charCodeAt(i);
    }
    return bytes;
  }
</script>

<div
  bind:this={containerEl}
  class="w-full h-full"
  class:hidden={!visible}
  style="background-color: {termBg}"
></div>
