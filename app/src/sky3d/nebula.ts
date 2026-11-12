// Nebulae with structure (#297, pass 3). Each project's gas is its own procedural texture, baked
// once into one atlas from seeded, domain-warped fBm: wispy filaments, irregular silhouettes,
// dust lanes and globules cutting across the glow, bright rims where gas faces its stars. The
// texture's type and how much gas is left come from the seed and the project's progress. Each
// nebula is then drawn as 3 to 5 camera-facing slices at different depths (volume and parallax),
// in two instanced draws: dust that absorbs the light behind it, then the glowing gas.
import * as THREE from "three";
import type { System } from "./milkyway";
import { NEBULA_TYPES, nebulaStyle } from "./milkyway";
import { nebulaStage } from "./optics";
import { seeded } from "./random";

/** Slots per atlas side and each slot's size in texels. */
const SLOTS = 8;
const SLOT_PX = 320;

const BAKE_VS = /* glsl */ `
varying vec2 vUv;
void main() {
  vUv = uv;
  gl_Position = vec4(position.xy * 2.0, 0.0, 1.0);
}`;

// One draw bakes every slot: the fragment finds its slot and reads that nebula's parameters.
// Output: R hydrogen-ish emission, G the second gas, B dust density, A bright rims and filaments.
const BAKE_FS = /* glsl */ `
precision highp float;
uniform vec4 uA[${SLOTS * SLOTS}];
uniform vec4 uB[${SLOTS * SLOTS}];
varying vec2 vUv;

vec2 hash(vec2 p) {
  p = vec2(dot(p, vec2(127.1, 311.7)), dot(p, vec2(269.5, 183.3)));
  return -1.0 + 2.0 * fract(sin(p) * 43758.5453123);
}
float noise(vec2 p) {
  vec2 i = floor(p);
  vec2 f = fract(p);
  vec2 u = f * f * (3.0 - 2.0 * f);
  return mix(mix(dot(hash(i), f), dot(hash(i + vec2(1.0, 0.0)), f - vec2(1.0, 0.0)), u.x),
             mix(dot(hash(i + vec2(0.0, 1.0)), f - vec2(0.0, 1.0)), dot(hash(i + vec2(1.0, 1.0)), f - vec2(1.0, 1.0)), u.x), u.y);
}
const mat2 ROT = mat2(0.8, 0.6, -0.6, 0.8);
float fbm(vec2 p) {
  float a = 0.5;
  float s = 0.0;
  for (int i = 0; i < 6; i++) {
    s += a * noise(p);
    p = ROT * p * 2.03 + vec2(1.7, 9.2);
    a *= 0.5;
  }
  return s;
}
// Ridged noise: thin bright threads where the noise crosses zero.
float ridge(vec2 p) {
  float a = 0.5;
  float s = 0.0;
  for (int i = 0; i < 5; i++) {
    float n = 1.0 - abs(noise(p) * 1.8);
    s += a * n * n;
    p = ROT * p * 2.1 + vec2(3.1, 1.3);
    a *= 0.52;
  }
  return s;
}

void main() {
  vec2 cell = floor(vUv * ${SLOTS}.0);
  int idx = int(cell.y) * ${SLOTS} + int(cell.x);
  vec4 A = uA[idx];
  vec4 B = uB[idx];
  if (A.y < 0.0) {
    gl_FragColor = vec4(0.0);
    return;
  }
  vec2 uv = fract(vUv * ${SLOTS}.0) * 2.0 - 1.0;
  float seed = A.x;
  float type = A.y;
  float gas = A.z;
  float cav = A.w;
  float aspect = B.x;

  vec2 o = vec2(seed * 173.0, seed * 311.0);
  vec2 p = uv * 1.7 + o;
  // Domain warping, twice: the gas folds into itself instead of falling off in circles.
  vec2 q = vec2(fbm(p), fbm(p + vec2(5.2, 1.3)));
  vec2 r = vec2(fbm(p + 3.0 * q + vec2(1.7, 9.2)), fbm(p + 3.0 * q + vec2(8.3, 2.8)));
  float f = fbm(p + 3.5 * r);
  vec2 w = (uv + 0.55 * r) * vec2(1.0, 1.0 / aspect);
  float rad = length(w);
  // An irregular silhouette: the noise itself decides where the cloud ends, not a circle.
  float edge = 1.0 - smoothstep(0.25, 0.8, rad + 0.75 * f - 0.08);
  float dens = pow(clamp(f * 2.0 + 0.5, 0.0, 1.0), 1.6) * edge;
  float fil = ridge(p * 1.25 + 2.2 * r);

  float em = 0.0;
  float em2 = 0.0;
  float dust = 0.0;
  float rim = 0.0;
  if (type < 0.5) {
    // Emission cloud: thick glowing gas, oxygen near the hot heart, dark lanes across it.
    em = pow(dens, 1.4) * (0.55 + 0.6 * fil * fil);
    em2 = em * smoothstep(0.75, 0.05, rad) * 0.8;
    dust = smoothstep(0.62, 0.9, ridge(p * 0.8 + r * 1.6 + 11.0)) * edge;
    rim = pow(fil, 5.0) * dens * 0.9;
  } else if (type < 1.5) {
    // Reflection nebula: soft, streaky dust lit blue by nearby hot stars.
    float streak = fbm(vec2(p.x * 0.6 + r.x * 2.0, p.y * 2.2 + r.y));
    em = pow(clamp(streak * 1.6 + 0.5, 0.0, 1.0), 2.6) * edge * (0.5 + 0.7 * fil);
    em2 = em * smoothstep(0.6, 0.0, rad) * 0.6;
    dust = smoothstep(0.5, 0.8, ridge(p * 0.7 + 4.0 + r * 1.4)) * edge;
    rim = pow(fil, 7.0) * em * 0.4;
  } else if (type < 2.5) {
    // Bubble: a shell of gas blown out by a hot star, ragged and brighter along its rim.
    float R = 0.55 + 0.12 * f;
    float shell = exp(-pow((rad - R) / 0.1, 2.0));
    em = shell * (0.45 + 0.9 * fil * fil) + 0.12 * dens;
    em2 = smoothstep(R, 0.0, rad) * 0.35 * (0.6 + 0.5 * f);
    dust = smoothstep(0.7, 0.95, ridge(p * 0.9 + 5.0 + r)) * smoothstep(R - 0.05, R + 0.25, rad) * edge;
    rim = shell * pow(fil, 3.0);
  } else if (type < 3.5) {
    // Supernova remnant: fine braided filaments in a broken shell, almost no dust.
    float band = smoothstep(0.25, 0.55, rad) * smoothstep(1.0, 0.7, rad + 0.2 * f);
    float s1 = pow(ridge(p * 1.6 + r * 2.8), 6.0);
    float s2 = pow(ridge(p * 1.6 + r * 2.8 + vec2(0.35, -0.2)), 7.0);
    em = s1 * band * 1.5;
    em2 = s2 * band * 1.3;
    rim = s1 * s1 * band;
  } else {
    // Globule complex: dark knots of dust against faint glowing gas, their edges lit.
    em = 0.42 * dens * (0.6 + 0.6 * fil);
    em2 = em * 0.5 * smoothstep(0.7, 0.0, rad);
    float knots = smoothstep(0.05, 0.3, fbm(p * 0.9 + 7.0 + 1.5 * r) + 0.1 * fil);
    dust = knots * edge;
    rim = smoothstep(0.12, 0.0, abs(fbm(p * 0.9 + 7.0 + 1.5 * r) - 0.08)) * dens * 1.2;
  }

  // What progress has done: a young cluster has blown a cavity in the gas, and the gas facing
  // it glows in a bright rim; what is left of the gas thins toward a few wisps.
  float rc = length(uv + 0.18 * r);
  if (cav > 0.001) {
    float keep = smoothstep(cav * 0.6, cav * 1.08, rc);
    float front = exp(-pow((rc - cav) / 0.07, 2.0)) * dens;
    em *= keep;
    em2 *= keep;
    dust *= keep;
    rim = rim * keep + front * 1.1;
  }
  em *= gas;
  em2 *= gas;
  dust *= mix(0.25, 1.0, gas);
  rim *= mix(0.35, 1.0, gas);
  float slot = smoothstep(1.0, 0.86, max(abs(uv.x), abs(uv.y)));
  gl_FragColor = clamp(vec4(em, em2, dust, rim) * slot, 0.0, 1.0);
}`;

