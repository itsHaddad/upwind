// The WebAssembly engine must give the same answer as the native build.

import { beforeAll, expect, test } from "bun:test";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Engine, ROW_WIDTH } from "../src/engine";
import { availableHours, latestStart, runTrace } from "../src/trace";
import { type Fetcher, Winds } from "../src/winds";

const root = join(import.meta.dir, "../..");
const native = join(root, "target/release/upwind");
const data = mkdtempSync(join(tmpdir(), "upwind-synth-"));
const fromDisk: Fetcher = async (url) => new Uint8Array(await Bun.file(url).arrayBuffer());

/** 2026-01-03 00 UTC, the end of the made-up history. */
const END = 1_767_225_600 + 48 * 3600;
const starts = [
  { lon: 13.19, lat: 55.7, heightM: 500 },
  { lon: 13.19, lat: 55.7, heightM: 1500 },
];

let engine: Engine;
let winds: Winds;

beforeAll(async () => {
  const made = Bun.spawnSync([native, "synth", "--data", data]);
  if (made.exitCode !== 0) throw new Error(`upwind synth failed: ${made.stderr.toString()}`);
  engine = await Engine.load(await Bun.file(join(root, "web/public/engine.wasm")).arrayBuffer());
  winds = await Winds.open(data, fromDisk);
  engine.reset(winds.times);
}, 120_000);

test("the history says what it covers", () => {
  expect(winds.times.length).toBe(17);
  expect(latestStart(winds, END + 5000)).toBe(END);
  expect(latestStart(winds, END - 1800)).toBe(END - 3600);
  expect(availableHours(winds, END, 240)).toBe(48);
});

test("a trace in the browser engine matches the native one", async () => {
  const progress: number[] = [];
  const result = await runTrace(
    engine,
    winds,
    { starts, startTime: END, hours: 48 },
    { rowsPerChunk: 12, onProgress: (p) => progress.push(p.rows) },
  );
  expect(result.rows).toBe(49);
  expect(progress.at(-1)).toBe(49);
  expect(progress.length).toBeGreaterThan(3);
  expect(winds.tiles).toBeGreaterThan(4);
  expect(winds.tiles).toBeLessThan(17 * 72);

  const run = Bun.spawnSync([
    native, "trace", "--data", data, "--lat", "55.7", "--lon", "13.19",
    "--height", "500,1500", "--hours", "48", "--time", "2026010300", "--json",
  ]);
  expect(run.exitCode).toBe(0);
  const reference = JSON.parse(run.stdout.toString()) as { parcels: { path: number[][] }[] };
  for (const [parcel, { path }] of reference.parcels.entries()) {
    expect(path.length).toBe(49);
    for (const [row, expected] of path.entries()) {
      const at = (row * starts.length + parcel) * ROW_WIDTH;
      expect(result.points[at]).toBeCloseTo(expected[0] as number, 3);
      expect(result.points[at + 1]).toBeCloseTo(expected[1] as number, 3);
      expect(result.points[at + 2]).toBeCloseTo(expected[2] as number, 1);
    }
  }
  const lastLon = result.points[48 * starts.length * ROW_WIDTH] as number;
  expect(Math.abs(lastLon - 13.19)).toBeGreaterThan(5);
});

test("a second trace reuses the wind already downloaded", async () => {
  await Bun.sleep(50);
  const before = winds.tiles;
  await runTrace(engine, winds, { starts, startTime: END, hours: 48 });
  expect(winds.tiles).toBe(before);
});

test("a trace can be cancelled", async () => {
  const controller = new AbortController();
  controller.abort();
  const run = runTrace(engine, winds, { starts, startTime: END, hours: 48 }, { signal: controller.signal });
  expect(run).rejects.toThrow();
});

test("a damaged tile is refused", () => {
  expect(() => engine.addTile(new Uint8Array(64))).toThrow();
});
