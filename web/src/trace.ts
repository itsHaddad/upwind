// Runs a trace to the end, downloading wind as the engine asks for it.

import { type Engine, ROW_WIDTH, type Start } from "./engine";
import type { Winds } from "./winds";

export interface TraceRequest {
  starts: Start[];
  /** Unix seconds to trace back from. */
  startTime: number;
  hours: number;
}

export interface TraceResult {
  request: TraceRequest;
  /** Hourly rows recorded; row r is r hours before the start. */
  rows: number;
  /** rows × parcels × ROW_WIDTH values; NaN where the history ran out. */
  points: Float32Array;
  /** Milliseconds spent computing, without the time spent downloading. */
  computeMs: number;
  /** Times the trace had to stop for wind that was not downloaded yet, and for how long. */
  waits: number;
  waitMs: number;
}

export interface TraceProgress {
  rows: number;
  points: Float32Array;
}

export interface TraceOptions {
  /** Rows to compute between progress reports. */
  rowsPerChunk?: number;
  onProgress?: (progress: TraceProgress) => void;
  /**
   * Download wind the paths are heading for before it is needed. Faster,
   * at the cost of some downloads that turn out not to be needed. On by
   * default.
   */
  lookAhead?: boolean;
  /** Called between chunks; lets the page draw a frame. */
  pause?: () => Promise<void>;
  signal?: AbortSignal;
}

/** The newest whole hour the history can be traced back from. */
export function latestStart(winds: Winds, nowSeconds: number): number {
  return Math.floor(Math.min(nowSeconds, winds.lastTime) / 3600) * 3600;
}

/** Hours of history available before `startTime`, at most `wanted`. */
export function availableHours(winds: Winds, startTime: number, wanted: number): number {
  return Math.max(0, Math.min(wanted, Math.floor((startTime - winds.firstTime) / 3600)));
}

export async function runTrace(
  engine: Engine,
  winds: Winds,
  request: TraceRequest,
  options: TraceOptions = {},
): Promise<TraceResult> {
  const chunk = options.rowsPerChunk ?? 240;
  engine.start(request.starts, request.startTime, request.hours);
  let computeMs = 0;
  let waits = 0;
  let waitMs = 0;
  let reported = 0;
  for (;;) {
    options.signal?.throwIfAborted();
    const began = performance.now();
    const step = engine.advance(chunk);
    computeMs += performance.now() - began;
    if (step.status === "done") break;
    // Start on the wind the paths are heading for, so that the trace rarely
    // has to stop and wait for a download.
    if (options.lookAhead !== false) winds.prefetch(engine.lookAhead());
    const rows = engine.rows();
    if (rows > reported) {
      reported = rows;
      options.onProgress?.({ rows, points: engine.points() });
      await options.pause?.();
    }
    if (step.status === "need") {
      const asked = performance.now();
      const files = await winds.fetchNew(step.tiles);
      const waited = performance.now() - asked;
      if (waited > 4) {
        waits += 1;
        waitMs += waited;
      }
      options.signal?.throwIfAborted();
      for (const file of files) engine.add(file);
    }
  }
  const points = engine.points();
  const rows = points.length / (request.starts.length * ROW_WIDTH);
  options.onProgress?.({ rows, points });
  return { request, rows, points, computeMs, waits, waitMs };
}
