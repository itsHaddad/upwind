// The globe: owns the canvas, the camera and the frame loop.

import { Camera, FIELD_OF_VIEW } from "./camera";
import { Earth } from "./earth";
import { direction, lookAtOrigin, type Mat4, multiply, perspective, shifted, transform } from "./mat";
import { type PathSet, Paths } from "./paths";

export interface ScreenPoint {
  /** CSS pixels from the canvas's top-left corner. */
  x: number;
  y: number;
  /** False when the point is behind the globe or off screen. */
  visible: boolean;
}

export class Globe {
  readonly camera: Camera;
  readonly paths: Paths;
  private readonly gl: WebGL2RenderingContext;
  private readonly earth: Earth;
  private viewProjection: Mat4 = new Float32Array(16);
  private lastFrame = performance.now();
  /** Moves the globe's centre: 0.2 shifts it up by a tenth of the canvas. */
  shiftY = 0;
  /** The same sideways: 0.2 shifts the globe right by a tenth of the canvas. */
  shiftX = 0;
  /** Called every frame after drawing, for labels that follow the globe. */
  onFrame: ((now: number) => void) | undefined;

  constructor(readonly canvas: HTMLCanvasElement) {
    const gl = canvas.getContext("webgl2", { antialias: true, alpha: true, premultipliedAlpha: true });
    if (!gl) throw new Error("This browser cannot draw the globe (WebGL 2 is unavailable).");
    this.gl = gl;
    this.earth = new Earth(gl);
    this.paths = new Paths(gl);
    this.camera = new Camera(canvas);
    requestAnimationFrame((now) => this.frame(now));
  }

  setMap(map: TexImageSource): void {
    this.earth.setMap(map);
  }

  setPaths(paths: PathSet): void {
    this.paths.set(paths);
  }

  /** Largest texture width the device handles comfortably. */
  get mapWidth(): number {
    const limit = this.gl.getParameter(this.gl.MAX_TEXTURE_SIZE) as number;
    const small = Math.min(window.innerWidth, window.innerHeight) < 700;
    return Math.min(limit, small ? 2048 : 4096);
  }

  get aspect(): number {
    return this.canvas.clientWidth / Math.max(1, this.canvas.clientHeight);
  }

  /** Where a point above the globe lands on screen. */
  project(lon: number, lat: number, radius = 1): ScreenPoint {
    const d = direction(lon, lat);
    const [x, y, , w] = transform(this.viewProjection, [d[0] * radius, d[1] * radius, d[2] * radius]);
    const eye = this.camera.eye;
    // Visible when the point is on the near side of the horizon.
    const facing = d[0] * eye[0] + d[1] * eye[1] + d[2] * eye[2] > 1;
    const width = this.canvas.clientWidth;
    const height = this.canvas.clientHeight;
    return {
      x: ((x / w) * 0.5 + 0.5) * width,
      y: (0.5 - (y / w) * 0.5) * height,
      visible: facing && w > 0,
    };
  }

  private resize(): number {
    const ratio = Math.min(window.devicePixelRatio || 1, 2);
    const width = Math.round(this.canvas.clientWidth * ratio);
    const height = Math.round(this.canvas.clientHeight * ratio);
    if (this.canvas.width !== width || this.canvas.height !== height) {
      this.canvas.width = width;
      this.canvas.height = height;
    }
    return ratio;
  }

  private frame(now: number): void {
    const gl = this.gl;
    const ratio = this.resize();
    this.camera.update(now, Math.min(100, now - this.lastFrame));
    this.lastFrame = now;

    const distance = this.camera.view.distance;
    const projection = perspective(FIELD_OF_VIEW, this.aspect, Math.max(0.05, distance - 1.3), distance + 1.3);
    const eye = this.camera.eye;
    this.viewProjection = multiply(shifted(projection, this.shiftX, this.shiftY), lookAtOrigin(eye));

    gl.viewport(0, 0, this.canvas.width, this.canvas.height);
    gl.clearColor(0, 0, 0, 0);
    gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);
    gl.enable(gl.DEPTH_TEST);
    this.earth.draw(this.viewProjection, eye);
    this.paths.draw(this.viewProjection, [this.canvas.width, this.canvas.height], ratio);

    this.onFrame?.(now);
    requestAnimationFrame((next) => this.frame(next));
  }
}