const SLICE_VS = /* glsl */ `
uniform float uDpr;
attribute vec3 aCentre;
attribute vec4 aSlice;
attribute vec3 aColA;
attribute vec3 aColB;
attribute vec3 aRim;
attribute vec2 aLook;
attribute float aAlpha;
varying vec2 vUv;
varying vec2 vSlot;
varying vec3 vA;
varying vec3 vB;
varying vec3 vRim;
varying float vDust;
varying float vFade;
void main() {
  vec3 right = vec3(viewMatrix[0][0], viewMatrix[1][0], viewMatrix[2][0]);
  vec3 up = vec3(viewMatrix[0][1], viewMatrix[1][1], viewMatrix[2][1]);
  vec2 corner = position.xy;
  vec3 world = aCentre + (right * corner.x + up * corner.y) * aSlice.x;
  float c = cos(aSlice.y);
  float s = sin(aSlice.y);
  vUv = mat2(c, -s, s, c) * corner * aLook.x + 0.5;
  vSlot = aSlice.zw;
  vec4 mv = viewMatrix * vec4(world, 1.0);
  gl_Position = projectionMatrix * mv;
  // Fly into a nebula and its slices thin out around the camera instead of filling the view.
  float depth = -mv.z;
  vFade = aAlpha * smoothstep(0.15, 0.75, depth / aSlice.x);
  vA = aColA;
  vB = aColB;
  vRim = aRim;
  vDust = aLook.y;
}`;

