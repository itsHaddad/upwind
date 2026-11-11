// The trajectory engine: a thin, typed layer over the WebAssembly module.

/** A chunk of wind: its valid time in unix seconds, its tile and its altitude band. */
export type TileKey = readonly [time: number, tile: number, band: number];

export interface Start {
  /** Degrees east. */
  lon: number;
  /** Degrees north. */
  lat: number;
  /** Metres above the ground. */
  heightM: number;
}

export type Advance = { status: "done" } | { status: "running" } | { status: "need"; tiles: TileKey[] };

/** Values per parcel per hourly row: longitude, latitude, pressure in hPa, metres above ground. */
export const ROW_WIDTH = 4;

interface Exports {
  memory: WebAssembly.Memory;
  uw_alloc(len: number): number;
  uw_free(pointer: number, len: number): void;
  uw_reset(times: number, count: number): void;
  uw_add(bytes: number, len: number): number;
  uw_start(starts: number, count: number, startTime: number, hours: number): void;
  uw_advance(maxRows: number): number;
  uw_needs(): number;
  uw_needs_len(): number;
  uw_look_ahead(): number;
  uw_hints(): number;
  uw_rows(): number;
  uw_points(): number;
  uw_points_len(): number;
}

export class Engine {
  private constructor(private readonly wasm: Exports) {}

  static async load(module: BufferSource): Promise<Engine> {
    const instance = await WebAssembly.instantiate(await WebAssembly.compile(module), {});
    return new Engine(instance.exports as unknown as Exports);
  }

  /** Copies bytes into the module, runs `use` on them, and frees them. */
  private withBytes<T>(bytes: Uint8Array, use: (pointer: number) => T): T {
    const pointer = this.wasm.uw_alloc(bytes.length);
    try {
      new Uint8Array(this.wasm.memory.buffer, pointer, bytes.length).set(bytes);
      return use(pointer);
    } finally {
      this.wasm.uw_free(pointer, bytes.length);
    }
  }

  private withFloats<T>(values: number[], use: (pointer: number) => T): T {
    const floats = new Float64Array(values);
    return this.withBytes(new Uint8Array(floats.buffer), use);
  }

  /** Empties the engine and sets the valid times (unix seconds) it covers. */
  reset(times: number[]): void {
    this.withFloats(times, (pointer) => this.wasm.uw_reset(pointer, times.length));
  }

  /** Adds a downloaded wind file. */
  add(bytes: Uint8Array): void {
    const result = this.withBytes(bytes, (pointer) => this.wasm.uw_add(pointer, bytes.length));
    if (result < 0) throw new Error("A wind file arrived damaged.");
    if (result === 0) throw new Error("A wind file is for a time outside the history.");
  }

  /** Starts a backward trace of `hours` hours from `startTime` (unix seconds). */
  start(starts: Start[], startTime: number, hours: number): void {
    const flat = starts.flatMap((s) => [s.lon, s.lat, s.heightM]);
    this.withFloats(flat, (pointer) => this.wasm.uw_start(pointer, starts.length, startTime, hours));
  }

  /** Runs the trace for up to `maxRows` more hourly rows. */
  advance(maxRows: number): Advance {
    const status = this.wasm.uw_advance(maxRows);
    if (status === 0) return { status: "done" };
    if (status === 1) return { status: "running" };
    if (status !== 2) throw new Error("The trace was not started.");
    return { status: "need", tiles: this.keys(this.wasm.uw_needs(), this.wasm.uw_needs_len()) };
  }

  /** Reads `(time, tile, band)` triples out of the module's memory. */
  private keys(pointer: number, length: number): TileKey[] {
    const flat = new Float64Array(this.wasm.memory.buffer, pointer, length);
    const keys: TileKey[] = [];
    for (let i = 0; i + 2 < flat.length; i += 3) {
      keys.push([flat[i] as number, flat[i + 1] as number, flat[i + 2] as number]);
    }
    return keys;
  }

  /** Wind the trace will probably need soon and has not been given. */
  lookAhead(): TileKey[] {
    const length = this.wasm.uw_look_ahead();
    return this.keys(this.wasm.uw_hints(), length);
  }

  /** Hourly rows recorded so far. */
  rows(): number {
    return this.wasm.uw_rows();
  }

  /** A copy of the recorded positions: rows × parcels × ROW_WIDTH values. */
  points(): Float32Array {
    const length = this.wasm.uw_points_len();
    if (length === 0) return new Float32Array(0);
    return new Float32Array(this.wasm.memory.buffer, this.wasm.uw_points(), length).slice();
  }
}
