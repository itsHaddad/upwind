// The wind history on the server: what exists and how to download it.

import type { TileKey } from "./engine";

/** The manifest file the pipeline writes next to the tiles. */
export interface Manifest {
  version: number;
  /** Unix seconds when the history was last updated. */
  generated: number;
  step_hours: number;
  levels_hpa: number[];
  /** Valid time in unix seconds and the directory holding its tiles. */
  times: [number, string][];
}

export type Fetcher = (url: string) => Promise<Uint8Array>;

const browserFetch: Fetcher = async (url) => {
  const response = await fetch(url);
  if (!response.ok) throw new Error(`Could not load ${url} (${response.status}).`);
  return new Uint8Array(await response.arrayBuffer());
};

export class Winds {
  private readonly directories = new Map<number, string>();
  private readonly inFlight = new Map<string, Promise<Uint8Array>>();
  /** Bytes downloaded so far. */
  bytes = 0;
  /** Tiles downloaded so far. */
  tiles = 0;

  constructor(
    private readonly base: string,
    readonly manifest: Manifest,
    private readonly fetcher: Fetcher = browserFetch,
  ) {
    for (const [time, directory] of manifest.times) this.directories.set(time, directory);
  }

  static async open(base: string, fetcher: Fetcher = browserFetch): Promise<Winds> {
    const text = new TextDecoder().decode(await fetcher(`${base}/manifest.json`));
    const manifest = JSON.parse(text) as Manifest;
    if (manifest.version !== 1 || manifest.times.length < 2) {
      throw new Error("The wind history is missing or in a format this page does not know.");
    }
    return new Winds(base, manifest, fetcher);
  }

  /** Valid times in unix seconds, oldest first. */
  get times(): number[] {
    return this.manifest.times.map(([time]) => time);
  }

  get firstTime(): number {
    return (this.manifest.times[0] as [number, string])[0];
  }

  get lastTime(): number {
    return (this.manifest.times[this.manifest.times.length - 1] as [number, string])[0];
  }

  url([time, tile]: TileKey): string {
    const directory = this.directories.get(time);
    if (directory === undefined) throw new Error(`No wind is stored for time ${time}.`);
    return `${this.base}/${directory}/${tile}.uwt`;
  }

  /** Starts downloading the same tile for the `steps` valid times before `key`. */
  prefetch([time, tile]: TileKey, steps: number): void {
    const at = this.manifest.times.findIndex(([t]) => t === time);
    for (let i = at - 1; i >= 0 && i >= at - steps; i--) {
      const earlier = (this.manifest.times[i] as [number, string])[0];
      this.tile([earlier, tile]).catch(() => {});
    }
  }

  /** Downloads a tile; concurrent requests for the same tile share one download. */
  tile(key: TileKey): Promise<Uint8Array> {
    const url = this.url(key);
    let pending = this.inFlight.get(url);
    if (!pending) {
      pending = this.fetcher(url).then((bytes) => {
        this.bytes += bytes.length;
        this.tiles += 1;
        return bytes;
      });
      this.inFlight.set(url, pending);
      pending.catch(() => this.inFlight.delete(url));
    }
    return pending;
  }
}