const SLICE_EMIT_FS = /* glsl */ `
uniform sampler2D uAtlas;
varying vec2 vUv;
varying vec2 vSlot;
varying vec3 vA;
varying vec3 vB;
varying vec3 vRim;
varying float vDust;
varying float vFade;
void main() {
  if (vUv.x < 0.0 || vUv.y < 0.0 || vUv.x > 1.0 || vUv.y > 1.0 || vFade < 0.002) discard;
  vec4 t = texture2D(uAtlas, (vSlot + vUv) / ${SLOTS}.0);
  vec3 col = (t.r * vA + t.g * vB + t.a * vRim * 1.6) * (1.0 - 0.8 * t.b * vDust);
  gl_FragColor = vec4(col * vFade, 1.0);
}`;

const SLICE_ABSORB_FS = /* glsl */ `
uniform sampler2D uAtlas;
varying vec2 vUv;
varying vec2 vSlot;
varying float vDust;
varying float vFade;
void main() {
  if (vUv.x < 0.0 || vUv.y < 0.0 || vUv.x > 1.0 || vUv.y > 1.0 || vFade < 0.002) discard;
  float d = texture2D(uAtlas, (vSlot + vUv) / ${SLOTS}.0).b * vDust * min(vFade, 1.0);
  gl_FragColor = vec4(min(vec3(0.9), d * vec3(0.72, 0.84, 1.0)), 1.0);
}`;

export type NebulaInput = { repo: string; place: System; progress: number };

/** The baked atlas plus the instanced slices for every nebula in the sky. */
export class Nebulae {
  readonly group = new THREE.Group();
  private atlas: THREE.WebGLRenderTarget;
  private geo: THREE.InstancedBufferGeometry | null = null;
  private emit: THREE.ShaderMaterial;
  private absorb: THREE.ShaderMaterial;
  private alpha: THREE.InstancedBufferAttribute | null = null;
  /** Per repo: the instance range of its slices. */
  private ranges = new Map<string, [number, number]>();
  private key = "";

