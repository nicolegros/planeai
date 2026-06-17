<script lang="ts">
  import { onMount } from "svelte";
  import { Terminal } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import { WebglAddon } from "@xterm/addon-webgl";
  import "@xterm/xterm/css/xterm.css";
  import { runReplay, type ReplayOptions } from "../lib/benchmark/replay";

  interface Props {
    config: ReplayOptions;
  }

  let { config }: Props = $props();
  let containerEl: HTMLDivElement;
  let status = $state("Initializing...");

  onMount(() => {
    const term = new Terminal({
      cursorBlink: false,
      fontSize: 13,
      fontFamily: '"JetBrains Mono", "Menlo", monospace',
      scrollback: 20000,
      convertEol: true,
      allowProposedApi: true,
      cols: config.cols,
      rows: config.rows,
    });

    const fitAddon = new FitAddon();
    term.loadAddon(fitAddon);
    term.open(containerEl);

    try {
      const webgl = new WebglAddon();
      webgl.onContextLoss(() => webgl.dispose());
      term.loadAddon(webgl);
    } catch {
      // WebGL not available
    }

    status = `Replaying ${config.fixturePath} (${config.cols}x${config.rows}, ${config.chunkSize}B chunks, ${config.chunkIntervalMs}ms interval)...`;

    runReplay(term, config)
      .then(() => {
        status = "Replay complete. Metrics written.";
      })
      .catch((e) => {
        status = `Error: ${e}`;
      });

    return () => term.dispose();
  });
</script>

<div class="bench-container">
  <div class="bench-status">{status}</div>
  <div bind:this={containerEl} class="bench-terminal"></div>
</div>

<style>
  .bench-container {
    width: 100vw;
    height: 100vh;
    display: flex;
    flex-direction: column;
    background: #1e1e1e;
  }
  .bench-status {
    padding: 8px 16px;
    font-family: monospace;
    font-size: 12px;
    color: #888;
    background: #111;
  }
  .bench-terminal {
    flex: 1;
    overflow: hidden;
  }
</style>
