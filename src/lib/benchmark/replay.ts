/**
 * Benchmark replay: streams fixture bytes through the real xterm.js write path.
 *
 * This uses the same term.write() call that the normal PTY output path uses,
 * including the callback-based backpressure mechanism.
 */

import { invoke, Channel } from "@tauri-apps/api/core";
import type { Terminal } from "@xterm/xterm";
import { MetricsCollector, type BenchConfig } from "./metrics";

export interface ReplayOptions {
  fixturePath: string;
  metricsPath: string;
  snapshotPath?: string;
  cols: number;
  rows: number;
  chunkSize: number;
  chunkIntervalMs: number;
  exitWhenDone: boolean;
}

export async function runReplay(term: Terminal, opts: ReplayOptions): Promise<void> {
  const info = await invoke<{ bytes_total: number; filename: string }>("bench_fixture_info", {
    fixturePath: opts.fixturePath,
  });

  const config: BenchConfig = {
    backend: "tauri-xterm",
    fixture: info.filename,
    cols: opts.cols,
    rows: opts.rows,
    chunkSize: opts.chunkSize,
    chunkIntervalMs: opts.chunkIntervalMs,
    bytesTotal: info.bytes_total,
    metricsPath: opts.metricsPath,
    snapshotPath: opts.snapshotPath,
  };

  const metrics = new MetricsCollector(config);
  term.resize(opts.cols, opts.rows);

  let pendingBytes = 0;
  let outstandingWrites = 0;
  let bytesWrittenSinceLastRaf = 0; // bytes whose write callback fired since last RAF
  let replayDone = false;

  // RAF loop: measures frame_delta_ms and render_work_ms
  let lastFrameTime = performance.now();
  let rafId: number;

  function frameLoop() {
    const now = performance.now();
    const delta = now - lastFrameTime;
    lastFrameTime = now;

    // render_work_ms: time from the last write callback to this RAF
    // This approximates how much rendering work xterm.js did between
    // the last write completing and the frame being presented
    const lastCb = metrics.getLastWriteCallbackTime();
    let renderWork: number | null = null;
    if (lastCb > 0 && now - lastCb < delta) {
      renderWork = now - lastCb;
    }

    // Track unrendered bytes (writes completed but not yet in a frame)
    metrics.recordPendingUnrendered(bytesWrittenSinceLastRaf);
    bytesWrittenSinceLastRaf = 0;

    metrics.recordFrame(delta, renderWork);

    if (!replayDone) {
      rafId = requestAnimationFrame(frameLoop);
    }
  }

  // Data channel — mirrors the real PTY output path in Terminal.svelte
  const onData = new Channel<ArrayBuffer>();
  onData.onmessage = (raw: ArrayBuffer) => {
    const data = new Uint8Array(raw);
    pendingBytes += data.byteLength;
    outstandingWrites++;

    metrics.recordFrontendChunkReceived(data.byteLength, pendingBytes);
    metrics.recordWriteStart(pendingBytes, outstandingWrites);

    const writeStart = performance.now();

    term.write(data, () => {
      const latency = performance.now() - writeStart;
      pendingBytes = Math.max(pendingBytes - data.byteLength, 0);
      outstandingWrites--;
      bytesWrittenSinceLastRaf += data.byteLength;
      metrics.recordWriteDone(latency, pendingBytes);
    });
  };

  // Start
  metrics.start();
  rafId = requestAnimationFrame(frameLoop);

  await invoke("bench_replay_file", {
    fixturePath: opts.fixturePath,
    chunkSize: opts.chunkSize,
    chunkIntervalMs: opts.chunkIntervalMs,
    onData,
  });

  metrics.markProducerDone();

  // Wait for all xterm writes to drain
  await new Promise<void>((resolve) => {
    const check = () => {
      if (outstandingWrites <= 0) resolve();
      else setTimeout(check, 10);
    };
    setTimeout(check, 50);
  });

  replayDone = true;
  cancelAnimationFrame(rafId);

  await metrics.finalize(pendingBytes);

  // Correctness snapshot
  if (opts.snapshotPath) {
    const buffer = term.buffer.active;
    const lines: string[] = [];
    for (let i = 0; i < buffer.length; i++) {
      const line = buffer.getLine(i);
      if (line) lines.push(line.translateToString(true));
    }
    await invoke("bench_write_snapshot", {
      snapshotPath: opts.snapshotPath,
      content: lines.join("\n"),
    });
  }

  if (opts.exitWhenDone) {
    setTimeout(() => {
      // @ts-expect-error Tauri process exit
      window.__TAURI_INTERNALS__?.invoke("plugin:process|exit", { code: 0 });
    }, 500);
  }
}
