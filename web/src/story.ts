// Turns a traced path into the things worth saying about it.

import { inSentence, type Region } from "./regions";

const EARTH_RADIUS_KM = 6371.229;

export interface PathPoint {
  /** Degrees east, 0 to 360. */
  lon: number;
  lat: number;
  /** Pressure in hectopascals. */
  hpa: number;
  /** Metres above the ground. */
  heightM: number;
}

/** One leg of the journey: consecutive hours over the same region. */
export interface Leg {
  region: Region | undefined;
  /** Hours before the start when the air left this region (nearest to now). */
  untilHoursAgo: number;
  /** Hours before the start when the air entered this region. */
  fromHoursAgo: number;
}

export interface Story {
  /** Hours of path the story covers. */
  hours: number;
  /** Distance along the path, in kilometres. */
  pathKm: number;
  /** Straight-line distance from the oldest point to the start. */
  originKm: number;
  /** Greatest height above the ground, in metres, and when. */
  peakM: number;
  peakHoursAgo: number;
  /** Fastest hourly speed, in kilometres per hour. */
  topSpeedKmh: number;
  /** Kilometres along the path at each hour. */
  cumulativeKm: number[];
  /** Legs from the start back to the oldest point. */
  legs: Leg[];
  origin: Region | undefined;
  headline: string;
}

export function distanceKm(a: PathPoint, b: PathPoint): number {
  const rad = Math.PI / 180;
  const dLat = (b.lat - a.lat) * rad;
  const dLon = (b.lon - a.lon) * rad;
  const h = Math.sin(dLat / 2) ** 2 + Math.cos(a.lat * rad) * Math.cos(b.lat * rad) * Math.sin(dLon / 2) ** 2;
  return 2 * EARTH_RADIUS_KM * Math.asin(Math.min(1, Math.sqrt(h)));
}

/** "now", "yesterday", "3 days ago", "5 hours ago". */
export function ago(hours: number): string {
  if (hours < 1) return "now";
  if (hours < 24) return hours === 1 ? "an hour ago" : `${Math.round(hours)} hours ago`;
  const days = Math.round(hours / 24);
  return days === 1 ? "yesterday" : `${days} days ago`;
}

/** 1234.5 → "1,200"; 87 → "90": distances are not known to the kilometre. */
export function roundKm(km: number): string {
  const step = km >= 1000 ? 100 : 10;
  return (Math.round(km / step) * step).toLocaleString("en-US");
}

/** Legs shorter than this are folded into their neighbour. */
const MIN_LEG_HOURS = 3;

function buildLegs(path: PathPoint[], regionAt: (point: PathPoint) => Region | undefined): Leg[] {
  const legs: Leg[] = [];
  for (const [hour, point] of path.entries()) {
    const region = regionAt(point);
    const last = legs.at(-1);
    if (last && last.region?.name === region?.name) {
      last.fromHoursAgo = hour;
    } else {
      legs.push({ region, untilHoursAgo: hour, fromHoursAgo: hour });
    }
  }
  // Drop brief crossings (a corner of a country clipped for an hour), then
  // join neighbours that turned out to be the same region.
  const kept = legs.filter(
    (leg, index) =>
      index === 0 || index === legs.length - 1 || leg.fromHoursAgo - leg.untilHoursAgo + 1 >= MIN_LEG_HOURS,
  );
  const joined: Leg[] = [];
  for (const leg of kept) {
    const last = joined.at(-1);
    if (last && last.region?.name === leg.region?.name) last.fromHoursAgo = leg.fromHoursAgo;
    else joined.push({ ...leg });
  }
  for (const [index, leg] of joined.entries()) {
    const next = joined[index + 1];
    if (next) leg.fromHoursAgo = next.untilHoursAgo - 1;
  }
  return joined;
}

export function tellStory(place: string, path: PathPoint[], regionAt: (point: PathPoint) => Region | undefined): Story {
  const start = path[0];
  const oldest = path.at(-1);
  if (!start || !oldest || path.length < 2) throw new Error("The path is too short to describe.");

  const cumulativeKm = [0];
  let topSpeedKmh = 0;
  let peakM = Number.NEGATIVE_INFINITY;
  let peakHoursAgo = 0;
  for (const [hour, point] of path.entries()) {
    if (point.heightM > peakM) {
      peakM = point.heightM;
      peakHoursAgo = hour;
    }
    const previous = path[hour - 1];
    if (previous) {
      const step = distanceKm(previous, point);
      topSpeedKmh = Math.max(topSpeedKmh, step);
      cumulativeKm.push((cumulativeKm[hour - 1] as number) + step);
    }
  }

  const hours = path.length - 1;
  const origin = regionAt(oldest);
  const originKm = distanceKm(start, oldest);
  const where = origin ? `over ${inSentence(origin)}` : "far from any coast";
  const when = ago(hours);
  const headline = `${when[0]?.toUpperCase()}${when.slice(1)}, the air above ${place} was ${where}.`;

  return {
    hours,
    pathKm: cumulativeKm.at(-1) as number,
    originKm,
    peakM,
    peakHoursAgo,
    topSpeedKmh,
    cumulativeKm,
    legs: buildLegs(path, regionAt),
    origin,
    headline,
  };
}
