// A camera that orbits the globe: drag to turn, wheel or pinch to zoom, and
// smooth flights to a view.

import { direction, lonDelta, lonLat, type Vec3 } from "./mat";

export interface View {
  /** The point the camera looks straight down on. */
  lon: number;
  lat: number;
  /** Distance from the globe's centre in globe radii. */
  distance: number;
}

const MIN_DISTANCE = 1.12;
const MAX_DISTANCE = 8;
const MAX_LAT = 85;
export const FIELD_OF_VIEW = 32;

const clamp = (value: number, low: number, high: number) => Math.min(high, Math.max(low, value));
const ease = (t: number) => (t < 0.5 ? 4 * t * t * t : 1 - (-2 * t + 2) ** 3 / 2);

/** The part of the canvas a view should fit into, as fractions of its width and height. */
export interface FreeArea {
  width: number;
  height: number;
}

/**
 * A view that fits the given points (unit vectors) into the free part of a
 * canvas with the given width over height.
 */
export function frame(points: Vec3[], aspect: number, free: FreeArea, padding = 1.25): View {
  const sum: Vec3 = [0, 0, 0];
  for (const p of points) {
    sum[0] += p[0];
    sum[1] += p[1];
    sum[2] += p[2];
  }
  const length = Math.hypot(sum[0], sum[1], sum[2]);
  if (length < 1e-6) return { lon: 0, lat: 20, distance: 3.6 };
  const centre: Vec3 = [sum[0] / length, sum[1] / length, sum[2] / length];
  let spread = 0;
  for (const p of points) {
    const cos = clamp(p[0] * centre[0] + p[1] * centre[1] + p[2] * centre[2], -1, 1);
    spread = Math.max(spread, Math.acos(cos));
  }
  // Distance at which a cap of angular radius `spread` fills the narrower
  // side of the free area.
  const tanHalf = Math.tan(((FIELD_OF_VIEW / 2) * Math.PI) / 180);
  const halfNarrow = Math.atan(tanHalf * Math.min(free.height, free.width * aspect));
  const reach = Math.min(spread * padding, 1.25);
  const distance = Math.cos(reach) + Math.sin(reach) / Math.tan(halfNarrow);
  const [lon, lat] = lonLat(centre);
  return { lon, lat: clamp(lat, -MAX_LAT, MAX_LAT), distance: clamp(distance, 1.45, 7) };
}

export class Camera {
  view: View = { lon: 10, lat: 25, distance: 3.6 };
  /** Degrees per second the globe drifts when nobody is touching it. */
  drift = 0;
  private flight: { from: View; to: View; start: number; ms: number } | undefined;
  private readonly pointers = new Map<number, { x: number; y: number }>();
  private pinch = 0;
  /** Called when the person takes hold of the globe. */
  onGrab: (() => void) | undefined;

  constructor(private readonly element: HTMLElement) {
    element.addEventListener("pointerdown", (event) => this.down(event));
    element.addEventListener("pointermove", (event) => this.move(event));
    for (const type of ["pointerup", "pointercancel", "pointerleave"] as const) {
      element.addEventListener(type, (event) => this.up(event));
    }
    element.addEventListener("wheel", (event) => this.wheel(event), { passive: false });
  }

  get eye(): Vec3 {
    const d = direction(this.view.lon, this.view.lat);
    return [d[0] * this.view.distance, d[1] * this.view.distance, d[2] * this.view.distance];
  }

  flyTo(to: View, ms = 1600): void {
    this.flight = { from: { ...this.view }, to, start: performance.now(), ms };
  }

  /** Advances flights and drift; call once per frame. */
  update(now: number, deltaMs: number): void {
    if (this.flight) {
      const { from, to, start, ms } = this.flight;
      const t = clamp((now - start) / ms, 0, 1);
      const k = ease(t);
      this.view = {
        lon: from.lon + lonDelta(from.lon, to.lon) * k,
        lat: from.lat + (to.lat - from.lat) * k,
        distance: from.distance + (to.distance - from.distance) * k,
      };
      if (t >= 1) this.flight = undefined;
    } else if (this.pointers.size === 0 && this.drift !== 0) {
      this.view.lon += (this.drift * deltaMs) / 1000;
    }
  }

  private grab(): void {
    this.flight = undefined;
    this.drift = 0;
    this.onGrab?.();
  }

  private down(event: PointerEvent): void {
    this.element.setPointerCapture(event.pointerId);
    this.pointers.set(event.pointerId, { x: event.clientX, y: event.clientY });
    this.pinch = this.spread();
    this.grab();
  }

  private move(event: PointerEvent): void {
    const last = this.pointers.get(event.pointerId);
    if (!last) return;
    const dx = event.clientX - last.x;
    const dy = event.clientY - last.y;
    last.x = event.clientX;
    last.y = event.clientY;
    if (this.pointers.size === 1) {
      // Turn so that the point under the finger roughly follows it.
      const height = this.element.clientHeight || 1;
      const visible = 2 * Math.tan((FIELD_OF_VIEW * Math.PI) / 360) * (this.view.distance - 1);
      const degreesPerPixel = ((visible / height) * 180) / Math.PI;
      this.view.lon -= dx * degreesPerPixel;
      this.view.lat = clamp(this.view.lat + dy * degreesPerPixel, -MAX_LAT, MAX_LAT);
    } else {
      const spread = this.spread();
      if (this.pinch > 0 && spread > 0) this.zoom(this.pinch / spread);
      this.pinch = spread;
    }
  }

  private up(event: PointerEvent): void {
    this.pointers.delete(event.pointerId);
    this.pinch = this.spread();
  }

  private wheel(event: WheelEvent): void {
    event.preventDefault();
    this.grab();
    this.zoom(Math.exp(event.deltaY * 0.0012));
  }

  /** Scales the height above the surface by `factor`. */
  private zoom(factor: number): void {
    this.view.distance = clamp(1 + (this.view.distance - 1) * factor, MIN_DISTANCE, MAX_DISTANCE);
  }

  private spread(): number {
    const [a, b] = [...this.pointers.values()];
    return a && b ? Math.hypot(a.x - b.x, a.y - b.y) : 0;
  }
}
