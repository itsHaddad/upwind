// The traced air, drawn as glowing ribbons above the globe.
//
// Every hour of every parcel is one instance of a quad that is widened in
// screen space, so lines keep their width at any zoom.

import { createProgram, type Program, staticBuffer } from "./gl";
import { direction, type Mat4 } from "./mat";

/** How much higher than true scale the air is drawn above the ground. */
export const HEIGHT_EXAGGERATION = 70;
const EARTH_RADIUS_M = 6_371_229;

/** Radius at which air `heightM` above the ground is drawn; the globe's surface is 1. */
export function drawnRadius(heightM: number): number {
  return 1.004 + (Math.max(0, heightM) * HEIGHT_EXAGGERATION) / EARTH_RADIUS_M;
}

const VERTEX = `
in vec2 aCorner;
in vec4 aStart;
in vec4 aEnd;
in vec2 aHourKind;
uniform mat4 uViewProjection;
uniform vec2 uViewport;
uniform float uWidth;
uniform float uLift;
out float vHour;
out float vSide;
out float vKind;
void main() {
  vec4 a = uViewProjection * vec4(aStart.xyz * mix(1.002, aStart.w, uLift), 1.0);
  vec4 b = uViewProjection * vec4(aEnd.xyz * mix(1.002, aEnd.w, uLift), 1.0);
  vec2 pa = a.xy / a.w * uViewport;
  vec2 pb = b.xy / b.w * uViewport;
  vec2 along = pb - pa;
  float len = length(along);
  along = len > 1e-4 ? along / len : vec2(1.0, 0.0);
  vec2 across = vec2(-along.y, along.x);
  vec4 p = mix(a, b, aCorner.x);
  float width = uWidth * mix(1.0, 0.55, aHourKind.y);
  vec2 offset = across * aCorner.y * width + along * (aCorner.x * 2.0 - 1.0) * width * 0.5;
  p.xy += offset / uViewport * p.w;
  gl_Position = p;
  vHour = aHourKind.x + aCorner.x;
  vSide = aCorner.y;
  vKind = aHourKind.y;
}`;

const FRAGMENT = `
in float vHour;
in float vSide;
in float vKind;
uniform float uHead;
uniform float uHours;
uniform float uOpacity;
uniform float uSoft;
out vec4 color;
void main() {
  if (vHour > uHead) discard;
  float age = clamp(vHour / max(uHours, 1.0), 0.0, 1.0);
  vec3 recent = vec3(1.00, 0.86, 0.60);
  vec3 middle = vec3(1.00, 0.52, 0.30);
  vec3 old = vec3(0.56, 0.42, 1.00);
  vec3 c = age < 0.45 ? mix(recent, middle, age / 0.45) : mix(middle, old, (age - 0.45) / 0.55);
  float edge = 1.0 - smoothstep(1.0 - uSoft, 1.0, abs(vSide));
  float tip = smoothstep(0.0, 6.0, uHead - vHour) * 0.25 + 0.75;
  float alpha = edge * uOpacity * mix(1.0, 0.20, vKind) * tip;
  color = vec4(c * alpha, alpha);
}`;

const FLOATS_PER_SEGMENT = 10;

export interface PathSet {
  /** Parcels traced together; parcel 0 is the main one. */
  parcels: number;
  rows: number;
  /** rows × parcels × 3: longitude, latitude, height above ground in metres. */
  points: Float32Array;
}

export class Paths {
  private readonly program: Program;
  private readonly vao: WebGLVertexArrayObject;
  private readonly instances: WebGLBuffer;
  private segments = 0;
  /** Hours revealed, counted back from the start. */
  head = 0;
  hours = 240;

