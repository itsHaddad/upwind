// The parts of the page that talk to the device around it: its position,
// its share sheet, its clock, and the browser's task queue.

import { type Place, placeFromQuery, searchPlaces, timezoneCity } from "./places";

/** Something to tell the person, and whether it is a problem. */
export interface Notice {
  text: string;
  isError: boolean;
}

/** Yields to the browser. A message, not a timer or animation frame: those slow to a crawl in a hidden tab. */
export function nextTask(): Promise<void> {
  return new Promise((resolve) => {
    const channel = new MessageChannel();
    channel.port1.onmessage = () => resolve();
    channel.port2.postMessage(null);
  });
}

/**
 * Hands the page address and a line of text to the device's share sheet,
 * or copies the address where there is none. Returns what to tell the
 * person, if anything.
 */
export async function share(text: string): Promise<Notice | undefined> {
  try {
    if (navigator.share) {
      await navigator.share({ title: document.title, text, url: location.href });
      return undefined;
    }
    await navigator.clipboard.writeText(location.href);
    return { text: "Link copied. Anyone who opens it sees this path.", isError: false };
  } catch (error) {
    // Closing the share sheet is not an error worth reporting.
    if (error instanceof DOMException && error.name === "AbortError") return undefined;
    return { text: "The link could not be shared. Copy it from the address bar.", isError: true };
  }
}

/** Asks the browser where the device is. */
export function locate(onPlace: (place: Place) => void, onNotice: (notice: Notice) => void): void {
  if (!navigator.geolocation) {
    onNotice({ text: "This browser cannot share a location. Type a place instead.", isError: true });
    return;
  }
  onNotice({ text: "Asking your browser where you are…", isError: false });
  navigator.geolocation.getCurrentPosition(
    ({ coords }) =>
      onPlace({
        name: "your location",
        detail: "",
        // Two decimals is about a kilometre: plenty for wind, kinder to privacy.
        lon: Math.round(coords.longitude * 100) / 100,
        lat: Math.round(coords.latitude * 100) / 100,
      }),
    () => onNotice({ text: "Your location was not shared. Type a place instead.", isError: true }),
    { maximumAge: 600_000, timeout: 15_000 },
  );
}

/** The place to show first: the one in the page address, or the city the device's time zone is named after. */
export async function firstPlace(): Promise<Place | undefined> {
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