  constructor(private renderer: THREE.WebGLRenderer) {
    const size = SLOTS * SLOT_PX;
    this.atlas = new THREE.WebGLRenderTarget(size, size, {
      generateMipmaps: true,
      minFilter: THREE.LinearMipmapLinearFilter,
      magFilter: THREE.LinearFilter,
    });
    const uniforms = () => ({
      uAtlas: { value: this.atlas.texture },
      uDpr: { value: 1 },
    });
    const common = {
      vertexShader: SLICE_VS,
      transparent: true,
      depthWrite: false,
      depthTest: true,
    };
    this.emit = new THREE.ShaderMaterial({
      ...common,
      fragmentShader: SLICE_EMIT_FS,
      uniforms: uniforms(),
      blending: THREE.CustomBlending,
      blendEquation: THREE.AddEquation,
      blendSrc: THREE.OneFactor,
      blendDst: THREE.OneFactor,
      blendSrcAlpha: THREE.ZeroFactor,
      blendDstAlpha: THREE.OneFactor,
    });
    this.absorb = new THREE.ShaderMaterial({
      ...common,
      fragmentShader: SLICE_ABSORB_FS,
      uniforms: uniforms(),
      blending: THREE.CustomBlending,
      blendEquation: THREE.AddEquation,
      blendSrc: THREE.ZeroFactor,
      blendDst: THREE.OneMinusSrcColorFactor,
      blendSrcAlpha: THREE.ZeroFactor,
      blendDstAlpha: THREE.OneFactor,
    });
  }

  /** Bake (only when a nebula's look changed) and lay out the slices. */
  set(list: readonly NebulaInput[]) {
    const items = list.slice(0, SLOTS * SLOTS);
    const key = items.map((n) => `${n.repo}:${nebulaStage(n.progress).cavity.toFixed(3)}:${n.place.scale.toFixed(2)}`).join("|");
    if (key !== this.key) {
      this.key = key;
      this.bake(items);
    }
    this.layout(items);
  }

  private bake(items: readonly NebulaInput[]) {
    const A = Array.from({ length: SLOTS * SLOTS }, () => new THREE.Vector4(0, -1, 0, 0));
    const B = Array.from({ length: SLOTS * SLOTS }, () => new THREE.Vector4(1, 0, 0, 0));
    items.forEach((n, i) => {
      const st = nebulaStyle(n.repo);
      const stage = nebulaStage(n.progress);
      A[i].set(st.seed, NEBULA_TYPES.indexOf(st.type), stage.gas, stage.cavity);
      B[i].set(st.aspect, 0, 0, 0);
    });
    const mat = new THREE.ShaderMaterial({
      vertexShader: BAKE_VS,
      fragmentShader: BAKE_FS,
      uniforms: { uA: { value: A }, uB: { value: B } },
      depthTest: false,
      depthWrite: false,
    });
    const quad = new THREE.Mesh(new THREE.PlaneGeometry(1, 1), mat);
    quad.frustumCulled = false;
    const scene = new THREE.Scene();
    scene.add(quad);
    const cam = new THREE.Camera();
    const prev = this.renderer.getRenderTarget();
    this.renderer.setRenderTarget(this.atlas);
    this.renderer.setClearColor(0x000000, 0);
    this.renderer.clear(true, false, false);
    this.renderer.render(scene, cam);
    this.renderer.setRenderTarget(prev);
    quad.geometry.dispose();
    mat.dispose();
  }