  constructor(private readonly gl: WebGL2RenderingContext) {
    this.program = createProgram(gl, VERTEX, FRAGMENT, ["aCorner", "aStart", "aEnd", "aHourKind"]);
    this.vao = gl.createVertexArray() as WebGLVertexArrayObject;
    gl.bindVertexArray(this.vao);
    staticBuffer(gl, gl.ARRAY_BUFFER, new Float32Array([0, -1, 1, -1, 0, 1, 1, 1]));
    gl.enableVertexAttribArray(0);
    gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 0, 0);
    this.instances = gl.createBuffer() as WebGLBuffer;
    gl.bindBuffer(gl.ARRAY_BUFFER, this.instances);
    const stride = FLOATS_PER_SEGMENT * 4;
    for (const [slot, size, offset] of [
      [1, 4, 0],
      [2, 4, 16],
      [3, 2, 32],
    ] as const) {
      gl.enableVertexAttribArray(slot);
      gl.vertexAttribPointer(slot, size, gl.FLOAT, false, stride, offset);
      gl.vertexAttribDivisor(slot, 1);
    }
    gl.bindVertexArray(null);
  }

  /** Replaces the drawn paths. */
  set(paths: PathSet): void {
    const { parcels, rows, points } = paths;
    const data = new Float32Array(Math.max(0, rows - 1) * parcels * FLOATS_PER_SEGMENT);
    let count = 0;
    const vertex = (row: number, parcel: number, into: number): boolean => {
      const at = (row * parcels + parcel) * 3;
      const lon = points[at] as number;
      if (Number.isNaN(lon)) return false;
      const d = direction(lon, points[at + 1] as number);
      data.set([d[0], d[1], d[2], drawnRadius(points[at + 2] as number)], into);
      return true;
    };
    // Members first so the main path is drawn over them.
    for (let parcel = parcels - 1; parcel >= 0; parcel--) {
      for (let row = 0; row + 1 < rows; row++) {
        const into = count * FLOATS_PER_SEGMENT;
        if (!vertex(row, parcel, into) || !vertex(row + 1, parcel, into + 4)) continue;
        data[into + 8] = row;
        data[into + 9] = parcel === 0 ? 0 : 1;
        count++;
      }
    }
    this.segments = count;
    const gl = this.gl;
    gl.bindBuffer(gl.ARRAY_BUFFER, this.instances);
    gl.bufferData(gl.ARRAY_BUFFER, data.subarray(0, count * FLOATS_PER_SEGMENT), gl.DYNAMIC_DRAW);
  }

  clear(): void {
    this.segments = 0;
  }

  draw(viewProjection: Mat4, viewportPx: [number, number], pixelRatio: number): void {
    if (this.segments === 0) return;
    const gl = this.gl;
    const u = (name: string) => this.program.uniform(name);
    gl.useProgram(this.program.program);
    gl.bindVertexArray(this.vao);
    gl.uniformMatrix4fv(u("uViewProjection"), false, viewProjection);
    gl.uniform2f(u("uViewport"), viewportPx[0] / 2, viewportPx[1] / 2);
    gl.uniform1f(u("uHead"), this.head);
    gl.uniform1f(u("uHours"), this.hours);
    gl.disable(gl.CULL_FACE);
    gl.enable(gl.BLEND);
    gl.blendFunc(gl.ONE, gl.ONE_MINUS_SRC_ALPHA);
    gl.depthMask(false);
    const pass = (lift: number, width: number, opacity: number, soft: number) => {
      gl.uniform1f(u("uLift"), lift);
      gl.uniform1f(u("uWidth"), width * pixelRatio);
      gl.uniform1f(u("uOpacity"), opacity);
      gl.uniform1f(u("uSoft"), soft);
      gl.drawArraysInstanced(gl.TRIANGLE_STRIP, 0, 4, this.segments);
    };
    pass(0, 1.6, 0.16, 0.6); // the track on the ground beneath the air
    pass(1, 9, 0.1, 1.0); // glow
    pass(1, 1.7, 0.95, 0.5); // the path itself
    gl.depthMask(true);
    gl.bindVertexArray(null);
  }
}
