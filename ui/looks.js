// Previews of the looks: the same effects as src-tauri/looks/*.fx, written again for WebGL, drawn on
// the game's own picture. One WebGL context draws everything; each preview is copied into its own
// ordinary canvas.
(() => {
  'use strict';
  let gl = null, glCanvas = null, half = false, broken = false;
  const progs = {};
  let DEFS = null;
  let failed = false;
  // Read from the window's own files, or through the core when the window isn't allowed to.
  const viaCore = () => window.__TAURI__.core.invoke('ui_data', { name: 'looks' });
  const defsReady = fetch('looks.json').then((r) => (r.ok ? r.json() : Promise.reject(r.status))).catch(viaCore).catch(viaCore)
    .then((d) => { if (!d || !Array.isArray(d.looks)) throw new Error('no looks'); return (DEFS = d); })
    .catch(() => { failed = true; return null; });

  const VS = `#version 300 es
uniform float uFlip;
out vec2 uv;
void main() {
  uv = vec2(gl_VertexID == 2 ? 2.0 : 0.0, gl_VertexID == 1 ? 2.0 : 0.0);
  vec2 p = uv * vec2(2.0, -2.0) + vec2(-1.0, 1.0);
  gl_Position = vec4(p.x, p.y * uFlip, 0.0, 1.0);
}`;

  const HEAD = `#version 300 es
precision highp float;
precision highp int;
in vec2 uv;
out vec4 outColor;
uniform sampler2D uBack;
uniform vec2 uSize;
const vec3 LUMA = vec3(0.2126, 0.7152, 0.0722);
vec3 fetchB(vec2 p) { return textureLod(uBack, p, 0.0).rgb; }
vec3 toLin(vec3 c) { return pow(max(c, 0.0), vec3(2.2)); }
vec3 toGam(vec3 c) { return pow(max(c, 0.0), vec3(1.0 / 2.2)); }
vec3 softClip(vec3 c) { vec3 o = max(c - 0.8, 0.0); return min(c, 0.8) + o / (1.0 + o * 5.0); }
float hash(vec2 p) { vec3 p3 = fract(vec3(p.xyx) * 0.1031); p3 += dot(p3, p3.yzx + 33.33); return fract((p3.x + p3.y) * p3.z); }
`;

  const FS = {
    copy: HEAD + `void main() { outColor = vec4(fetchB(uv), 1.0); }`,
    sharp: HEAD + `
uniform float Sharpness, Detail;
void main() {
  vec2 px = 1.0 / uSize;
  vec3 e = fetchB(uv);
  vec3 a = fetchB(uv + vec2(-px.x, -px.y)), b = fetchB(uv + vec2(0.0, -px.y)), c = fetchB(uv + vec2(px.x, -px.y));
  vec3 d = fetchB(uv + vec2(-px.x, 0.0)), f = fetchB(uv + vec2(px.x, 0.0));
  vec3 g = fetchB(uv + vec2(-px.x, px.y)), h = fetchB(uv + vec2(0.0, px.y)), i = fetchB(uv + vec2(px.x, px.y));
  vec3 mn = min(min(min(d, e), min(f, b)), h);
  vec3 mn2 = min(mn, min(min(a, c), min(g, i)));
  mn += mn2;
  vec3 mx = max(max(max(d, e), max(f, b)), h);
  vec3 mx2 = max(mx, max(max(a, c), max(g, i)));
  mx += mx2;
  vec3 amp = clamp(min(mn, 2.0 - mx) / max(mx, 1e-5), 0.0, 1.0);
  amp = inversesqrt(max(amp, 1e-5));
  float peak = mix(8.0, 5.0, Detail);
  vec3 w = -1.0 / (amp * peak);
  vec3 sharp = clamp(((b + d + f + h) * w + e) / (4.0 * w + 1.0), 0.0, 1.0);
  outColor = vec4(mix(e, sharp, Sharpness), 1.0);
}`,
    vivid: HEAD + `
uniform float Vibrance, Contrast, Blacks;
void main() {
  vec3 c = fetchB(uv);
  c = clamp((c - Blacks) / (1.0 - Blacks), 0.0, 1.0);
  vec3 s = c * c * (3.0 - 2.0 * c);
  c = mix(c, s, Contrast);
  float luma = dot(c, LUMA);
  float sat = max(max(c.r, c.g), c.b) - min(min(c.r, c.g), c.b);
  float gain = 1.0 + Vibrance * (1.0 - sat) * 1.4;
  outColor = vec4(clamp(mix(vec3(luma), c, gain), 0.0, 1.0), 1.0);
}`,
    film: HEAD + `
uniform float Tone, Grain, GrainSize, Vignette;
uniform float uFrame;
void main() {
  vec3 c = fetchB(uv);
  float luma = dot(c, LUMA);
  float k = 0.3 * Tone;
  c = c * (1.0 + k) / (1.0 + c * k);
  c = c * (1.0 - 0.035 * Tone) + 0.035 * Tone;
  vec3 shadows = vec3(-0.025, 0.006, 0.03), highs = vec3(0.035, 0.012, -0.03);
  float hw = smoothstep(0.35, 0.9, luma), sw = 1.0 - smoothstep(0.05, 0.5, luma);
  c += (shadows * sw + highs * hw) * Tone * 1.6;
  c = mix(vec3(dot(c, LUMA)), c, 1.0 - 0.12 * Tone);
  vec2 d = (uv - 0.5) * vec2(1.0, uSize.y / uSize.x) * 1.6;
  float v = clamp(1.0 - dot(d, d) * 0.6, 0.0, 1.0);
  c *= mix(1.0, v * v * (3.0 - 2.0 * v), Vignette);
  vec2 cell = floor(gl_FragCoord.xy / GrainSize);
  float n = hash(cell + mod(uFrame, 1024.0) * 17.13) + hash(cell * 1.7 + mod(uFrame, 997.0) * 3.71) - 1.0;
  float mid = 1.0 - abs(luma * 2.0 - 1.0);
  c += n * Grain * (0.35 + 0.65 * mid);
  outColor = vec4(clamp(c, 0.0, 1.0), 1.0);
}`,
    crtDown: HEAD + `
uniform vec2 uLow;
uniform int uTaps;
void main() {
  vec3 sum = vec3(0.0);
  for (int y = 0; y < 6; y++) {
    if (y >= uTaps) break;
    for (int x = 0; x < 6; x++) {
      if (x >= uTaps) break;
      vec2 o = (vec2(float(x), float(y)) + 0.5) / float(uTaps) - 0.5;
      sum += toLin(fetchB(uv + o / uLow));
    }
  }
  outColor = vec4(sum / float(uTaps * uTaps), 1.0);
}`,
    crtBlur: HEAD + `
uniform vec2 uStep;
void main() {
  float w[5] = float[5](0.2270270, 0.1945946, 0.1216216, 0.0540541, 0.0162162);
  vec3 c = textureLod(uBack, uv, 0.0).rgb * w[0];
  for (int i = 1; i < 5; i++) {
    c += textureLod(uBack, uv + uStep * float(i), 0.0).rgb * w[i];
    c += textureLod(uBack, uv - uStep * float(i), 0.0).rgb * w[i];
  }
  outColor = vec4(c, 1.0);
}`,
    crt: HEAD + `
uniform sampler2D uLowTex, uGlow;
uniform vec2 uLow;
uniform float uMaskPx;
uniform float Curvature, Scanlines, Beam, Mask, Glow, Sharpness, Vignette, Corner, Brightness;
uniform int MaskType;
vec2 warp(vec2 p) {
  vec2 c = p * 2.0 - 1.0;
  vec2 k = Curvature * vec2(1.0 / 24.0, 1.0 / 16.0);
  c *= 1.0 + c.yx * c.yx * k;
  return c * 0.5 + 0.5;
}
vec3 texel(ivec2 at) {
  at = clamp(at, ivec2(0), ivec2(uLow) - 1);
  return texelFetch(uLowTex, at, 0).rgb;
}
vec3 line(int row, float x) {
  float px = x - 0.5;
  int i = int(floor(px));
  float f = px - floor(px);
  float hard = mix(1.6, 5.0, Sharpness);
  vec3 c = vec3(0.0);
  float total = 0.0;
  for (int k = -1; k <= 2; k++) {
    float d = float(k) - f;
    float w = exp(-hard * d * d);
    c += texel(ivec2(i + k, row)) * w;
    total += w;
  }
  return c / total;
}
vec3 beamOf(vec3 c, float d) {
  float lum = sqrt(max(dot(c, LUMA), 0.0));
  float thin = mix(0.14, 0.22, Beam), wide = mix(0.28, 0.42, Beam);
  float s = mix(thin, wide, clamp(lum, 0.0, 1.0));
  return c * (exp(-0.5 * d * d / (s * s)) / (s * 2.5066283));
}
vec3 maskAt(vec2 p) {
  int x = int(floor(p.x / uMaskPx)), y = int(floor(p.y / uMaskPx));
  float dark = 1.0 - 0.7 * Mask;
  vec3 m = vec3(dark);
  int sub = MaskType == 2 ? ((x + (y % 2) * 3) % 6) / 2 : x % 3;
  if (sub == 0) m.r = 1.0; else if (sub == 1) m.g = 1.0; else m.b = 1.0;
  if (MaskType == 1) { int triad = x / 3; if ((y + (triad % 2) * 2) % 4 == 0) m *= dark; }
  float kept = (1.0 + 2.0 * dark) / 3.0;
  if (MaskType == 1) kept *= (3.0 + dark) / 4.0;
  return m * mix(1.0, 1.0 / kept, 0.8);
}
void main() {
  vec2 w = warp(uv);
  float aspect = uSize.x / uSize.y;
  vec2 q = abs(w - 0.5) * 2.0 * vec2(aspect, 1.0);
  float r = 0.12 * Corner;
  vec2 lim = vec2(aspect, 1.0) - r;
  float edge = length(max(q - lim, 0.0)) - r;
  float inside = 1.0 - smoothstep(-0.004, 0.004, edge);
  if (inside <= 0.0) { outColor = vec4(0.0, 0.0, 0.0, 1.0); return; }
  vec2 p = w * uLow;
  float y = p.y - 0.5;
  int row = int(floor(y));
  float f = y - floor(y);
  vec3 a = line(row, p.x), b = line(row + 1, p.x);
  vec3 flat_ = mix(a, b, f);
  vec3 scan = beamOf(a, f) + beamOf(b, 1.0 - f);
  vec3 c = mix(flat_, scan, Scanlines);
  // Screen pixels counted from the top, like Direct3D.
  vec2 pos = vec2(gl_FragCoord.x, uSize.y - gl_FragCoord.y);
  if (Mask > 0.0) c *= maskAt(pos);
  vec3 glow = textureLod(uGlow, w, 0.0).rgb;
  c = c * (1.0 - 0.2 * Glow) + glow * Glow * 0.6;
  vec2 v = (w - 0.5) * vec2(aspect, 1.0) * 1.3;
  c *= mix(1.0, clamp(1.0 - dot(v, v) * 0.5, 0.0, 1.0), Vignette);
  c *= Brightness * inside;
  outColor = vec4(toGam(softClip(c)), 1.0);
}`,
  };

  function init() {
    if (gl || broken) return !!gl;
    glCanvas = document.createElement('canvas');
    gl = glCanvas.getContext('webgl2', { preserveDrawingBuffer: true, premultipliedAlpha: false, antialias: false });
    if (!gl) { broken = true; return false; }
    half = !!gl.getExtension('EXT_color_buffer_float');
    glCanvas.addEventListener('webglcontextlost', (e) => { e.preventDefault(); gl = null; for (const k in progs) delete progs[k]; cache.clear(); });
    return true;
  }
  function prog(name) {
    if (progs[name]) return progs[name];
    const mk = (type, src) => { const s = gl.createShader(type); gl.shaderSource(s, src); gl.compileShader(s); if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) throw new Error(name + ': ' + gl.getShaderInfoLog(s)); return s; };
    const p = gl.createProgram();
    gl.attachShader(p, mk(gl.VERTEX_SHADER, VS));
    gl.attachShader(p, mk(gl.FRAGMENT_SHADER, FS[name]));
    gl.linkProgram(p);
    if (!gl.getProgramParameter(p, gl.LINK_STATUS)) throw new Error(name + ': ' + gl.getProgramInfoLog(p));
    const u = {};
    const n = gl.getProgramParameter(p, gl.ACTIVE_UNIFORMS);
    for (let i = 0; i < n; i++) { const info = gl.getActiveUniform(p, i); u[info.name.replace(/\[0\]$/, '')] = { loc: gl.getUniformLocation(p, info.name), type: info.type }; }
    return (progs[name] = { p, u });
  }
  function tex(w, h, float, filter) {
    const tx = gl.createTexture();
    gl.bindTexture(gl.TEXTURE_2D, tx);
    if (float && half) gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA16F, w, h, 0, gl.RGBA, gl.HALF_FLOAT, null);
    else gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA8, w, h, 0, gl.RGBA, gl.UNSIGNED_BYTE, null);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, filter || gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, filter || gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    return tx;
  }
  // Run one pass: program, its textures and settings, into a texture (or the canvas when `to` is null).
  function pass(name, to, w, h, textures, vals) {
    const { p, u } = prog(name);
    gl.useProgram(p);
    let fb = null;
    if (to) { fb = gl.createFramebuffer(); gl.bindFramebuffer(gl.FRAMEBUFFER, fb); gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, to, 0); }
    else gl.bindFramebuffer(gl.FRAMEBUFFER, null);
    gl.viewport(0, 0, w, h);
    let unit = 0;
    for (const [k, tx] of Object.entries(textures)) { if (!u[k]) continue; gl.activeTexture(gl.TEXTURE0 + unit); gl.bindTexture(gl.TEXTURE_2D, tx); gl.uniform1i(u[k].loc, unit); unit++; }
    const all = Object.assign({ uFlip: to ? -1 : 1, uSize: [w, h] }, vals);
    for (const [k, v] of Object.entries(all)) {
      const x = u[k];
      if (!x) continue;
      if (Array.isArray(v)) gl.uniform2f(x.loc, v[0], v[1]);
      else if (x.type === gl.INT) gl.uniform1i(x.loc, Math.round(v));
      else gl.uniform1f(x.loc, v);
    }
    gl.drawArrays(gl.TRIANGLES, 0, 3);
    if (fb) gl.deleteFramebuffer(fb);
  }

  // A picture, cropped to fill w x h, as a texture.
  const cache = new Map();
  function source(img, w, h, zoom) {
    const key = img.src + '|' + w + 'x' + h + '|' + zoom;
    if (cache.has(key)) return cache.get(key);
    const c = document.createElement('canvas');
    c.width = w; c.height = h;
    const x = c.getContext('2d');
    const s = Math.max(w / img.naturalWidth, h / img.naturalHeight) * zoom;
    const dw = img.naturalWidth * s, dh = img.naturalHeight * s;
    x.imageSmoothingQuality = 'high';
    x.drawImage(img, (w - dw) / 2, (h - dh) / 2, dw, dh);
    const tx = tex(w, h, false);
    gl.bindTexture(gl.TEXTURE_2D, tx);
    gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, false);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA8, gl.RGBA, gl.UNSIGNED_BYTE, c);
    if (cache.size > 24) { for (const [k, v] of cache) { gl.deleteTexture(v); cache.delete(k); break; } }
    cache.set(key, tx);
    return tx;
  }

  function values(look, amount) {
    const f = Math.max(0, Math.min(100, amount)) / 100;
    const v = Object.assign({}, look.fixed);
    for (const [k, [a, b]] of Object.entries(look.amount)) v[k] = a + (b - a) * f;
    return v;
  }

  let frame = 0;
  // Draw `img` with a look into the 2D canvas `out` (its own pixel size). lookId '' draws the picture
  // as it is. `zoom` above 1 shows the middle of the picture, closer.
  function draw(out, img, lookId, amount, lines, zoom) {
    if (!img || !img.naturalWidth || !init() || !DEFS) return false;
    const w = out.width, h = out.height;
    if (glCanvas.width !== w || glCanvas.height !== h) { glCanvas.width = w; glCanvas.height = h; }
    const look = DEFS.looks.find((l) => l.id === lookId);
    try {
      const back = source(img, w, h, zoom || 1);
      if (!look) pass('copy', null, w, h, { uBack: back }, {});
      else if (look.fx === 'RL_CRT.fx') {
        const L = lines || DEFS.default_lines;
        const RH = L * 2 > h ? Math.floor(h / 2) : L, RW = Math.floor(RH * w / h);
        const taps = Math.max(1, Math.min(6, Math.floor(h / RH)));
        const low = tex(RW, RH, true), ga = tex(RW >> 1, RH >> 1, true), gb = tex(RW >> 1, RH >> 1, true);
        pass('crtDown', low, RW, RH, { uBack: back }, { uLow: [RW, RH], uTaps: taps });
        pass('crtBlur', ga, RW >> 1, RH >> 1, { uBack: low }, { uStep: [2 / RW, 0] });
        pass('crtBlur', gb, RW >> 1, RH >> 1, { uBack: ga }, { uStep: [0, 2 / RH] });
        pass('crt', null, w, h, { uLowTex: low, uGlow: gb }, Object.assign({ uLow: [RW, RH], uMaskPx: Math.max(1, Math.round(h / 1080)) }, values(look, amount)));
        gl.deleteTexture(low); gl.deleteTexture(ga); gl.deleteTexture(gb);
      } else {
        pass(look.fx.replace(/^RL_|\.fx$/g, '').toLowerCase(), null, w, h, { uBack: back }, Object.assign({ uFrame: ++frame }, values(look, amount)));
      }
    } catch (e) {
      console.error(e);
      broken = true;
      return false;
    }
    const x = out.getContext('2d');
    x.drawImage(glCanvas, 0, 0);
    return true;
  }

  window.Looks = { ready: defsReady, draw, defs: () => DEFS, failed: () => failed, supported: () => init() };
})();
