// PRV-009: WebGL2 renderer for RGBA frames (POC-013: ~0.4 ms per 720x1600 frame).
// The texture is re-allocated only when the frame size changes; otherwise texSubImage2D.

const VS = `#version 300 es
in vec2 p;
out vec2 uv;
void main() {
  uv = vec2((p.x + 1.0) * 0.5, 1.0 - (p.y + 1.0) * 0.5);
  gl_Position = vec4(p, 0.0, 1.0);
}`;

const FS = `#version 300 es
precision mediump float;
in vec2 uv;
uniform sampler2D t;
out vec4 color;
void main() { color = vec4(texture(t, uv).rgb, 1.0); }`;

function shader(gl: WebGL2RenderingContext, type: number, src: string): WebGLShader {
  const s = gl.createShader(type)!;
  gl.shaderSource(s, src);
  gl.compileShader(s);
  if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(s) ?? "shader");
  return s;
}

export class FrameRenderer {
  private gl: WebGL2RenderingContext;
  private tex: WebGLTexture;
  private w = 0;
  private h = 0;

  constructor(private canvas: HTMLCanvasElement) {
    const gl = canvas.getContext("webgl2", { antialias: false, alpha: false, desynchronized: true, preserveDrawingBuffer: false });
    if (!gl) throw new Error("WebGL2 is not available");
    this.gl = gl;
    const prog = gl.createProgram()!;
    gl.attachShader(prog, shader(gl, gl.VERTEX_SHADER, VS));
    gl.attachShader(prog, shader(gl, gl.FRAGMENT_SHADER, FS));
    gl.linkProgram(prog);
    gl.useProgram(prog);
    const buf = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, buf);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 1, -1, -1, 1, 1, 1]), gl.STATIC_DRAW);
    const loc = gl.getAttribLocation(prog, "p");
    gl.enableVertexAttribArray(loc);
    gl.vertexAttribPointer(loc, 2, gl.FLOAT, false, 0, 0);
    this.tex = gl.createTexture()!;
    gl.bindTexture(gl.TEXTURE_2D, this.tex);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
  }

  /** Where the frame is drawn inside the canvas (letterboxed), in canvas pixels. */
  contentRect(fw: number, fh: number): { x: number; y: number; w: number; h: number } {
    const cw = this.canvas.width;
    const ch = this.canvas.height;
    const s = Math.min(cw / fw, ch / fh);
    const w = Math.round(fw * s);
    const h = Math.round(fh * s);
    return { x: Math.round((cw - w) / 2), y: Math.round((ch - h) / 2), w, h };
  }

  private bpp = 0;

  /** Uploads and draws one frame (RGB or RGBA, from its length); returns the time it took (ms). */
  draw(px: Uint8Array, fw: number, fh: number): number {
    const t0 = performance.now();
    const gl = this.gl;
    const bpp = px.length >= fw * fh * 4 ? 4 : 3;
    const fmt = bpp === 4 ? gl.RGBA : gl.RGB;
    gl.bindTexture(gl.TEXTURE_2D, this.tex);
    if (fw !== this.w || fh !== this.h || bpp !== this.bpp) {
      gl.texImage2D(gl.TEXTURE_2D, 0, bpp === 4 ? gl.RGBA8 : gl.RGB8, fw, fh, 0, fmt, gl.UNSIGNED_BYTE, px);
      this.w = fw;
      this.h = fh;
      this.bpp = bpp;
    } else {
      gl.texSubImage2D(gl.TEXTURE_2D, 0, 0, 0, fw, fh, fmt, gl.UNSIGNED_BYTE, px);
    }
    const r = this.contentRect(fw, fh);
    gl.viewport(0, 0, this.canvas.width, this.canvas.height);
    gl.clearColor(0.06, 0.07, 0.09, 1);
    gl.clear(gl.COLOR_BUFFER_BIT);
    // WebGL's viewport origin is bottom-left
    gl.viewport(r.x, this.canvas.height - r.y - r.h, r.w, r.h);
    gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
    return performance.now() - t0;
  }
}

/** p50 / p95 of a sample list. */
export function percentiles(v: number[]): { p50: number; p95: number } {
  if (v.length === 0) return { p50: 0, p95: 0 };
  const s = [...v].sort((a, b) => a - b);
  const at = (q: number) => s[Math.min(s.length - 1, Math.round((s.length - 1) * q))];
  return { p50: at(0.5), p95: at(0.95) };
}
