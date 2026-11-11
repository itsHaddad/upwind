// The page: wires the search box, the engine, the globe and the story panel.

import type { GeometryCollection, Topology } from "topojson-specification";
import { Engine } from "./engine";
import { type FreeArea, frame } from "./globe/camera";
import { paintMap } from "./globe/earth";
import { Globe } from "./globe/globe";
import { direction } from "./globe/mat";
import { drawnRadius } from "./globe/paths";
import { ensemble, MAX_HOURS, mainPath, pointAt, toPathSet } from "./journey";
import { Panel } from "./panel";
import { type Place, placeFromQuery, placeToQuery, searchPlaces, timezoneCity } from "./places";
import { Regions } from "./regions";
import { SearchBox } from "./search";
import { type PathPoint, tellStory } from "./story";
import { availableHours, latestStart, runTrace } from "./trace";
import { Winds } from "./winds";

/** Seconds the whole history takes to play. */
const PLAY_SECONDS = 9;
/** Where the wind history is served from; set UPWIND_WIND_BASE when building to point at a bucket. */
const WIND_BASE = process.env.UPWIND_WIND_BASE ?? "./data";

const byId = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;
const panel = new Panel();
const globe = new Globe(byId<HTMLCanvasElement>("globe"));
const pin = byId("pin");
const head = byId("head");

interface Current {
  place: Place;
  startTime: number;
  path: PathPoint[];
  /** Hours revealed so far; follows the trace, then the timeline. */
  hour: number;
  playing: boolean;
  /** Whole hours the engine has produced. */
  ready: number;
}

let current: Current | undefined;
let tracing: AbortController | undefined;
let regions: Regions | undefined;
let lastFrame = performance.now();
/** How the last trace went, for tests and tuning. */
let lastTrace: { waits: number; waitMs: number; files: number } | undefined;

/** Yields to the browser. A message, not a timer or animation frame: those slow to a crawl in a hidden tab. */
function nextTask(): Promise<void> {
  return new Promise((resolve) => {
    const channel = new MessageChannel();
    channel.port1.onmessage = () => resolve();
    channel.port2.postMessage(null);
  });
}

/** Whether the panel sits beside the globe (wide screens) or below it (phones). */
const isWide = () => window.innerWidth > 720;

function layout(): void {
  globe.shiftX = isWide() ? 0.3 : 0;
  globe.shiftY = isWide() ? 0 : 0.42;
}

/** The part of the screen the panel leaves free, which is where the path has to fit. */
function freeArea(): FreeArea {
  return isWide() ? { width: 0.62, height: 0.84 } : { width: 0.92, height: 0.4 };
}

function place(marker: HTMLElement, lon: number, lat: number, radius: number, label?: string): void {
  const at = globe.project(lon, lat, radius);
  marker.hidden = false;
  marker.style.opacity = at.visible ? "1" : "0";
  marker.style.transform = `translate(${at.x.toFixed(1)}px, ${at.y.toFixed(1)}px)`;
  if (label !== undefined) {
    const text = marker.querySelector("span") as HTMLElement;
    if (text.textContent !== label) text.textContent = label;
    text.hidden = label === "";
  }
}

globe.onFrame = (now) => {
  const seconds = Math.min(0.1, (now - lastFrame) / 1000);
  lastFrame = now;
  if (!current) return;
  if (current.playing) {
    const limit = Math.max(0, current.ready);
    current.hour = Math.min(limit, current.hour + (seconds * MAX_HOURS) / PLAY_SECONDS);
    if (current.hour >= current.path.length - 1 && tracing === undefined) setPlaying(false);
  }
  globe.paths.head = current.hour;
  place(pin, current.place.lon, current.place.lat, 1, current.place.name);
  const point = pointAt(current.path, current.hour);
  if (!point) return;
  // The text describes the nearest whole hour, so that it agrees with the
  // headline and the list of legs, which are built from hourly points.
  const whole = Math.min(current.path.length - 1, Math.round(current.hour));
  const state = current;
  const label = panel.showMoment(whole, current.startTime, () => {
    const at = state.path[whole] ?? point;
    return { point: at, region: regions?.at(at.lon, at.lat) };
  });
  place(head, point.lon, point.lat, drawnRadius(point.heightM), label);
};

function setPlaying(playing: boolean): void {
  if (!current) return;
  if (playing && current.hour >= current.path.length - 1) current.hour = 0;
  current.playing = playing;
  panel.setPlaying(playing);
}

panel.onScrub = (hour) => {
  if (!current) return;
  setPlaying(false);
  current.hour = Math.min(hour, current.ready);
};
panel.onPlayToggle = () => setPlaying(!current?.playing);
panel.onShare = () => void share();

/** Hands the page address and headline to the device's share sheet, or copies the address. */
async function share(): Promise<void> {
  const text = byId("headline").textContent ?? "";
  try {
    if (navigator.share) {
      await navigator.share({ title: document.title, text, url: location.href });
    } else {
      await navigator.clipboard.writeText(location.href);
      panel.setStatus("Link copied. Anyone who opens it sees this path.");
    }
  } catch (error) {
    // Closing the share sheet is not an error worth reporting.
    if (!(error instanceof DOMException && error.name === "AbortError")) {
      panel.setStatus("The link could not be shared. Copy it from the address bar.", true);
    }
  }
}

