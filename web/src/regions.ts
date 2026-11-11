// Names the country or sea under a point, from a quarter-degree grid.

export interface Region {
  name: string;
  /** "country", or a kind of water: ocean, sea, bay, gulf, strait, channel, sound. */
  kind: string;
}

interface RegionIndex {
  width: number;
  height: number;
  names: string[];
  kinds: string[];
}

/** How far to look for a named cell around an unnamed one, in cells. */
const SEARCH_CELLS = 12;

const COUNTRIES_WITH_ARTICLE = new Set([
  "Bahamas",
  "Central African Republic",
  "Czech Republic",
  "Dominican Republic",
  "Gambia",
  "Maldives",
  "Netherlands",
  "Philippines",
  "United Arab Emirates",
  "United Kingdom",
  "United States of America",
]);

/** Shorter everyday forms of some long official names. */
const SHORT_NAMES: Record<string, string> = {
  "United States of America": "United States",
  "Dem. Rep. Congo": "DR Congo",
  "Central African Rep.": "Central African Republic",
  "Dominican Rep.": "Dominican Republic",
  "Bosnia and Herz.": "Bosnia and Herzegovina",
  "Eq. Guinea": "Equatorial Guinea",
  "S. Sudan": "South Sudan",
  "W. Sahara": "Western Sahara",
  "Fr. S. Antarctic Lands": "the French Southern Lands",
  "Falkland Is.": "the Falkland Islands",
  "Faeroe Is.": "the Faroe Islands",
  "Solomon Is.": "the Solomon Islands",
  Czechia: "Czechia",
};

/** The name as it reads after "over": "the North Sea", "Sweden", "Hudson Bay". */
export function inSentence(region: Region): string {
  const name = SHORT_NAMES[region.name] ?? region.name;
  if (name.startsWith("the ")) return name;
  if (region.kind === "country") {
    return COUNTRIES_WITH_ARTICLE.has(region.name) || COUNTRIES_WITH_ARTICLE.has(name) ? `the ${name}` : name;
  }
  // "Hudson Bay" and "Baffin Bay" take no article; "Bay of Biscay" does.
  const bare = /^\p{Lu}\p{L}+ (Bay|Sound|Inlet)$/u.test(name) || name.startsWith("The ");
  return bare ? name : `the ${name}`;
}

export class Regions {
  constructor(
    private readonly index: RegionIndex,
    private readonly grid: Uint16Array,
  ) {
    if (grid.length !== index.width * index.height) throw new Error("The region grid has the wrong size.");
  }

  /** Loads the grid: `regions.json` and the gzip-packed `regions.bin` under `base`. */
  static async load(base: string): Promise<Regions> {
    const [index, packed] = await Promise.all([fetch(`${base}/regions.json`), fetch(`${base}/regions.bin`)]);
    if (!index.ok || !packed.ok || !packed.body) throw new Error("Could not load the map of region names.");
    const unpacked = packed.body.pipeThrough(new DecompressionStream("gzip"));
    const bytes = await new Response(unpacked).arrayBuffer();
    return new Regions((await index.json()) as RegionIndex, new Uint16Array(bytes));
  }

  private cell(row: number, column: number): number {
    const { width, height } = this.index;
    if (row < 0 || row >= height) return 0;
    return this.grid[row * width + (((column % width) + width) % width)] as number;
  }

  /** The region at a point, or the nearest named one within a few degrees. */
  at(lon: number, lat: number): Region | undefined {
    const { width, height, names, kinds } = this.index;
    const perDegree = width / 360;
    const row = Math.min(height - 1, Math.max(0, Math.floor((90 - lat) * perDegree)));
    const column = Math.floor(((((lon + 180) % 360) + 360) % 360) * perDegree);
    let id = this.cell(row, column);
    for (let reach = 1; id === 0 && reach <= SEARCH_CELLS; reach++) {
      for (let d = -reach; d <= reach && id === 0; d++) {
        id =
          this.cell(row - reach, column + d) ||
          this.cell(row + reach, column + d) ||
          this.cell(row + d, column - reach) ||
          this.cell(row + d, column + reach);
      }
    }
    if (id === 0) return lat < -60 ? { name: "Antarctica", kind: "country" } : undefined;
    return { name: names[id] as string, kind: kinds[id] as string };
  }
}
