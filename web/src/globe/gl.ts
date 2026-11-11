// WebGL2 helpers: compiling programs and looking up their inputs.

export interface Program {
  program: WebGLProgram;
  uniform(name: string): WebGLUniformLocation | null;
  attribute(name: string): number;
}

function compile(gl: WebGL2RenderingContext, type: number, source: string): WebGLShader {
  const shader = gl.createShader(type);
  if (!shader) throw new Error("The browser could not create a shader.");
  gl.shaderSource(shader, `#version 300 es\nprecision highp float;\n${source}`);
  gl.compileShader(shader);
  if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
    throw new Error(`Shader failed to compile: ${gl.getShaderInfoLog(shader)}`);
  }
  return shader;
}

/** Builds a program. `attributes` are given slots 0, 1, 2… in order. */
export function createProgram(
  gl: WebGL2RenderingContext,
  vertex: string,
  fragment: string,
  attributes: string[] = [],
): Program {
  const program = gl.createProgram();
  if (!program) throw new Error("The browser could not create a shader program.");
  gl.attachShader(program, compile(gl, gl.VERTEX_SHADER, vertex));
  gl.attachShader(program, compile(gl, gl.FRAGMENT_SHADER, fragment));
  for (const [slot, name] of attributes.entries()) gl.bindAttribLocation(program, slot, name);
  gl.linkProgram(program);
  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
    throw new Error(`Shader program failed to link: ${gl.getProgramInfoLog(program)}`);
  }
  const uniforms = new Map<string, WebGLUniformLocation | null>();
  return {
    program,
    uniform(name) {
      if (!uniforms.has(name)) uniforms.set(name, gl.getUniformLocation(program, name));
      return uniforms.get(name) ?? null;
    },
    attribute: (name) => gl.getAttribLocation(program, name),
  };
}

/** Uploads a static array and returns its buffer. */
export function staticBuffer(gl: WebGL2RenderingContext, target: number, data: AllowSharedBufferSource): WebGLBuffer {
  const buffer = gl.createBuffer();
  if (!buffer) throw new Error("The browser could not create a buffer.");
  gl.bindBuffer(target, buffer);
  gl.bufferData(target, data, gl.STATIC_DRAW);
  return buffer;
}
