// A finished or still-growing trace, in the shapes the page needs.

import { ROW_WIDTH, type Start } from "./engine";
import type { PathSet } from "./globe/paths";
import type { Place } from "./places";
import type { PathPoint } from "./story";

/** Height above ground the main parcel arrives at, in metres. */
export const ARRIVAL_HEIGHT_M = 500;
/** Longest history to follow, in hours. */
export const MAX_HOURS = 240;

/**
 * The parcels traced for a place. The first is the one the story follows;
 * the others arrive at other heights and a little to each side, and show how
 * much the answer depends on exactly where and how high the air is.
 */
export function ensemble(place: Place): Start[] {
  const beside = 0.3;
  const stretch = 1 / Math.max(0.2, Math.cos((place.lat * Math.PI) / 180));
  const at = (dLon: number, dLat: number, heightM: number): Start => ({
    lon: place.lon + dLon * stretch,
    lat: Math.max(-89.5, Math.min(89.5, place.lat + dLat)),
    heightM,
  });
  return [
    at(0, 0, ARRIVAL_HEIGHT_M),
    at(0, 0, 100),
    at(0, 0, 1000),
    at(0, 0, 1500),
    at(beside, 0, ARRIVAL_HEIGHT_M),
    at(-beside, 0, ARRIVAL_HEIGHT_M),
    at(0, beside, ARRIVAL_HEIGHT_M),
    at(0, -beside, ARRIVAL_HEIGHT_M),
  ];
}

/** The engine's rows as longitude, latitude and height for drawing. */
export function toPathSet(points: Float32Array, parcels: number): PathSet {
  const count = points.length / ROW_WIDTH;
  const drawn = new Float32Array(count * 3);
  for (let i = 0; i < count; i++) {
    drawn[i * 3] = points[i * ROW_WIDTH] as number;
    drawn[i * 3 + 1] = points[i * ROW_WIDTH + 1] as number;
    drawn[i * 3 + 2] = points[i * ROW_WIDTH + 3] as number;
  }
  return { parcels, rows: count / parcels, points: drawn };
}

/** The main parcel's hourly positions, up to where its history ends. */
export function mainPath(points: Float32Array, parcels: number): PathPoint[] {
  const path: PathPoint[] = [];
  const stride = parcels * ROW_WIDTH;
  for (let at = 0; at < points.length; at += stride) {
    const lon = points[at] as number;
    if (Number.isNaN(lon)) break;
    path.push({
      lon,
      lat: points[at + 1] as number,
      hpa: points[at + 2] as number,
      heightM: points[at + 3] as number,
    });
  }
  return path;
}

/** The position `hour` hours back, between the hourly points. */
export function pointAt(path: PathPoint[], hour: number): PathPoint | undefined {
  const last = path.length - 1;
  if (last < 0) return undefined;
  const h = Math.max(0, Math.min(last, hour));
  const a = path[Math.floor(h)] as PathPoint;
  const b = path[Math.min(last, Math.floor(h) + 1)] as PathPoint;
  const t = h - Math.floor(h);
  const dLon = ((b.lon - a.lon + 540) % 360) - 180;
  return {
    lon: a.lon + dLon * t,
    lat: a.lat + (b.lat - a.lat) * t,
    hpa: a.hpa + (b.hpa - a.hpa) * t,
    heightM: a.heightM + (b.heightM - a.heightM) * t,
  };
}
