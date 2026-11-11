// The WebAssembly engine must give the same answer as the native build.

import { beforeAll, expect, test } from "bun:test";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Engine, ROW_WIDTH } from "../src/engine";
import { availableHours, latestStart, runTrace } from "../src/trace";
import { type Fetcher, MissingWindError, Winds } from "../src/winds";

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
  winds = await Winds.open(join(data, "wind"), fromDisk);
  engine.reset(winds.times);
}, 120_000);

test("the history says what it covers", () => {
  expect(winds.times.length).toBe(17);
  expect(winds.manifest.days.length).toBe(3);
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
  expect(winds.files).toBeGreaterThan(4);
  expect(winds.files).toBeLessThan(60);

  const run = Bun.spawnSync([
    native,
    "trace",
    "--data",
    data,
    "--lat",
    "55.7",
    "--lon",
    "13.19",
    "--height",
    "500,1500",
    "--hours",
    "48",
    "--time",
    "2026010300",
    "--json",
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
      expect(Math.abs((result.points[at + 3] as number) - (expected[3] as number))).toBeLessThan(1);
    }
  }
  const lastLon = result.points[48 * starts.length * ROW_WIDTH] as number;
  expect(Math.abs(lastLon - 13.19)).toBeGreaterThan(5);
});

test("a second trace reuses the wind already downloaded", async () => {
  await Bun.sleep(50);
  const before = winds.files;
  await runTrace(engine, winds, { starts, startTime: END, hours: 48 });
  expect(winds.files).toBe(before);
});

/** A fresh engine and wind history, so a test cannot lean on what another one loaded. */
async function fresh(fetcher: Fetcher = fromDisk): Promise<{ engine: Engine; winds: Winds }> {
  const freshEngine = await Engine.load(await Bun.file(join(root, "web/public/engine.wasm")).arrayBuffer());
  const freshWinds = await Winds.open(join(data, "wind"), fetcher);
  freshEngine.reset(freshWinds.times);
  return { engine: freshEngine, winds: freshWinds };
}

const request = { starts, startTime: END, hours: 48 };

test("a trace that is already cancelled does not start", async () => {
  const controller = new AbortController();
  controller.abort();
  await expect(runTrace(engine, winds, request, { signal: controller.signal })).rejects.toThrow();
});

test("a trace interrupted while downloading leaves the next one able to finish", async () => {
  const baseline = await runTrace(engine, winds, request);

  // Interrupt at several different moments: during the first download,
  // during a later one, and between downloads.
  for (const stopAt of [1, 2, 5]) {
    const controller = new AbortController();
    let downloads = 0;
    const interrupting: Fetcher = async (url) => {
      const bytes = await fromDisk(url);
      if (url.endsWith(".uwb") && ++downloads === stopAt) controller.abort();
      return bytes;
    };
    const page = await fresh(interrupting);
    await expect(runTrace(page.engine, page.winds, request, { signal: controller.signal })).rejects.toThrow();

    // The same page traces again, as when a person picks another place.
    const again = await runTrace(page.engine, page.winds, request, { rowsPerChunk: 6, pause: () => Bun.sleep(0) });
    expect(again.rows).toBe(49);
    expect([...again.points]).toEqual([...baseline.points]);
  }
}, 60_000);

test("wind that does not hold what was asked for ends the trace instead of hanging it", async () => {
  // Every request is answered with the same file, whatever was asked for.
  let first: Uint8Array | undefined;
  const stuck: Fetcher = async (url) => {
    if (!url.endsWith(".uwb")) return fromDisk(url);
    first ??= await fromDisk(url);
    return first;
  };
  const page = await fresh(stuck);
  await expect(runTrace(page.engine, page.winds, request)).rejects.toThrow("could not be read");
});

test("a damaged file fails the trace and is fetched afresh on the next try", async () => {
  let damage = true;
  let fetched = 0;
  const flaky: Fetcher = async (url) => {
    const bytes = await fromDisk(url);
    if (!url.endsWith(".uwb")) return bytes;
    fetched += 1;
    return damage ? bytes.slice(0, bytes.length - 9) : bytes;
  };
  const page = await fresh(flaky);
  await expect(runTrace(page.engine, page.winds, request, { lookAhead: false })).rejects.toThrow("damaged");
  const afterFailure = fetched;
  damage = false;
  const again = await runTrace(page.engine, page.winds, request, { lookAhead: false });
  expect(again.rows).toBe(49);
  expect(fetched).toBeGreaterThan(afterFailure);
});

test("a file that has left the server is reported as a stale history", async () => {
  const gone: Fetcher = async (url) => {
    if (url.endsWith(".uwb")) throw new MissingWindError("gone");
    return fromDisk(url);
  };
  const page = await fresh(gone);
  await expect(runTrace(page.engine, page.winds, request)).rejects.toBeInstanceOf(MissingWindError);
});

test("a damaged file is refused", () => {
  expect(() => engine.add(new Uint8Array(64))).toThrow();
});

test("a trace reports only the wind it downloaded itself", async () => {
  const page = await fresh();
  const one = await runTrace(page.engine, page.winds, request);
  expect(one.files).toBeGreaterThan(0);
  expect(one.bytes).toBe(page.winds.bytes);
  await Bun.sleep(50);
  const settled = page.winds.files;
  const two = await runTrace(page.engine, page.winds, request);
  expect(two.files).toBe(page.winds.files - settled);
  expect(two.files).toBe(0);
});
