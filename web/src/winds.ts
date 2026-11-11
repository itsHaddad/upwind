// The wind history on the server: what exists and how to download it.

import type { TileKey } from "./engine";

/** The manifest file the pipeline writes next to the wind. */
export interface Manifest {
  version: number;
  /** Unix seconds when the history was last updated. */
  generated: number;
  step_hours: number;
  levels_hpa: number[];
  /** One entry per UTC day: its directory and its valid times in unix seconds. */
  days: { dir: string; times: number[] }[];
}

export type Fetcher = (url: string) => Promise<Uint8Array>;

const browserFetch: Fetcher = async (url) => {
  const response = await fetch(url);
  if (!response.ok) throw new Error(`Could not load ${url} (${response.status}).`);
  return new Uint8Array(await response.arrayBuffer());
};

export class Winds {
  /** Valid times in unix seconds, oldest first. */
  readonly times: number[];
  /** Index into the manifest's days of each valid time. */
  private readonly dayOf = new Map<number, number>();
  private readonly downloads = new Map<string, Promise<Uint8Array>>();
  private readonly delivered = new Set<string>();
  /** Bytes downloaded so far. */
  bytes = 0;
  /** Files downloaded so far. */
  files = 0;

  constructor(
    private readonly base: string,
    readonly manifest: Manifest,
    private readonly fetcher: Fetcher = browserFetch,
  ) {
    for (const [index, day] of manifest.days.entries()) {
      for (const time of day.times) this.dayOf.set(time, index);
    }
    this.times = manifest.days.flatMap((day) => day.times);
  }

  static async open(base: string, fetcher: Fetcher = browserFetch): Promise<Winds> {
    const text = new TextDecoder().decode(await fetcher(`${base}/manifest.json`));
    const manifest = JSON.parse(text) as Manifest;
    if (manifest.version !== 2 || !manifest.days?.length) {
      throw new Error("The wind history is missing or in a format this page does not know.");
    }
    return new Winds(base, manifest, fetcher);
  }

  get firstTime(): number {
    return this.times[0] as number;
  }

  get lastTime(): number {
    return this.times[this.times.length - 1] as number;
  }

  private fileUrl(day: number, tile: number, band: number): string {
    return `${this.base}/${(this.manifest.days[day] as { dir: string }).dir}/${band}/${tile}.uwb`;
  }

  private day([time]: TileKey): number {
    const day = this.dayOf.get(time);
    if (day === undefined) throw new Error(`No wind is stored for time ${time}.`);
    return day;
  }

  /** Downloads a file once; later requests share the same download. */
  private download(url: string): Promise<Uint8Array> {
    let pending = this.downloads.get(url);
    if (!pending) {
      pending = this.fetcher(url).then((bytes) => {
        this.bytes += bytes.length;
        this.files += 1;
        return bytes;
      });
      this.downloads.set(url, pending);
      pending.catch(() => this.downloads.delete(url));
    }
    return pending;
  }

  /**
   * The files that hold the given chunks and have not been handed out yet.
   * Each file is handed out once: it holds a whole day of one tile and
   * band, and the engine keeps what it is given.
   */
  async fetchNew(keys: readonly TileKey[]): Promise<Uint8Array[]> {
    const urls = new Set(keys.map((key) => this.fileUrl(this.day(key), key[1], key[2])));
    const fresh = [...urls].filter((url) => !this.delivered.has(url));
    const files = await Promise.all(fresh.map((url) => this.download(url)));
    for (const url of fresh) this.delivered.add(url);
    return files;
  }

  /** Starts downloading the files that hold the given chunks, ahead of need. */
  prefetch(keys: readonly TileKey[]): void {
    for (const key of keys) {
      this.download(this.fileUrl(this.day(key), key[1], key[2])).catch(() => {});
    }
  }
}
