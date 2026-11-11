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
}

export interface TraceProgress {
  rows: number;
  points: Float32Array;
}

export interface TraceOptions {
  /** Rows to compute between progress reports. */
  rowsPerChunk?: number;
  onProgress?: (progress: TraceProgress) => void;
  /** Called between chunks; lets the page draw a frame. */
  pause?: () => Promise<void>;
  signal?: AbortSignal;
}

/** Valid times fetched ahead of need for a tile the air is in. */
const LOOKAHEAD_STEPS = 4;

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
  let reported = 0;
  for (;;) {
    options.signal?.throwIfAborted();
    const began = performance.now();
    const step = engine.advance(chunk);
    computeMs += performance.now() - began;
    if (step.status === "done") break;
    const rows = engine.rows();
    if (rows > reported) {
      reported = rows;
      options.onProgress?.({ rows, points: engine.points() });
      await options.pause?.();
    }
    if (step.status === "need") {
      // The air will very likely stay in these tiles for the next hours
      // too, so start on the earlier times now instead of one wait each.
      for (const key of step.tiles) winds.prefetch(key, LOOKAHEAD_STEPS);
      const files = await Promise.all(step.tiles.map((key) => winds.tile(key)));
      options.signal?.throwIfAborted();
      for (const file of files) engine.addTile(file);
    }
  }
  const points = engine.points();
  const rows = points.length / (request.starts.length * ROW_WIDTH);
  options.onProgress?.({ rows, points });
  return { request, rows, points, computeMs };
}
