// Finding places by name.

export interface Place {
  name: string;
  /** Region and country, for telling places with the same name apart. */
  detail: string;
  /** Degrees east, -180 to 180. */
  lon: number;
  lat: number;
}

interface GeocoderResult {
  name: string;
  latitude: number;
  longitude: number;
  admin1?: string;
  country?: string;
}

const GEOCODER = "https://geocoding-api.open-meteo.com/v1/search";

/** Places matching a name, best match first. */
export async function searchPlaces(query: string, signal?: AbortSignal): Promise<Place[]> {
  const text = query.trim();
  if (text.length < 2) return [];
  const url = `${GEOCODER}?name=${encodeURIComponent(text)}&count=6&language=en&format=json`;
  const response = await fetch(url, { signal });
  if (!response.ok) throw new Error("The place search is not answering.");
  const body = (await response.json()) as { results?: GeocoderResult[] };
  return (body.results ?? []).map((r) => ({
    name: r.name,
    detail: [r.admin1, r.country].filter((part) => part && part !== r.name).join(", "),
    lon: r.longitude,
    lat: r.latitude,
  }));
}

/**
 * The city the browser's time zone is named after ("Europe/Stockholm" →
 * "Stockholm"): a guess at somewhere near the visitor that needs no
 * permission and sends nothing about them.
 */
export function timezoneCity(zone = Intl.DateTimeFormat().resolvedOptions().timeZone): string | undefined {
  const city = zone?.split("/").at(-1)?.replaceAll("_", " ");
  if (!city || !zone.includes("/") || /^(UTC|GMT|Etc)/i.test(zone)) return undefined;
  return city;
}

/** Reads a place from the page address, as written by `placeToQuery`. */
export function placeFromQuery(search: string): Place | undefined {
  const params = new URLSearchParams(search);
  const lat = Number(params.get("lat"));
  const lon = Number(params.get("lon"));
  if (!params.has("lat") || !params.has("lon") || !Number.isFinite(lat) || !Number.isFinite(lon)) return undefined;
  if (Math.abs(lat) > 90 || Math.abs(lon) > 360) return undefined;
  const name = (params.get("name") ?? "").slice(0, 80).trim();
  return { name: name || "this place", detail: "", lon: ((lon + 540) % 360) - 180, lat };
}

export function placeToQuery(place: Place): string {
  const params = new URLSearchParams({
    lat: place.lat.toFixed(2),
    lon: place.lon.toFixed(2),
    name: place.name,
  });
  return `?${params}`;
}
