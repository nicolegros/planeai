<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { Terminal } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import { WebglAddon } from "@xterm/addon-webgl";
  import "@xterm/xterm/css/xterm.css";
  import { getSettings, isDark } from "../lib/settings.svelte";
  import { getThemeById } from "../lib/terminal-themes";

  interface Props {
    sessionId: string;
    tmuxName: string;
    visible: boolean;
    focused: boolean;
  }

  let { sessionId, tmuxName, visible, focused }: Props = $props();

  let containerEl: HTMLDivElement;
  let term: Terminal;
  let fitAddon: FitAddon;
  let attached = false;

  onMount(() => {
    const s = getSettings();
    const themeId = isDark() ? s.terminal_theme_dark : s.terminal_theme_light;
    const theme = getThemeById(themeId);

    term = new Terminal({
      cursorBlink: true,
      fontSize: s.font_size,
      fontFamily: s.font_family,
      theme: theme.colors,
    });

    fitAddon = new FitAddon();
    term.loadAddon(fitAddon);
    term.open(containerEl);

    try {
      term.loadAddon(new WebglAddon());
    } catch {
      // WebGL not available, fall back to canvas
    }

    fitAddon.fit();

    // Send input to backend
    term.attachCustomKeyEventHandler((ev) => {
      if (ev.key === "Enter" && ev.shiftKey) {
        if (ev.type === "keydown") {
          const bytes = Array.from(new TextEncoder().encode("\n"));
          invoke("write_to_pty", { sessionId, data: bytes });
        }
        return false;
      }
      return true;
    });

    term.onData((data) => {
      const bytes = Array.from(new TextEncoder().encode(data));
      invoke("write_to_pty", { sessionId, data: bytes });
    });

    // Listen for PTY output
    const unlisten = listen<string>(`pty-output-${sessionId}`, (event) => {
      const bytes = base64Decode(event.payload);
      term.write(bytes);
    });

    // Attach to the tmux session
    invoke("attach_session", { sessionId, tmuxName }).then(() => {
      attached = true;
      // Send initial size
      const { rows, cols } = term;
      invoke("resize_pty", { sessionId, rows, cols });
    });

    // Resize observer
    const resizeObserver = new ResizeObserver(() => {
      if (visible) {
        fitAddon.fit();
        const { rows, cols } = term;
        invoke("resize_pty", { sessionId, rows, cols });
      }
    });
    resizeObserver.observe(containerEl);

    return () => {
      resizeObserver.disconnect();
      unlisten.then((fn) => fn());
      term.dispose();
    };
  });

  $effect(() => {
    if (visible && fitAddon) {
      // Re-fit when becoming visible
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
    const themeId = isDark() ? s.terminal_theme_dark : s.terminal_theme_light;
    term.options.theme = getThemeById(themeId).colors;
    term.options.fontSize = s.font_size;
    term.options.fontFamily = s.font_family;
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
></div>
