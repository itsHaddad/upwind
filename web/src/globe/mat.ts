// The little linear algebra the globe needs. Matrices are column-major.

export type Vec3 = [number, number, number];
export type Mat4 = Float32Array;

const RAD = Math.PI / 180;

/** Unit vector of a point on the globe: +y is north, +z faces 0°E. */
export function direction(lonDeg: number, latDeg: number): Vec3 {
  const lon = lonDeg * RAD;
  const lat = latDeg * RAD;
  return [Math.cos(lat) * Math.sin(lon), Math.sin(lat), Math.cos(lat) * Math.cos(lon)];
}

/** Longitude and latitude in degrees of a direction. */
export function lonLat(v: Vec3): [number, number] {
  const length = Math.hypot(v[0], v[1], v[2]) || 1;
  return [Math.atan2(v[0], v[2]) / RAD, Math.asin(Math.max(-1, Math.min(1, v[1] / length))) / RAD];
}

export function perspective(fovYDeg: number, aspect: number, near: number, far: number): Mat4 {
  const f = 1 / Math.tan((fovYDeg * RAD) / 2);
  const m = new Float32Array(16);
  m[0] = f / aspect;
  m[5] = f;
  m[10] = (far + near) / (near - far);
  m[11] = -1;
  m[14] = (2 * far * near) / (near - far);
  return m;
}

function normalize(v: Vec3): Vec3 {
  const length = Math.hypot(v[0], v[1], v[2]) || 1;
  return [v[0] / length, v[1] / length, v[2] / length];
}

function cross(a: Vec3, b: Vec3): Vec3 {
  return [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
}

function dot(a: Vec3, b: Vec3): number {
  return a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
}

/** View matrix of a camera at `eye` looking at the origin with +y up. */
export function lookAtOrigin(eye: Vec3): Mat4 {
  const back = normalize(eye);
  const right = normalize(cross([0, 1, 0], back));
  const up = cross(back, right);
  const m = new Float32Array(16);
  m.set([right[0], up[0], back[0], 0, right[1], up[1], back[1], 0, right[2], up[2], back[2], 0]);
  m[12] = -dot(right, eye);
  m[13] = -dot(up, eye);
  m[14] = -dot(back, eye);
  m[15] = 1;
  return m;
}

export function multiply(a: Mat4, b: Mat4): Mat4 {
  const m = new Float32Array(16);
  for (let column = 0; column < 4; column++) {
    for (let row = 0; row < 4; row++) {
      let sum = 0;
      for (let k = 0; k < 4; k++) sum += (a[k * 4 + row] as number) * (b[column * 4 + k] as number);
      m[column * 4 + row] = sum;
    }
  }
  return m;
}

/** Shifts the picture in clip space; `y` of 0.2 moves it up a tenth of the screen. */
export function shifted(projection: Mat4, x: number, y: number): Mat4 {
  const shift = new Float32Array([1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, x, y, 0, 1]);
  return multiply(shift, projection);
}

/** Transforms a point; returns clip coordinates `[x, y, z, w]`. */
export function transform(m: Mat4, p: Vec3): [number, number, number, number] {
  const at = (row: number) =>
    (m[row] as number) * p[0] + (m[4 + row] as number) * p[1] + (m[8 + row] as number) * p[2] + (m[12 + row] as number);
  return [at(0), at(1), at(2), at(3)];
}

/** Shortest signed difference between two longitudes, in degrees. */
export function lonDelta(from: number, to: number): number {
  return ((((to - from) % 360) + 540) % 360) - 180;
}