  private layout(items: readonly NebulaInput[]) {
    this.group.clear();
    this.geo?.dispose();
    this.ranges.clear();
    let total = 0;
    for (const n of items) total += nebulaStyle(n.repo).slices;
    const quad = new THREE.PlaneGeometry(1, 1);
    const geo = new THREE.InstancedBufferGeometry();
    geo.index = quad.index;
    geo.setAttribute("position", quad.getAttribute("position"));
    const centre = new Float32Array(total * 3);
    const slice = new Float32Array(total * 4);
    const colA = new Float32Array(total * 3);
    const colB = new Float32Array(total * 3);
    const rim = new Float32Array(total * 3);
    const look = new Float32Array(total * 2);
    const alpha = new Float32Array(total).fill(1);
    let k = 0;
    items.forEach((n, slot) => {
      const st = nebulaStyle(n.repo);
      const r = seeded(`slices:${n.repo}`);
      const sc = n.place.scale;
      const start = k;
      // Slices share one orientation (give or take), so the structure adds up instead of
      // averaging into fog; the differences between them are what gives the volume.
      const turn = r() * Math.PI * 2;
      const each = 1.9 / st.slices;
      for (let j = 0; j < st.slices; j++, k++) {
        // Slices spread through the cloud's depth, each turned and scaled a little differently,
        // so they never line up into one flat picture.
        const off = (j / Math.max(1, st.slices - 1) - 0.5) * 0.7;
        centre.set([n.place.x + (r() - 0.5) * 0.35 * sc, n.place.y + off * sc * 0.6, n.place.z + (r() - 0.5) * 0.35 * sc + off * sc * 0.4], k * 3);
        slice.set([sc * (2.3 + r() * 0.5), turn + (r() - 0.5) * 0.5, slot % SLOTS, Math.floor(slot / SLOTS)], k * 4);
        const w = each * (j === Math.floor(st.slices / 2) ? 1.8 : 0.65);
        colA.set(
          st.a.map((v) => v * w),
          k * 3,
        );
        colB.set(
          st.b.map((v) => v * w),
          k * 3,
        );
        rim.set(
          st.rim.map((v) => v * w),
          k * 3,
        );
        look.set([0.85 + r() * 0.3, st.dust], k * 2);
      }
      this.ranges.set(n.repo, [start, k - start]);
    });
    geo.setAttribute("aCentre", new THREE.InstancedBufferAttribute(centre, 3));
    geo.setAttribute("aSlice", new THREE.InstancedBufferAttribute(slice, 4));
    geo.setAttribute("aColA", new THREE.InstancedBufferAttribute(colA, 3));
    geo.setAttribute("aColB", new THREE.InstancedBufferAttribute(colB, 3));
    geo.setAttribute("aRim", new THREE.InstancedBufferAttribute(rim, 3));
    geo.setAttribute("aLook", new THREE.InstancedBufferAttribute(look, 2));
    this.alpha = new THREE.InstancedBufferAttribute(alpha, 1);
    this.alpha.setUsage(THREE.DynamicDrawUsage);
    geo.setAttribute("aAlpha", this.alpha);
    geo.instanceCount = total;
    quad.index = null;
    quad.dispose();
    this.geo = geo;
    // Dust first, so it darkens the disc behind; then the glow (which carries its own dust).
    const a = new THREE.Mesh(geo, this.absorb);
    const e = new THREE.Mesh(geo, this.emit);
    for (const m of [a, e]) m.frustumCulled = false;
    a.renderOrder = 2.5;
    e.renderOrder = 2.6;
    this.group.add(a, e);
  }

  /** A nebula's brightness this frame (lens, hover, fading as its constellation opens). */
  setAlpha(repo: string, v: number) {
    const r = this.ranges.get(repo);
    if (!r || !this.alpha) return;
    const arr = this.alpha.array as Float32Array;
    let changed = false;
    for (let i = r[0]; i < r[0] + r[1]; i++)
      if (arr[i] !== v) {
        arr[i] = v;
        changed = true;
      }
    if (changed) this.alpha.needsUpdate = true;
  }

  dispose() {
    this.group.clear();
    this.geo?.dispose();
    this.emit.dispose();
    this.absorb.dispose();
    this.atlas.dispose();
  }
}
