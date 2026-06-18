/**
 * Benchmark metrics collector for PlaneAI terminal path (tauri-xterm).
 * Conforms to the shared integration contract in bench/INTEGRATION_CONTRACT.md.
 */

import { invoke } from "@tauri-apps/api/core";

export interface BenchConfig {
  backend: string;
  fixture: string;
  cols: number;
  rows: number;
  chunkSize: number;
  chunkIntervalMs: number;
  bytesTotal: number;
  metricsPath: string;
  snapshotPath?: string;
}

interface MetricEvent {
  schema_version: number;
  timestamp_ms: number;
  event_type: string;
  backend: string;
  fixture: string;
  cols: number;
  rows: number;
  chunk_size: number;
  chunk_interval_ms: number;
  bytes_total: number;
  bytes_since_last_event?: number | null;
  queue_depth_bytes?: number | null;
  pending_write_bytes?: number | null;
  frames_total?: number | null;
  write_latency_ms?: number | null;
  render_work_ms?: number | null;
  frame_delta_ms?: number | null;
  parse_time_ms?: number | null;
  js_heap_mb?: number | null;
  rss_mb?: number | null;
}

const SCHEMA_VERSION = 1;
const FLUSH_INTERVAL_MS = 500;
const FLUSH_BATCH_SIZE = 200;

export class MetricsCollector {
  private buffer: string[] = [];
  private config: BenchConfig;
  private flushTimer: ReturnType<typeof setInterval> | null = null;
  private frameCount = 0;
  private frameDeltas: number[] = [];
  private renderWorks: number[] = [];
  private writeLatencies: number[] = [];
  private replayStartMs = 0;
  private producerDoneMs = 0;
  private totalChunks = 0;

  // Queue tracking (split metrics)
  private maxPendingInputBytes = 0; // max bytes waiting before term.write
  private maxPendingUnparsedBytes = 0; // same as input for xterm (write = parse)
  private maxPendingUnrenderedBytes = 0; // bytes written but not yet in a RAF
  private maxOutstandingWrites = 0;

  // render_work_ms tracking
  private lastWriteCallbackTime = 0; // timestamp of most recent write callback
  private pendingBytesAtLastCallback = 0;

  constructor(config: BenchConfig) {
    this.config = config;
  }

  start() {
    this.replayStartMs = performance.now();
    this.flushTimer = setInterval(() => this.flush(), FLUSH_INTERVAL_MS);
    this.record("replay_start", {});
  }

  stop() {
    if (this.flushTimer) {
      clearInterval(this.flushTimer);
      this.flushTimer = null;
    }
  }

  markProducerDone() {
    this.producerDoneMs = performance.now();
  }

  record(eventType: string, extra: Partial<MetricEvent>) {
    const event: MetricEvent = {
      schema_version: SCHEMA_VERSION,
      timestamp_ms: performance.now(),
      event_type: eventType,
      backend: this.config.backend,
      fixture: this.config.fixture,
      cols: this.config.cols,
      rows: this.config.rows,
      chunk_size: this.config.chunkSize,
      chunk_interval_ms: this.config.chunkIntervalMs,
      bytes_total: this.config.bytesTotal,
      ...extra,
    };
    this.buffer.push(JSON.stringify(event));
    if (this.buffer.length >= FLUSH_BATCH_SIZE) {
      this.flush();
    }
  }

  recordFrontendChunkReceived(bytesSent: number, pendingBytes: number) {
    this.totalChunks++;
    if (pendingBytes > this.maxPendingInputBytes) this.maxPendingInputBytes = pendingBytes;
    if (pendingBytes > this.maxPendingUnparsedBytes) this.maxPendingUnparsedBytes = pendingBytes;
    this.record("frontend_chunk_received", {
      bytes_since_last_event: bytesSent,
      queue_depth_bytes: pendingBytes,
    });
  }

  recordWriteStart(pendingBytes: number, outstandingWrites: number) {
    if (outstandingWrites > this.maxOutstandingWrites)
      this.maxOutstandingWrites = outstandingWrites;
    this.record("xterm_write_start", {
      pending_write_bytes: pendingBytes,
      queue_depth_bytes: pendingBytes,
    });
  }

  recordWriteDone(latencyMs: number, pendingBytes: number) {
    this.writeLatencies.push(latencyMs);
    this.lastWriteCallbackTime = performance.now();
    this.pendingBytesAtLastCallback = pendingBytes;
    // After write callback, bytes are parsed but not yet rendered (until next RAF)
    // Track unrendered = bytes that completed write since last RAF
    this.record("xterm_write_done", {
      write_latency_ms: latencyMs,
      pending_write_bytes: pendingBytes,
    });
  }

  /**
   * Called each RAF. Measures:
   * - frame_delta_ms: time since last RAF
   * - render_work_ms: time since last write callback completed (approximates
   *   the render work xterm did between write-done and frame presentation)
   */
  recordFrame(deltaMs: number, renderWorkMs: number | null) {
    this.frameCount++;
    this.frameDeltas.push(deltaMs);
    if (renderWorkMs !== null) {
      this.renderWorks.push(renderWorkMs);
    }
    if (this.frameCount % 10 === 0) {
      this.record("raf_frame", {
        frame_delta_ms: deltaMs,
        render_work_ms: renderWorkMs,
        frames_total: this.frameCount,
      });
    }
  }

