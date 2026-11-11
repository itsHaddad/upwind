// Builds the lookup the page uses to name where the air was: a quarter-degree
// grid of region numbers and the list of region names.
//
// Countries come from world-atlas, seas and oceans from Natural Earth's marine
// areas. Both are public domain. Run with: bun scripts/build-regions.ts

import { mkdirSync } from "node:fs";
import { join } from "node:path";
import type { Feature, FeatureCollection, MultiPolygon, Polygon, Position } from "geojson";
import { feature } from "topojson-client";
import type { GeometryCollection, Topology } from "topojson-specification";

const root = join(import.meta.dir, "..");
const cache = join(root, "node_modules/.cache/upwind");
const MARINE = "ne_50m_geography_marine_polys";
const MARINE_URL = `https://raw.githubusercontent.com/nvkelso/natural-earth-vector/v5.1.2/geojson/${MARINE}.geojson`;

/** Grid cells per degree. */
const PER_DEGREE = 4;
const WIDTH = 360 * PER_DEGREE;
const HEIGHT = 180 * PER_DEGREE;

/** Kinds of marine area that are worth naming, most general first. */
const MARINE_KINDS = ["ocean", "sea", "bay", "gulf", "strait", "channel", "sound"];

type Area = Feature<Polygon | MultiPolygon, Record<string, unknown>>;

async function marineAreas(): Promise<Area[]> {
  mkdirSync(cache, { recursive: true });
  const path = join(cache, `${MARINE}.geojson`);
  let text: string;
  if (await Bun.file(path).exists()) {
    text = await Bun.file(path).text();
  } else {
    const response = await fetch(MARINE_URL);
    if (!response.ok) throw new Error(`Could not download ${MARINE_URL}`);
    text = await response.text();
    await Bun.write(path, text);
  }
  const collection = JSON.parse(text) as FeatureCollection<Polygon | MultiPolygon>;
  return collection.features as Area[];
}

async function countries(): Promise<Area[]> {
  const path = join(root, "node_modules/world-atlas/countries-50m.json");
  const topology = (await Bun.file(path).json()) as Topology<{ countries: GeometryCollection }>;
  const collection = feature(topology, topology.objects.countries) as FeatureCollection<Polygon | MultiPolygon>;
  return collection.features as Area[];
}

/**
 * Writes `id` into every cell whose centre is inside the polygon.
 *
 * A polygon that crosses the antimeridian has neighbouring vertices near
 * +180° and -180°. Read literally, that edge spans the whole map and the
 * fill floods a band around the world. Such polygons are shifted into
 * 0°..360° first, where they are in one piece.
 */
function fill(grid: Uint16Array, input: Position[][], id: number): void {
  // An edge along the top or bottom of the map (a polygon around a pole) also
  // jumps from one side to the other, but it is level and crosses nothing.
  const crosses = input.some((ring) =>
    ring.some((point, k) => {
      const previous = ring[k - 1];
      if (!previous) return false;
      return Math.abs((point[0] as number) - (previous[0] as number)) > 180 && point[1] !== previous[1];
    }),
  );
  const rings = crosses
    ? input.map((ring) =>
        ring.map((point) => [(point[0] as number) < 0 ? (point[0] as number) + 360 : point[0], point[1]] as Position),
      )
    : input;
  let south = 90;
  let north = -90;
  for (const ring of rings) {
    for (const point of ring) {
      const lat = point[1] as number;
      if (lat < south) south = lat;
      if (lat > north) north = lat;
    }
  }
  const firstRow = Math.max(0, Math.floor((90 - north) * PER_DEGREE));
  const lastRow = Math.min(HEIGHT - 1, Math.ceil((90 - south) * PER_DEGREE));
  for (let row = firstRow; row <= lastRow; row++) {
    const lat = 90 - (row + 0.5) / PER_DEGREE;
    const crossings: number[] = [];
    for (const ring of rings) {
      for (let k = 0; k + 1 < ring.length; k++) {
        const [x1, y1] = ring[k] as [number, number];
        const [x2, y2] = ring[k + 1] as [number, number];
        if (y1 <= lat !== y2 <= lat) crossings.push(x1 + ((lat - y1) / (y2 - y1)) * (x2 - x1));
      }
    }
    crossings.sort((a, b) => a - b);
    for (let k = 0; k + 1 < crossings.length; k += 2) {
      const from = Math.ceil(((crossings[k] as number) + 180) * PER_DEGREE - 0.5);
      const to = Math.floor(((crossings[k + 1] as number) + 180) * PER_DEGREE - 0.5);
      for (let column = from; column <= to; column++) {
        grid[row * WIDTH + (((column % WIDTH) + WIDTH) % WIDTH)] = id;
      }
    }
  }
}

function fillArea(grid: Uint16Array, area: Area, id: number): void {
  const polygons = area.geometry.type === "Polygon" ? [area.geometry.coordinates] : area.geometry.coordinates;
  for (const rings of polygons) fill(grid, rings, id);
}

/** "SOUTHERN OCEAN" → "Southern Ocean"; other names are kept as they are. */
function tidy(name: string): string {
  if (name !== name.toUpperCase()) return name;
  return name.toLowerCase().replace(/(^|[\s-])\p{L}/gu, (letter) => letter.toUpperCase());
}

const grid = new Uint16Array(WIDTH * HEIGHT);
const names: string[] = [""];
const kinds: string[] = [""];
const idOf = new Map<string, number>();
const register = (name: string, kind: string): number => {
  const key = `${kind}:${name}`;
  let id = idOf.get(key);
  if (id === undefined) {
    id = names.length;
    idOf.set(key, id);
    names.push(name);
    kinds.push(kind);
  }
  return id;
};

// Oceans first, then smaller waters over them, then land over everything.
const marine = (await marineAreas())
  .filter((area) => MARINE_KINDS.includes(String(area.properties.featurecla)))
  .sort((a, b) => Number(a.properties.scalerank) - Number(b.properties.scalerank));
for (const area of marine) {
  const name = tidy(String(area.properties.name_en ?? area.properties.name));
  fillArea(grid, area, register(name, String(area.properties.featurecla)));
}
for (const area of await countries()) {
  fillArea(grid, area, register(String(area.properties.name), "country"));
}

const unnamed = grid.reduce((count, id) => count + (id === 0 ? 1 : 0), 0);
const out = join(root, "web/public");
mkdirSync(out, { recursive: true });
const packed = Bun.gzipSync(new Uint8Array(grid.buffer), { level: 9 });
await Bun.write(join(out, "regions.bin"), packed);
await Bun.write(join(out, "regions.json"), JSON.stringify({ width: WIDTH, height: HEIGHT, names, kinds }));
console.log(
  `${names.length - 1} regions, ${((100 * unnamed) / grid.length).toFixed(1)}% of cells unnamed, ` +
    `${(packed.length / 1000).toFixed(0)} kB`,
);
