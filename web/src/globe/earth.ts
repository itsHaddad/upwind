// The Earth itself: a sphere textured with land, coasts and borders, and the
// glow of the atmosphere around it.

import type { FeatureCollection, MultiLineString, MultiPolygon, Polygon, Position } from "geojson";
import { feature, mesh } from "topojson-client";
import type { GeometryCollection, Topology } from "topojson-specification";
import { createProgram, type Program, staticBuffer } from "./gl";
import type { Mat4, Vec3 } from "./mat";

type Atlas = Topology<{ countries: GeometryCollection; land: GeometryCollection }>;

const SPHERE_VERTEX = `
in vec3 aPosition;
in vec2 aUv;
uniform mat4 uViewProjection;
uniform float uRadius;
out vec3 vNormal;
out vec2 vUv;
void main() {
  vNormal = aPosition;
  vUv = aUv;
  gl_Position = uViewProjection * vec4(aPosition * uRadius, 1.0);
}`;

// The texture holds land in red, country borders in green, coasts in blue.
const EARTH_FRAGMENT = `
in vec3 vNormal;
in vec2 vUv;
uniform sampler2D uMap;
uniform vec3 uEye;
out vec4 color;
float gridLine(float degrees, float every) {
  float d = abs(fract(degrees / every + 0.5) - 0.5) * every;
  return 1.0 - smoothstep(0.0, fwidth(degrees) * 1.2, d);
}
void main() {
  vec3 n = normalize(vNormal);
  vec3 map = texture(uMap, vUv).rgb;
  vec3 ocean = vec3(0.020, 0.043, 0.082);
  vec3 land = vec3(0.071, 0.106, 0.157);
  vec3 c = mix(ocean, land, map.r);
  c += vec3(0.10, 0.17, 0.26) * map.g * 0.55;
  c += vec3(0.20, 0.38, 0.58) * map.b * 0.55;
  float lat = (0.5 - vUv.y) * 180.0;
  // Meridians crowd together at the poles; let them fade out before they do.
  float meridians = gridLine(vUv.x * 360.0, 30.0) * (1.0 - smoothstep(60.0, 80.0, abs(lat)));
  float lines = max(meridians, gridLine(lat, 30.0) * step(abs(lat), 85.0));
  c += vec3(0.16, 0.26, 0.40) * lines * 0.16;
  float facing = max(dot(n, normalize(uEye - n)), 0.0);
  c *= 0.42 + 0.58 * facing;
  c += vec3(0.14, 0.32, 0.60) * pow(1.0 - facing, 4.5) * 0.5;
  color = vec4(c, 1.0);
}`;

// Drawn on the inside of a larger sphere, so it shows only beyond the limb.
// It fades to transparent, leaving the page background visible around it.
const GLOW_FRAGMENT = `
in vec3 vNormal;
uniform vec3 uEye;
uniform float uRadius;
out vec4 color;
void main() {
  vec3 n = normalize(vNormal);
  float d = -dot(n, normalize(uEye - n * uRadius));
  float limb = sqrt(1.0 - 1.0 / (uRadius * uRadius));
  float glow = pow(clamp(d / limb, 0.0, 1.0), 4.0) * 0.7;
  color = vec4(vec3(0.24, 0.50, 0.92) * glow, glow);
}`;

const SEGMENTS_LAT = 72;
const SEGMENTS_LON = 144;
const GLOW_RADIUS = 1.16;

function sphereMesh(): { vertices: Float32Array; indices: Uint16Array } {
  const vertices: number[] = [];
  const indices: number[] = [];
  for (let j = 0; j <= SEGMENTS_LAT; j++) {
    const v = j / SEGMENTS_LAT;
    const lat = (0.5 - v) * Math.PI;
    for (let i = 0; i <= SEGMENTS_LON; i++) {
      const u = i / SEGMENTS_LON;
      const lon = (u - 0.5) * 2 * Math.PI;
      vertices.push(Math.cos(lat) * Math.sin(lon), Math.sin(lat), Math.cos(lat) * Math.cos(lon), u, v);
    }
  }
  const row = SEGMENTS_LON + 1;
  for (let j = 0; j < SEGMENTS_LAT; j++) {
    for (let i = 0; i < SEGMENTS_LON; i++) {
      const a = j * row + i;
      indices.push(a, a + row, a + 1, a + 1, a + row, a + row + 1);
    }
  }
  return { vertices: new Float32Array(vertices), indices: new Uint16Array(indices) };
}

