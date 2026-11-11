// The page: wires the search box, the engine, the globe and the story panel.

import type { GeometryCollection, Topology } from "topojson-specification";
import { firstPlace, locate, type Notice, nextTask, share } from "./device";
import { Engine } from "./engine";
import { type FreeArea, frame } from "./globe/camera";
import { paintMap } from "./globe/earth";
import { Globe } from "./globe/globe";
import { direction } from "./globe/mat";
import { drawnRadius } from "./globe/paths";
import { ensemble, MAX_HOURS, mainPath, pointAt, toPathSet } from "./journey";
import { Panel } from "./panel";
import { type Place, placeToQuery } from "./places";
import { Regions } from "./regions";
import { SearchBox } from "./search";
import { type PathPoint, tellStory } from "./story";
import { availableHours, latestStart, runTrace } from "./trace";
import { MissingWindError, Winds } from "./winds";

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
/** Settles when the place names have loaded or failed to. */
let regionsSettled: Promise<void> = Promise.resolve();
/** The engine and the wind history it is reading. */
let session: { engine: Engine; winds: Winds } | undefined;
let lastFrame = performance.now();
/** How the last trace went, for tests and tuning. */
let lastTrace: { waits: number; waitMs: number; files: number } | undefined;

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
const notify = (notice: Notice | undefined) => {
  if (notice) panel.setStatus(notice.text, notice.isError);
};
panel.onShare = () => void share(byId("headline").textContent ?? "").then(notify);

/**
 * The wind history is refreshed every few hours and old days are removed.
 * A page that has been open across a refresh holds an out-of-date list of
 * files; this fetches the current one.
 */
async function refreshWinds(): Promise<void> {
  if (!session) return;
  session.winds = await Winds.open(WIND_BASE);
  session.engine.reset(session.winds.times);
  panel.setSource(session.winds.manifest.generated);
}

async function follow(chosen: Place, isRetry = false): Promise<void> {
  if (!session) return;
  const { engine, winds } = session;
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
    lastTrace = { waits: result.waits, waitMs: Math.round(result.waitMs), files: result.files };
    // The story names places; wait for the names rather than tell it without them.
    await regionsSettled;
    if (mine.signal.aborted) return;
    const megabytes = (result.bytes / 1e6).toFixed(1);
    const fetched = result.files > 0 ? `, from ${megabytes} MB of wind` : "";
    const unnamed = regions ? "" : " Place names could not be loaded.";
    panel.setStatus(
      `${starts.length} paths traced in ${Math.max(1, Math.round(result.computeMs))} ms on your device${fetched}.${unnamed}`,
      !regions,
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
    if (error instanceof MissingWindError && !isRetry) {
      panel.setStatus("Fetching the newest wind…");
      try {
        await refreshWinds();
      } catch (refreshError) {
        panel.setStatus(refreshError instanceof Error ? refreshError.message : String(refreshError), true);
        return;
      }
      // Unless the person picked another place while that was loading.
      if (tracing === undefined) await follow(chosen, true);
      return;
    }
    panel.setStatus(error instanceof Error ? error.message : String(error), true);
  }
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
  regionsSettled = Regions.load(".")
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
  session = { engine, winds };

  const pick = (chosen: Place) => void follow(chosen);
  const search = new SearchBox(byId<HTMLFormElement>("search"), pick, (message) => panel.setStatus(message, true));
  byId("locate").addEventListener("click", () => locate(pick, notify));

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