  recordPendingUnrendered(bytes: number) {
    if (bytes > this.maxPendingUnrenderedBytes) this.maxPendingUnrenderedBytes = bytes;
  }

  getLastWriteCallbackTime() {
    return this.lastWriteCallbackTime;
  }

  getHeapMb(): number | null {
    const perf = performance as unknown as { memory?: { usedJSHeapSize: number } };
    return perf.memory ? perf.memory.usedJSHeapSize / (1024 * 1024) : null;
  }

  async finalize(queueDepthAtEnd: number) {
    const wallTime = performance.now() - this.replayStartMs;
    const producerTime =
      this.producerDoneMs > 0 ? this.producerDoneMs - this.replayStartMs : wallTime;
    const drainTime = wallTime - producerTime;

    this.record("replay_done", {
      frames_total: this.frameCount,
      js_heap_mb: this.getHeapMb(),
    });

    const percentile = (arr: number[], p: number) => {
      if (arr.length === 0) return 0;
      const s = [...arr].sort((a, b) => a - b);
      const idx = Math.ceil((p / 100) * s.length) - 1;
      return s[Math.max(0, idx)];
    };

    const wl = this.writeLatencies;
    const fd = this.frameDeltas;
    const rw = this.renderWorks;
    const totalBytes = this.config.bytesTotal;
    const mbPerSec = wallTime > 0 ? totalBytes / (1024 * 1024) / (wallTime / 1000) : 0;
    const replayMode = this.config.chunkIntervalMs > 0 ? "realtime" : "maxspeed";
    const expectedMinReplayMs =
      this.config.chunkIntervalMs > 0
        ? Math.ceil(totalBytes / this.config.chunkSize) * this.config.chunkIntervalMs
        : 0;

    const summary = {
      schema_version: SCHEMA_VERSION,
      timestamp_ms: performance.now(),
      event_type: "summary",
      backend: this.config.backend,
      fixture: this.config.fixture,
      cols: this.config.cols,
      rows: this.config.rows,
      chunk_size: this.config.chunkSize,
      chunk_interval_ms: this.config.chunkIntervalMs,
      replay_mode: replayMode,
      total_bytes: totalBytes,
      total_chunks: this.totalChunks,
      actual_chunk_count: this.totalChunks,
      average_chunk_size: this.totalChunks > 0 ? Math.round(totalBytes / this.totalChunks) : 0,
      expected_min_replay_time_ms: expectedMinReplayMs,
      wall_time_ms: wallTime,
      producer_time_ms: producerTime,
      drain_time_ms: drainTime,
      total_replay_time_ms: wallTime,
      average_mb_per_sec: mbPerSec,

      p50_frame_delta_ms: percentile(fd, 50),
      p95_frame_delta_ms: percentile(fd, 95),
      p99_frame_delta_ms: percentile(fd, 99),

      p50_render_work_ms: rw.length > 0 ? percentile(rw, 50) : null,
      p95_render_work_ms: rw.length > 0 ? percentile(rw, 95) : null,
      p99_render_work_ms: rw.length > 0 ? percentile(rw, 99) : null,

      p50_parse_time_ms: null as number | null,
      p95_parse_time_ms: null as number | null,
      p99_parse_time_ms: null as number | null,

      p50_write_latency_ms: percentile(wl, 50),
      p95_write_latency_ms: percentile(wl, 95),
      p99_write_latency_ms: percentile(wl, 99),

      frames_over_16_7ms: fd.filter((t) => t > 16.7).length,
      frames_over_33_3ms: fd.filter((t) => t > 33.3).length,
      frames_over_50ms: fd.filter((t) => t > 50).length,

      fixture_bytes_loaded: totalBytes,
      max_pending_input_bytes: this.maxPendingInputBytes,
      max_pending_unparsed_bytes: this.maxPendingUnparsedBytes,
      max_pending_unrendered_bytes: this.maxPendingUnrenderedBytes,
      queue_depth_at_end_bytes: queueDepthAtEnd,
      max_queue_depth_bytes: this.maxPendingInputBytes,
      max_outstanding_writes: this.maxOutstandingWrites,

      startup_time_ms: null as number | null,
      final_rss_mb: null as number | null,
      final_js_heap_mb: this.getHeapMb(),

      snapshot_path: this.config.snapshotPath ?? null,
    };

    this.buffer.push(JSON.stringify(summary));
    await this.flush();
    this.stop();
  }

  private async flush() {
    if (this.buffer.length === 0) return;
    const batch = this.buffer.splice(0);
    try {
      await invoke("bench_write_metrics", {
        metricsPath: this.config.metricsPath,
        lines: batch,
      });
    } catch (e) {
      console.error("bench metrics flush failed:", e);
    }
  }
}