/** Paints land, borders and coasts into an equirectangular canvas. */
export function paintMap(atlas: Atlas, width: number): HTMLCanvasElement {
  const canvas = document.createElement("canvas");
  canvas.width = width;
  canvas.height = width / 2;
  const context = canvas.getContext("2d");
  if (!context) throw new Error("The browser could not create a drawing canvas.");
  const x = (lon: number) => ((lon + 180) / 360) * canvas.width;
  const y = (lat: number) => ((90 - lat) / 180) * canvas.height;
  const trace = (line: Position[], close: boolean) => {
    for (const [index, point] of line.entries()) {
      const previous = line[index - 1];
      // Lines that jump across the map are the antimeridian; do not draw them.
      const jump = previous !== undefined && Math.abs((point[0] as number) - (previous[0] as number)) > 180;
      if (index === 0 || jump) context.moveTo(x(point[0] as number), y(point[1] as number));
      else context.lineTo(x(point[0] as number), y(point[1] as number));
    }
    if (close) context.closePath();
  };

  context.fillStyle = "#000";
  context.fillRect(0, 0, canvas.width, canvas.height);
  context.globalCompositeOperation = "lighter";
  context.lineJoin = "round";

  const land = feature(atlas, atlas.objects.land) as FeatureCollection<Polygon | MultiPolygon>;
  const polygons = land.features.flatMap((f) =>
    f.geometry.type === "Polygon" ? [f.geometry.coordinates] : f.geometry.coordinates,
  );
  context.beginPath();
  for (const rings of polygons) for (const ring of rings) trace(ring, true);
  context.fillStyle = "#f00";
  context.fill("evenodd");

  const scale = width / 4096;
  const stroke = (lines: MultiLineString, style: string, lineWidth: number) => {
    context.beginPath();
    for (const line of lines.coordinates) trace(line, false);
    context.strokeStyle = style;
    context.lineWidth = lineWidth * scale;
    context.stroke();
  };
  stroke(
    mesh(atlas, atlas.objects.countries, (a, b) => a !== b),
    "#0f0",
    1.6,
  );
  stroke(
    mesh(atlas, atlas.objects.countries, (a, b) => a === b),
    "#00f",
    1.7,
  );
  return canvas;
}

export class Earth {
  private readonly surface: Program;
  private readonly glow: Program;
  private readonly vao: WebGLVertexArrayObject;
  private readonly indexCount: number;
  private readonly texture: WebGLTexture;

  constructor(private readonly gl: WebGL2RenderingContext) {
    this.surface = createProgram(gl, SPHERE_VERTEX, EARTH_FRAGMENT, ["aPosition", "aUv"]);
    this.glow = createProgram(gl, SPHERE_VERTEX, GLOW_FRAGMENT, ["aPosition", "aUv"]);
    const { vertices, indices } = sphereMesh();
    this.indexCount = indices.length;
    this.vao = gl.createVertexArray() as WebGLVertexArrayObject;
    gl.bindVertexArray(this.vao);
    staticBuffer(gl, gl.ARRAY_BUFFER, vertices);
    gl.enableVertexAttribArray(0);
    gl.vertexAttribPointer(0, 3, gl.FLOAT, false, 20, 0);
    gl.enableVertexAttribArray(1);
    gl.vertexAttribPointer(1, 2, gl.FLOAT, false, 20, 12);
    staticBuffer(gl, gl.ELEMENT_ARRAY_BUFFER, indices);
    gl.bindVertexArray(null);

    this.texture = gl.createTexture() as WebGLTexture;
    gl.bindTexture(gl.TEXTURE_2D, this.texture);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, 1, 1, 0, gl.RGBA, gl.UNSIGNED_BYTE, new Uint8Array([0, 0, 0, 255]));
  }

  /** Replaces the map texture. */
  setMap(map: TexImageSource): void {
    const gl = this.gl;
    gl.bindTexture(gl.TEXTURE_2D, this.texture);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, map);
    gl.generateMipmap(gl.TEXTURE_2D);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR_MIPMAP_LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.REPEAT);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    const anisotropy = gl.getExtension("EXT_texture_filter_anisotropic");
    if (anisotropy) {
      const most = gl.getParameter(anisotropy.MAX_TEXTURE_MAX_ANISOTROPY_EXT) as number;
      gl.texParameterf(gl.TEXTURE_2D, anisotropy.TEXTURE_MAX_ANISOTROPY_EXT, Math.min(8, most));
    }
  }

  draw(viewProjection: Mat4, eye: Vec3): void {
    const gl = this.gl;
    gl.bindVertexArray(this.vao);
    gl.enable(gl.CULL_FACE);

    // The glow: inside faces of a larger sphere, blended over the background.
    gl.useProgram(this.glow.program);
    gl.uniformMatrix4fv(this.glow.uniform("uViewProjection"), false, viewProjection);
    gl.uniform3fv(this.glow.uniform("uEye"), eye);
    gl.uniform1f(this.glow.uniform("uRadius"), GLOW_RADIUS);
    gl.cullFace(gl.FRONT);
    gl.depthMask(false);
    gl.enable(gl.BLEND);
    gl.blendFunc(gl.ONE, gl.ONE_MINUS_SRC_ALPHA);
    gl.drawElements(gl.TRIANGLES, this.indexCount, gl.UNSIGNED_SHORT, 0);

    gl.useProgram(this.surface.program);
    gl.uniformMatrix4fv(this.surface.uniform("uViewProjection"), false, viewProjection);
    gl.uniform3fv(this.surface.uniform("uEye"), eye);
    gl.uniform1f(this.surface.uniform("uRadius"), 1);
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, this.texture);
    gl.uniform1i(this.surface.uniform("uMap"), 0);
    gl.cullFace(gl.BACK);
    gl.depthMask(true);
    gl.disable(gl.BLEND);
    gl.drawElements(gl.TRIANGLES, this.indexCount, gl.UNSIGNED_SHORT, 0);
    gl.bindVertexArray(null);
  }
}
