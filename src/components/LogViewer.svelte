<script lang="ts">
  import { onMount } from "svelte";
  import { sessionLogs } from "../lib/api";
  import type { SessionLogEntry } from "../lib/api";
  import { Terminal } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import "@xterm/xterm/css/xterm.css";

  interface Props {
    onClose: () => void;
  }

  let { onClose }: Props = $props();

  let logs = $state<SessionLogEntry[]>([]);
  let selectedLog = $state<SessionLogEntry | null>(null);
  let replayState = $state<"idle" | "playing" | "paused" | "done">("idle");
  let bytesReplayed = $state(0);
  let error = $state("");

  // Replay state
  let termEl: HTMLDivElement;
  let term: Terminal | null = null;
  let fitAddon: FitAddon | null = null;
  let replayOffset = 0;
  let replayTimer: ReturnType<typeof setTimeout> | null = null;
  const CHUNK_SIZE = 64 * 1024; // 64 KiB
  const CHUNK_INTERVAL_MS = 16;

  onMount(async () => {
    await refreshLogs();
  });

  async function refreshLogs() {
    try {
      logs = await sessionLogs.list();
    } catch (e) {
      error = String(e);
    }
  }

  async function deleteLog() {
    if (!selectedLog) return;
    if (!confirm(`Delete log for session ${selectedLog.session_id.slice(0, 8)}…?`)) return;
    try {
      await sessionLogs.delete(selectedLog.session_id);
      selectedLog = null;
      stopReplay();
      await refreshLogs();
    } catch (e) {
      error = String(e);
    }
  }

  function formatDate(iso: string | null): string {
    if (!iso) return "—";
    try {
      return new Date(iso).toLocaleString();
    } catch {
      return iso;
    }
  }

  function formatBytes(n: number): string {
    if (n < 1024) return `${n} B`;
    if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KiB`;
    return `${(n / (1024 * 1024)).toFixed(1)} MiB`;
  }

  function selectLog(log: SessionLogEntry) {
    stopReplay();
    selectedLog = log;
    replayState = "idle";
    bytesReplayed = 0;
  }

  function initTerminal() {
    if (term) {
      term.dispose();
    }
    term = new Terminal({
      rows: 24,
      cols: 80,
      scrollback: 10000,
      disableStdin: true,
      cursorBlink: false,
      convertEol: false,
      theme: { background: "#1e1e2e", foreground: "#cdd6f4" },
    });
    fitAddon = new FitAddon();
    term.loadAddon(fitAddon);
    if (termEl) {
      term.open(termEl);
      fitAddon.fit();
    }
  }

  async function startReplay() {
    if (!selectedLog) return;
    initTerminal();
    replayOffset = 0;
    bytesReplayed = 0;
    replayState = "playing";
    scheduleNextChunk();
  }

  function scheduleNextChunk() {
    if (replayState !== "playing") return;
    replayTimer = setTimeout(readNextChunk, CHUNK_INTERVAL_MS);
  }

  async function readNextChunk() {
    if (!selectedLog || replayState !== "playing") return;
    try {
      const chunk = await sessionLogs.readChunk(
        selectedLog.ansi_log_path,
        replayOffset,
        CHUNK_SIZE,
      );
      if (chunk.length === 0) {
        replayState = "done";
        return;
      }
      const bytes = new Uint8Array(chunk);
      term?.write(bytes);
      replayOffset += chunk.length;
      bytesReplayed = replayOffset;
      scheduleNextChunk();
    } catch (e) {
      error = String(e);
      replayState = "done";
    }
  }

  function pauseReplay() {
    if (replayTimer) clearTimeout(replayTimer);
    replayTimer = null;
    replayState = "paused";
  }

  function resumeReplay() {
    replayState = "playing";
    scheduleNextChunk();
  }

  function stopReplay() {
    if (replayTimer) clearTimeout(replayTimer);
    replayTimer = null;
    replayState = "idle";
    replayOffset = 0;
    bytesReplayed = 0;
    if (term) {
      term.dispose();
      term = null;
    }
  }

  async function copyPath() {
    if (selectedLog) {
      await navigator.clipboard.writeText(selectedLog.ansi_log_path);
    }
  }

  async function openFolder() {
    if (selectedLog) {
      try {
        await sessionLogs.openFolder(selectedLog.ansi_log_path);
      } catch (e) {
        error = String(e);
      }
    }
  }
</script>

<div class="log-viewer">
  <div class="log-viewer-header">
    <h2>Session Log Viewer <span class="badge">dogfood</span></h2>
    <div class="header-actions">
      <button class="close-btn" onclick={refreshLogs} title="Refresh">↻</button>
      <button class="close-btn" onclick={onClose}>✕</button>
    </div>
  </div>

  {#if error}
    <div class="error">{error}</div>
  {/if}

  <div class="log-viewer-body">
    <!-- Left: log list -->
    <div class="log-list">
      <h3>Saved Sessions ({logs.length})</h3>
      {#each logs as log}
        <button
          class="log-entry"
          class:selected={selectedLog?.session_id === log.session_id}
          onclick={() => selectLog(log)}
        >
          <div class="log-entry-id">{log.session_id.slice(0, 8)}…</div>
          <div class="log-entry-meta">
            <span class="status-badge" class:running={log.status === "running"} class:exited={log.status === "exited"}>{log.status}</span>
            <span>{formatBytes(log.bytes_written)}</span>
            <span>{formatDate(log.started_at)}</span>
          </div>
          <div class="log-entry-cmd">{log.command || "—"}</div>
        </button>
      {:else}
        <div class="empty">No session logs found. Run with PLANEAI_SESSION_LOG_DIR set.</div>
      {/each}
    </div>

    <!-- Right: detail + replay -->
    <div class="log-detail">
      {#if selectedLog}
        <div class="meta-panel">
          <table>
            <tbody>
            <tr><td>Session ID</td><td>{selectedLog.session_id}</td></tr>
            <tr><td>Status</td><td>{selectedLog.status}</td></tr>
            <tr><td>Started</td><td>{formatDate(selectedLog.started_at)}</td></tr>
            <tr><td>Ended</td><td>{formatDate(selectedLog.ended_at)}</td></tr>
            <tr><td>Command</td><td><code>{selectedLog.command}</code></td></tr>
            <tr><td>CWD</td><td><code>{selectedLog.cwd}</code></td></tr>
            <tr><td>Bytes Written</td><td>{formatBytes(selectedLog.bytes_written)}</td></tr>
            <tr><td>Bytes Dropped</td><td>{formatBytes(selectedLog.bytes_dropped)}</td></tr>
            </tbody>
          </table>
          <div class="meta-actions">
            <button onclick={copyPath}>Copy Path</button>
            <button onclick={openFolder}>Open Folder</button>
            <button class="danger" onclick={deleteLog}>Delete</button>
          </div>
        </div>

        <div class="replay-controls">
          {#if replayState === "idle"}
            <button class="primary" onclick={startReplay}>▶ Replay</button>
          {:else if replayState === "playing"}
            <button onclick={pauseReplay}>⏸ Pause</button>
            <button onclick={stopReplay}>⏹ Stop</button>
            <button onclick={startReplay}>↻ Restart</button>
            <span>{formatBytes(bytesReplayed)} / {formatBytes(selectedLog.bytes_written)}</span>
          {:else if replayState === "paused"}
            <button onclick={resumeReplay}>▶ Resume</button>
            <button onclick={stopReplay}>⏹ Stop</button>
            <button onclick={startReplay}>↻ Restart</button>
            <span>{formatBytes(bytesReplayed)} / {formatBytes(selectedLog.bytes_written)} (paused)</span>
          {:else}
            <button onclick={startReplay}>↻ Restart</button>
            <span>Replay complete — {formatBytes(bytesReplayed)}</span>
          {/if}
          <span class="read-only-label">READ-ONLY REPLAY</span>
        </div>

        <div class="replay-terminal" bind:this={termEl}></div>
      {:else}
        <div class="empty">Select a session log to view details and replay.</div>
      {/if}
    </div>
  </div>
</div>

<style>
  .log-viewer {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--color-surface, #1e1e2e);
    color: var(--color-text, #cdd6f4);
    font-family: var(--font-sans, system-ui);
    font-size: 13px;
  }
  .log-viewer-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 16px;
    border-bottom: 1px solid var(--color-border, #313244);
  }
  .log-viewer-header h2 {
    margin: 0;
    font-size: 16px;
    font-weight: 600;
  }
  .header-actions { display: flex; gap: 4px; }
  .badge {
    font-size: 10px;
    background: #f9e2af;
    color: #1e1e2e;
    padding: 2px 6px;
    border-radius: 4px;
    margin-left: 8px;
    text-transform: uppercase;
    font-weight: 700;
  }
  .close-btn {
    background: none;
    border: none;
    color: var(--color-text, #cdd6f4);
    font-size: 18px;
    cursor: pointer;
    padding: 4px 8px;
    border-radius: 4px;
  }
  .close-btn:hover { background: var(--color-border, #313244); }
  .error {
    padding: 8px 16px;
    background: #f38ba8;
    color: #1e1e2e;
  }
  .log-viewer-body {
    display: flex;
    flex: 1;
    overflow: hidden;
  }
  .log-list {
    width: 300px;
    min-width: 200px;
    border-right: 1px solid var(--color-border, #313244);
    overflow-y: auto;
    padding: 8px;
  }
  .log-list h3 {
    margin: 0 0 8px;
    font-size: 12px;
    text-transform: uppercase;
    opacity: 0.7;
  }
  .log-entry {
    display: block;
    width: 100%;
    text-align: left;
    background: none;
    border: 1px solid transparent;
    border-radius: 4px;
    padding: 8px;
    margin-bottom: 4px;
    cursor: pointer;
    color: inherit;
  }
  .log-entry:hover { border-color: var(--color-border, #313244); }
  .log-entry.selected { border-color: var(--color-accent, #89b4fa); background: rgba(137, 180, 250, 0.1); }
  .log-entry-id { font-family: monospace; font-weight: 600; }
  .log-entry-meta { font-size: 11px; opacity: 0.7; margin-top: 2px; display: flex; gap: 8px; align-items: center; }
  .log-entry-cmd { font-size: 11px; opacity: 0.6; margin-top: 2px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .status-badge { font-size: 10px; padding: 1px 5px; border-radius: 3px; background: #585b70; }
  .status-badge.running { background: #a6e3a1; color: #1e1e2e; }
  .status-badge.exited { background: #585b70; }
  .log-detail {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    padding: 12px;
    gap: 12px;
  }
  .meta-panel table {
    width: 100%;
    border-collapse: collapse;
    font-size: 12px;
  }
  .meta-panel td { padding: 3px 8px; }
  .meta-panel td:first-child { font-weight: 600; opacity: 0.7; width: 120px; }
  .meta-panel code { font-family: monospace; font-size: 11px; }
  .meta-actions { display: flex; gap: 8px; margin-top: 8px; }
  .meta-actions button, .replay-controls button {
    padding: 4px 12px;
    border: 1px solid var(--color-border, #313244);
    border-radius: 4px;
    background: var(--color-surface, #1e1e2e);
    color: var(--color-text, #cdd6f4);
    cursor: pointer;
    font-size: 12px;
  }
  .meta-actions button:hover, .replay-controls button:hover { background: var(--color-border, #313244); }
  .meta-actions button.danger { color: #f38ba8; border-color: #f38ba8; }
  .meta-actions button.danger:hover { background: rgba(243, 139, 168, 0.15); }
  .replay-controls button.primary { background: #89b4fa; color: #1e1e2e; border-color: #89b4fa; }
  .replay-controls {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 0;
  }
  .read-only-label {
    margin-left: auto;
    font-size: 10px;
    text-transform: uppercase;
    font-weight: 700;
    opacity: 0.5;
    letter-spacing: 0.5px;
  }
  .replay-terminal {
    flex: 1;
    min-height: 200px;
    border: 1px solid var(--color-border, #313244);
    border-radius: 4px;
    overflow: hidden;
  }
  .empty {
    padding: 32px;
    text-align: center;
    opacity: 0.5;
  }
</style>