async function follow(engine: Engine, winds: Winds, chosen: Place): Promise<void> {
  tracing?.abort();
  const mine = new AbortController();
  tracing = mine;
  const startTime = latestStart(winds, Date.now() / 1000);
  const hours = availableHours(winds, startTime, MAX_HOURS);
  const starts = ensemble(chosen);
  current = { place: chosen, startTime, path: [], hour: 0, playing: true, ready: 0 };
  const state = current;

  history.replaceState(null, "", placeToQuery(chosen));
  document.title = `Upwind — the air above ${chosen.name}`;
  panel.begin(chosen.name);
  panel.setPlaying(true);
  panel.setStatus("Fetching the wind…");
  globe.paths.clear();
  globe.paths.hours = hours;
  globe.camera.drift = 0;
  globe.camera.flyTo({ lon: chosen.lon, lat: chosen.lat, distance: 2.1 }, 1400);

  try {
    const result = await runTrace(
      engine,
      winds,
      { starts, startTime, hours },
      {
        rowsPerChunk: 24,
        // People who asked their browser to save data get only the wind the paths need.
        lookAhead: !(navigator as Navigator & { connection?: { saveData?: boolean } }).connection?.saveData,
        signal: mine.signal,
        pause: nextTask,
        onProgress: ({ rows, points }) => {
          state.path = mainPath(points, starts.length);
          state.ready = state.path.length - 1;
          globe.setPaths(toPathSet(points, starts.length));
          panel.setStatus(`Following the wind back… ${Math.max(0, rows - 1)} of ${hours} hours`);
        },
      },
    );
    if (mine.signal.aborted) return;
    tracing = undefined;
    lastTrace = { waits: result.waits, waitMs: Math.round(result.waitMs), files: winds.files };
    const megabytes = (winds.bytes / 1e6).toFixed(1);
    panel.setStatus(
      `${starts.length} paths traced in ${Math.max(1, Math.round(result.computeMs))} ms on your device, from ${megabytes} MB of wind.`,
    );
    const story = tellStory(chosen.name, state.path, (p) => regions?.at(p.lon, p.lat));
    panel.finish(story, (leg) => {
      setPlaying(false);
      state.hour = (leg.untilHoursAgo + leg.fromHoursAgo) / 2;
    });
    const view = frame(
      state.path.filter((_, hour) => hour % 3 === 0).map((p) => direction(p.lon, p.lat)),
      globe.aspect,
      freeArea(),
    );
    globe.camera.flyTo(view, 4200);
  } catch (error) {
    if (mine.signal.aborted) return;
    tracing = undefined;
    panel.setStatus(error instanceof Error ? error.message : String(error), true);
  }
}

async function firstPlace(): Promise<Place | undefined> {
  const fromAddress = placeFromQuery(location.search);
  if (fromAddress) return fromAddress;
  const city = timezoneCity();
  if (!city) return undefined;
  try {
    return (await searchPlaces(city))[0];
  } catch {
    return undefined;
  }
}

function locate(pick: (place: Place) => void): void {
  if (!navigator.geolocation) {
    panel.setStatus("This browser cannot share a location. Type a place instead.", true);
    return;
  }
  panel.setStatus("Asking your browser where you are…");
  navigator.geolocation.getCurrentPosition(
    ({ coords }) =>
      pick({
        name: "your location",
        detail: "",
        // Two decimals is about a kilometre: plenty for wind, kinder to privacy.
        lon: Math.round(coords.longitude * 100) / 100,
        lat: Math.round(coords.latitude * 100) / 100,
      }),
    () => panel.setStatus("Your location was not shared. Type a place instead.", true),
    { maximumAge: 600_000, timeout: 15_000 },
  );
}

async function start(): Promise<void> {
  layout();
  window.addEventListener("resize", layout);
  globe.camera.drift = 2.5;
  globe.camera.onGrab = () => {
    if (current?.playing && tracing === undefined) setPlaying(false);
  };

  type Atlas = Topology<{ countries: GeometryCollection; land: GeometryCollection }>;
  void fetch("./countries-50m.json")
    .then((response) => response.json() as Promise<Atlas>)
    .then((atlas) => globe.setMap(paintMap(atlas, globe.mapWidth)))
    .catch(() => panel.setStatus("The map could not be loaded; the paths will still be drawn.", true));
  void Regions.load(".")
    .then((loaded) => {
      regions = loaded;
    })
    .catch(() => undefined);

  const [engine, winds] = await Promise.all([
    fetch("./engine.wasm")
      .then((response) => response.arrayBuffer())
      .then((bytes) => Engine.load(bytes)),
    Winds.open(WIND_BASE),
  ]);
  engine.reset(winds.times);
  panel.setSource(winds.manifest.generated);

  const pick = (chosen: Place) => void follow(engine, winds, chosen);
  const search = new SearchBox(byId<HTMLFormElement>("search"), pick, (message) => panel.setStatus(message, true));
  byId("locate").addEventListener("click", () => locate(pick));

  const first = await firstPlace();
  if (first) {
    search.show(first);
    pick(first);
  }
}

// A handle for tests and for poking at the page from the console.
Object.assign(window, { upwind: { globe, current: () => current, lastTrace: () => lastTrace } });

start().catch((error: unknown) => {
  panel.setStatus(error instanceof Error ? error.message : String(error), true);
});
