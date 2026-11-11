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

/**
 * A wind file the manifest lists is gone from the server. The history is
 * refreshed every few hours and old days are removed, so this means the
 * manifest in hand is out of date: open the history again.
 */
export class MissingWindError extends Error {}

/**
 * Downloads a file. `fresh` asks for the server's current copy even if the
 * browser has one it believes is still good.
 */
export type Fetcher = (url: string, fresh?: boolean) => Promise<Uint8Array>;

const browserFetch: Fetcher = async (url, fresh = false) => {
  const response = await fetch(url, fresh ? { cache: "no-cache" } : undefined);
  if (response.status === 404) throw new MissingWindError("The wind history was updated while this page was open.");
  if (!response.ok)
    throw new Error(`Could not load the wind (${response.status}). Check your connection and try again.`);
  return new Uint8Array(await response.arrayBuffer());
};

/** Most downloads held at once while waiting to be needed (about 35 kB each). */
const MAX_HELD_FILES = 300;

/** A downloaded wind file: one tile and altitude band for one day. */
export interface WindFile {
  url: string;
  bytes: Uint8Array;
}

export class Winds {
  /** Valid times in unix seconds, oldest first. */
  readonly times: number[];
  /** Index into the manifest's days of each valid time. */
  private readonly dayOf = new Map<number, number>();
  /** Downloads running or finished and not yet given to the engine, oldest first. */
  private readonly downloads = new Map<string, Promise<Uint8Array>>();
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
    // Always the server's current manifest: a remembered one would point at
    // days that have since been removed.
    const text = new TextDecoder().decode(await fetcher(`${base}/manifest.json`, true));
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

  private fileUrl(key: TileKey): string {
    const day = this.dayOf.get(key[0]);
    if (day === undefined) throw new Error(`No wind is stored for time ${key[0]}.`);
    return `${this.base}/${(this.manifest.days[day] as { dir: string }).dir}/${key[2]}/${key[1]}.uwb`;
  }

  /** Downloads a file once; requests made while it is held share it. */
  private download(url: string): Promise<Uint8Array> {
    let pending = this.downloads.get(url);
    if (!pending) {
      pending = this.fetcher(url).then((bytes) => {
        this.bytes += bytes.length;
        this.files += 1;
        return bytes;
      });
      this.downloads.set(url, pending);
      pending.catch(() => this.forget(url));
      // Files fetched ahead of need that no trace asked for would pile up
      // over a long visit. Let the oldest go; if one is wanted after all it
      // is simply downloaded again.
      for (const oldest of this.downloads.keys()) {
        if (this.downloads.size <= MAX_HELD_FILES) break;
        this.downloads.delete(oldest);
      }
    }
    return pending;
  }

  /** The files that hold the given chunks: one per tile, band and day. */
  async fetch(keys: readonly TileKey[]): Promise<WindFile[]> {
    const urls = [...new Set(keys.map((key) => this.fileUrl(key)))];
    return Promise.all(urls.map(async (url) => ({ url, bytes: await this.download(url) })));
  }

  /** Starts downloading the files that hold the given chunks, ahead of need. */
  prefetch(keys: readonly TileKey[]): void {
    for (const key of keys) this.download(this.fileUrl(key)).catch(() => {});
  }

  /**
   * Lets go of a downloaded file. Call it once the engine has the file (the
   * engine keeps what it is given and will not ask again), or when the file
   * turned out to be unusable, so that a retry downloads it afresh.
   */
  forget(url: string): void {
    this.downloads.delete(url);
  }
}
