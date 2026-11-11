// The story panel: headline, facts, timeline and the list of legs.

import { inSentence, type Region } from "./regions";
import { ago, type Leg, type PathPoint, roundKm, type Story } from "./story";

const byId = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

const PLAY_ICON = "M7 4l14 8-14 8z";
const PAUSE_ICON = "M6 4h4v16H6zM14 4h4v16h-4z";

const whenFormat = new Intl.DateTimeFormat(undefined, {
  weekday: "short",
  day: "numeric",
  month: "short",
  hour: "2-digit",
  minute: "2-digit",
});

/** "2–4 days ago", "0–18 hours ago". */
function span(leg: Leg): string {
  if (leg.fromHoursAgo < 48) return `${leg.untilHoursAgo}–${leg.fromHoursAgo} hours ago`;
  const from = Math.round(leg.untilHoursAgo / 24);
  const to = Math.round(leg.fromHoursAgo / 24);
  return from === to ? `${to} days ago` : `${from}–${to} days ago`;
}

function title(region: Region | undefined): string {
  if (!region) return "Open water";
  const text = inSentence(region).replace(/^the /, "");
  return `${text[0]?.toUpperCase()}${text.slice(1)}`;
}

export class Panel {
  private readonly headline = byId("headline");
  private readonly kicker = byId("kicker");
  private readonly status = byId("status");
  private readonly result = byId("result");
  private readonly moment = byId("moment");
  private readonly legs = byId<HTMLOListElement>("legs");
  private readonly scrub = byId<HTMLInputElement>("scrub");
  private readonly playIcon = document.getElementById("play-icon") as unknown as SVGPathElement;
  private legItems: { leg: Leg; item: HTMLLIElement }[] = [];
  private shownHour = -1;
  private label = "";
  /** The person moved the timeline to this hour. */
  onScrub: ((hour: number) => void) | undefined;
  onPlayToggle: (() => void) | undefined;
  onShare: (() => void) | undefined;

  constructor() {
    this.scrub.addEventListener("input", () => this.onScrub?.(Number(this.scrub.value)));
    byId("play").addEventListener("click", () => this.onPlayToggle?.());
    byId("share").addEventListener("click", () => this.onShare?.());
  }

  setStatus(text: string, isError = false): void {
    this.status.textContent = text;
    this.status.classList.toggle("error", isError);
  }

  setSource(generatedUnix: number): void {
    const time = new Intl.DateTimeFormat(undefined, { hour: "2-digit", minute: "2-digit" }).format(
      generatedUnix * 1000,
    );
    byId("source").textContent = `Wind from NOAA's GFS model, updated ${time}.`;
  }

  /** A trace has begun for a place. */
  begin(placeName: string): void {
    this.kicker.textContent = "Following the wind back";
    this.headline.textContent = `Where has the air above ${placeName} been?`;
    this.result.hidden = true;
    this.shownHour = -1;
    this.label = "";
  }

  /** The trace is complete. */
  finish(story: Story, onLeg: (leg: Leg) => void): void {
    this.kicker.textContent = "Where has your air been?";
    this.headline.textContent = story.headline;
    byId("fact-distance").textContent = `${roundKm(story.pathKm)} km`;
    byId("fact-peak").textContent = `${roundKm(Math.max(0, story.peakM))} m`;
    byId("fact-speed").textContent = `${Math.round(story.topSpeedKmh)} km/h`;
    this.scrub.max = String(story.hours);
    this.legItems = story.legs.map((leg) => {
      const item = document.createElement("li");
      const name = document.createElement("span");
      name.textContent = title(leg.region);
      const when = document.createElement("span");
      when.textContent = span(leg);
      item.append(name, when);
      item.addEventListener("click", () => onLeg(leg));
      return { leg, item };
    });
    this.legs.replaceChildren(...this.legItems.map(({ item }) => item));
    this.result.hidden = false;
  }

  setPlaying(playing: boolean): void {
    this.playIcon.setAttribute("d", playing ? PAUSE_ICON : PLAY_ICON);
    byId("play").ariaLabel = playing ? "Pause" : "Play";
  }

  /**
   * Describes the moment `whole` hours back, given where the air was then.
   * `locate` is only called when the hour changes. Returns the short label
   * for the globe.
   */
  showMoment(whole: number, startTime: number, locate: () => { point: PathPoint; region: Region | undefined }): string {
    if (whole !== this.shownHour) {
      const { point, region } = locate();
      const where = region ? `over ${inSentence(region)}` : "over open water";
      this.shownHour = whole;
      this.scrub.value = String(whole);
      const when = whenFormat.format((startTime - whole * 3600) * 1000);
      const height = roundKm(point.heightM);
      const lead = document.createElement("b");
      lead.textContent = `${ago(whole)[0]?.toUpperCase()}${ago(whole).slice(1)}`;
      this.moment.replaceChildren(lead, ` · ${when} · ${where}, about ${height} m above the ground`);
      for (const { leg, item } of this.legItems) {
        item.classList.toggle("current", whole >= leg.untilHoursAgo && whole <= leg.fromHoursAgo);
      }
      this.label = whole === 0 ? "" : `${ago(whole)} · ${title(region)}`;
    }
    return this.label;
  }
}
