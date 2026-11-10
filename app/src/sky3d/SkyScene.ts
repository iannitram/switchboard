// The 3D sky's renderer (#297, pass 2): one living spiral galaxy. Projects are star systems in
// its disc, the switchboard project is the black hole at its centre, and the camera zooms
// continuously from the whole galaxy down to a single issue star. It draws only; the React
// overlay (Sky3D.tsx) holds labels, buttons and panels and asks this scene where things are.
import * as THREE from "three";
import type { Constellation, SkyModel } from "../sky/model";
import { highlightChain } from "../sky/chain";
import { blendPose, fitDistance, layoutInterior, poseEye, type InteriorLayout, type Pose, type Vec3 } from "./layout";
import { discParticles, dustParticles, GAL_R, HOLE, placeSystems, systemParticles, type System } from "./milkyway";
import { dampPose, detailFade, flightMs, MIN_DIST, settling, zoneStep, zoomToward } from "./camera";
import { easeInOutCubic } from "./motion";
import { condensation, debrisCount, lensDebris, lensDisc, lensStar, lensSystem, starPhysics, systemFacts, type Lens } from "./physics";
import { spanningTree } from "./wayfind";
import { gauss, seeded } from "./random";

const FOV = 42;
const OVER_ELEV = 0.72;
const SYS_ELEV = 1.05;
/** A click lands this many system radii away: the whole constellation in view. */
const SYS_DIST = 2.35;
const IDLE_MS = 20000;
const DISC_COUNT = 64000;
const DUST_COUNT = 3800;
const AMBER = new THREE.Color("#f5a524");
const REUSE = new THREE.Color("#93c5fd");

// Light adds to the canvas without writing alpha, so it composites over the CSS sky as pure
// additive light (alpha written by additive points would show as dark specks).
const LIGHT = {
  transparent: true,
  depthWrite: false,
  blending: THREE.CustomBlending,
  blendEquation: THREE.AddEquation,
  blendSrc: THREE.OneFactor,
  blendDst: THREE.OneFactor,
  blendSrcAlpha: THREE.ZeroFactor,
  blendDstAlpha: THREE.OneFactor,
} as const;
/** The same for sprites and lines, whose colour is scaled by their alpha. */
const GLOW = { ...LIGHT, blendSrc: THREE.SrcAlphaFactor } as const;
/** Dust and dark knots: real occlusion, darkening what is behind. */
const SHADE = { transparent: true, depthWrite: false, blending: THREE.NormalBlending } as const;

export type Screen = { x: number; y: number; r: number; visible: boolean };
export type ZoneEvent = { enter: string } | { leave: true };

type Sys = {
  c: Constellation;
  place: System;
  points: THREE.Points | null;
  mat: THREE.ShaderMaterial | null;
  core: THREE.Sprite | null;
  debris: [number, number];
  lens: number;
};

type Interior = {
  repo: string;
  group: THREE.Group;
  layout: InteriorLayout;
  R: number;
  stars: { number: number; index: number; state: "open" | "closed"; inFlight: boolean; chain: boolean; edged: boolean; size: number }[];
  starGeo: THREE.BufferGeometry;
  starMat: THREE.ShaderMaterial;
  darkMat: THREE.ShaderMaterial | null;
  flowGeo: THREE.BufferGeometry | null;
  flowMat: THREE.ShaderMaterial | null;
  flowEdge: number[];
  edgeGeo: THREE.BufferGeometry | null;
  edges: [number, number][];
  chainPairs: Set<string>;
  fadeMats: { mat: THREE.Material & { opacity: number }; base: number }[];
  own: { dispose(): void }[];
  fade: number;
};

// ---- shaders ------------------------------------------------------------------------------

const DISC_VS = /* glsl */ `
uniform float uTime;
uniform float uPx;
uniform float uDpr;
uniform float uSpin;
uniform float uDim;
attribute vec4 aOrbit;
attribute vec3 aColor;
attribute float aPhase;
attribute float aKind;
varying vec3 vColor;
varying float vKind;
varying float vAlpha;
varying float vOp;
void main() {
  float r = aOrbit.x;
  float inner = 30.0 / (r + 30.0);
  // Slow rotation, faster inside; the shear breathes instead of winding the arms up.
  float th = aOrbit.y + uSpin * (uTime * (0.0018 + 0.0014 * inner) + 0.05 * inner * sin(uTime * 0.0021));
  vec3 p = vec3(r * cos(th), aOrbit.z, r * sin(th));
  vec4 mv = modelViewMatrix * vec4(p, 1.0);
  gl_Position = projectionMatrix * mv;
  float depth = max(-mv.z, 0.001);
  float px = aOrbit.w * uPx / depth;
  float a = 1.0;
  if (aKind < 0.5) {
    // A star stays a point: farther is dimmer, not smaller.
    a = clamp(px * 0.8, 0.22, 1.0);
    px = clamp(px, 1.0 * uDpr, 3.0 * uDpr);
    float tw = 0.8 + 0.2 * sin(uTime * (0.6 + aPhase * 1.5) + aPhase * 60.0);
    a *= mix(1.0, tw, step(0.55, aPhase));
  } else {
    px = min(px, 200.0 * uDpr);
    a = smoothstep(1.0, 14.0, depth);
  }
  gl_PointSize = px;
  vColor = aColor;
  vKind = aKind;
  vAlpha = a * uDim;
  vOp = aPhase;
}`;

const DISC_FS = /* glsl */ `
varying vec3 vColor;
varying float vKind;
varying float vAlpha;
void main() {
  float d = length(gl_PointCoord - 0.5) * 2.0;
  if (d > 1.0) discard;
  float a = vKind < 0.5 ? exp(-d * d * 4.0) * 0.6 + smoothstep(0.55, 0.0, d) * 0.55 : exp(-d * d * 3.4) * 0.42;
  gl_FragColor = vec4(vColor * a * vAlpha, 1.0);
}`;

const DUST_FS = /* glsl */ `
varying vec3 vColor;
varying float vAlpha;
varying float vOp;
void main() {
  float d = length(gl_PointCoord - 0.5) * 2.0;
  if (d > 1.0) discard;
  gl_FragColor = vec4(vColor, exp(-d * d * 3.0) * vOp * min(vAlpha, 1.0));
}`;

const SYS_VS = /* glsl */ `
uniform float uTime;
uniform float uPx;
uniform float uDpr;
uniform float uScale;
uniform float uAlpha;
attribute vec3 aColor;
attribute float aSize;
attribute float aKind;
attribute float aPhase;
varying vec3 vColor;
varying float vKind;
varying float vAlpha;
void main() {
  vec3 p = position;
  float ang = uTime * 0.012 * (1.3 - min(length(p), 1.0));
  float c = cos(ang);
  float s = sin(ang);
  p.xz = mat2(c, -s, s, c) * p.xz;
  vec4 mv = modelViewMatrix * vec4(p, 1.0);
  gl_Position = projectionMatrix * mv;
  float depth = max(-mv.z, 0.001);
  float px = aSize * uScale * uPx / depth;
  float a = 1.0;
  if (aKind < 0.5) {
    a = clamp(px * 0.5, 0.35, 1.0);
    px = clamp(px, 1.4 * uDpr, 8.0 * uDpr);
    a *= 0.85 + 0.15 * sin(uTime * (0.8 + aPhase) + aPhase * 50.0);
  } else if (aKind < 1.5) {
    px = min(px, 220.0 * uDpr);
    a = smoothstep(0.6, 3.0, depth / uScale);
  } else {
    // A wisp: fine structure, which only shows once the camera is close enough to resolve it.
    px = min(px, 90.0 * uDpr);
    a = smoothstep(0.25, 1.2, depth / uScale) * smoothstep(9.0, 2.5, depth / uScale);
  }
  gl_PointSize = px;
  vColor = aColor;
  vKind = aKind;
  vAlpha = a * uAlpha;
}`;

const SYS_FS = /* glsl */ `
varying vec3 vColor;
varying float vKind;
varying float vAlpha;
void main() {
  float d = length(gl_PointCoord - 0.5) * 2.0;
  if (d > 1.0) discard;
  float a = vKind < 0.5 ? exp(-d * d * 5.0) * 0.6 + smoothstep(0.45, 0.0, d) * 0.5 : vKind < 1.5 ? exp(-d * d * 3.0) * 0.05 : exp(-d * d * 6.0) * 0.34;
  gl_FragColor = vec4(vColor * a * vAlpha, 1.0);
}`;

const FIELD_VS = /* glsl */ `
uniform float uDpr;
uniform float uTime;
attribute float aSize;
attribute vec3 aColor;
attribute float aPhase;
varying vec3 vColor;
void main() {
  vec4 mv = modelViewMatrix * vec4(position, 1.0);
  gl_Position = projectionMatrix * mv;
  gl_PointSize = aSize * uDpr;
  float tw = 0.85 + 0.15 * sin(uTime * (0.5 + aPhase) + aPhase * 40.0);
  vColor = aColor * tw;
}`;

const FIELD_FS = /* glsl */ `
varying vec3 vColor;
void main() {
  float d = length(gl_PointCoord - 0.5) * 2.0;
  if (d > 1.0) discard;
  float a = smoothstep(1.0, 0.0, d);
  gl_FragColor = vec4(vColor * a * a, 1.0);
}`;

const DEBRIS_VS = /* glsl */ `
uniform float uTime;
uniform float uDpr;
uniform float uPx;
attribute vec4 aOrbit;
attribute float aAlpha;
varying float vAlpha;
void main() {
  float ang = aOrbit.y + uTime * aOrbit.z;
  vec3 p = position + vec3(cos(ang) * aOrbit.x, sin(ang * 3.0 + aOrbit.y * 5.0) * aOrbit.w, sin(ang) * aOrbit.x);
  vec4 mv = modelViewMatrix * vec4(p, 1.0);
  gl_Position = projectionMatrix * mv;
  float px = 0.16 * uPx / max(-mv.z, 0.001);
  gl_PointSize = clamp(px, 1.3 * uDpr, 4.5 * uDpr);
  vAlpha = aAlpha * clamp(px, 0.4, 1.0);
}`;

const DEBRIS_FS = /* glsl */ `
uniform vec3 uColor;
varying float vAlpha;
void main() {
  float d = length(gl_PointCoord - 0.5) * 2.0;
  if (d > 1.0) discard;
  gl_FragColor = vec4(uColor * (exp(-d * d * 4.0) * 0.9) * vAlpha, 1.0);
}`;

const BRIDGE_VS = /* glsl */ `
attribute float aT;
attribute vec3 aColor;
varying float vT;
varying vec3 vColor;
void main() {
  vT = aT;
  vColor = aColor;
  gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
}`;

const BRIDGE_FS = /* glsl */ `
uniform float uTime;
uniform float uAlpha;
varying float vT;
varying vec3 vColor;
void main() {
  float flow = pow(fract(vT * 3.0 - uTime * 0.12), 8.0);
  float ends = smoothstep(0.0, 0.08, vT) * smoothstep(1.0, 0.92, vT);
  gl_FragColor = vec4(vColor * (0.3 + 0.9 * flow) * ends * uAlpha, 1.0);
}`;

const DISCRING_VS = /* glsl */ `
varying vec2 vP;
void main() {
  vP = position.xy;
  gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
}`;

const DISCRING_FS = /* glsl */ `
uniform float uTime;
uniform float uInner;
uniform float uOuter;
uniform float uAlpha;
uniform vec2 uView;
varying vec2 vP;
void main() {
  float r = length(vP);
  float ang = atan(vP.y, vP.x);
  float t = clamp((r - uInner) / (uOuter - uInner), 0.0, 1.0);
  float heat = pow(1.0 - t, 1.7);
  float sw = ang * 2.0 + uTime * 0.45 / (0.25 + t) - log(r) * 6.0;
  float n = 0.6 + 0.25 * sin(sw * 3.0 + sin(sw * 1.7 + r * 3.0) * 1.4) + 0.15 * sin(r * 14.0 - uTime * 0.6);
  float dop = 1.0 + 0.5 * dot(vec2(-sin(ang), cos(ang)), uView);
  vec3 col = mix(vec3(1.0, 0.42, 0.14), vec3(1.0, 0.94, 0.82), heat);
  float edge = smoothstep(0.0, 0.06, t) * smoothstep(1.0, 0.5, t);
  gl_FragColor = vec4(col * (0.1 + heat * 0.8) * n * dop * edge * uAlpha, 1.0);
}`;

const LENS_VS = /* glsl */ `
varying vec2 vUv;
void main() {
  vUv = uv;
  gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
}`;

const LENS_FS = /* glsl */ `
uniform float uHalf;
uniform float uHorizon;
uniform float uAlpha;
uniform float uTime;
varying vec2 vUv;
void main() {
  vec2 p = (vUv - 0.5) * 2.0 * uHalf;
  float d = length(p) / uHorizon;
  if (d < 1.0) discard;
  float up = 0.7 + 0.3 * (p.y / max(length(p), 1e-4));
  float photon = exp(-pow((d - 1.06) / 0.03, 2.0)) * 1.2;
  float einstein = exp(-pow((d - 1.4) / 0.22, 2.0)) * 0.22 * up * (0.9 + 0.1 * sin(uTime * 0.3 + atan(p.y, p.x) * 3.0));
  float halo = exp(-(d - 1.0) * 1.3) * 0.1;
  vec3 col = vec3(1.0, 0.83, 0.58) * (photon + einstein) + vec3(0.55, 0.68, 1.0) * halo;
  gl_FragColor = vec4(col * uAlpha, 1.0);
}`;

const STAR_VS = /* glsl */ `
uniform float uPx;
uniform float uDpr;
uniform float uScale;
uniform float uTime;
uniform float uPulse;
attribute vec3 aColor;
attribute float aSize;
attribute float aAlpha;
attribute float aKind;
attribute float aCorona;
attribute float aPhase;
attribute float aGlow;
varying float vGlow;
varying vec3 vColor;
varying float vAlpha;
varying float vKind;
varying float vCorona;
varying float vSpin;
void main() {
  vec4 mv = modelViewMatrix * vec4(position, 1.0);
  gl_Position = projectionMatrix * mv;
  float px = aSize * uScale * uPx / max(-mv.z, 0.001);
  float pulse = aKind > 1.5 ? uPulse * (0.5 + 0.5 * sin(uTime * 3.2 + aPhase * 6.283)) : 0.0;
  gl_PointSize = clamp(px * (1.0 + 0.12 * pulse), 9.0 * uDpr, 380.0 * uDpr);
  vColor = aColor;
  vAlpha = aAlpha * (1.0 + 0.55 * pulse);
  vKind = aKind;
  vCorona = aCorona;
  vSpin = uTime * 1.5 * uPulse + aPhase * 6.283;
  vGlow = aGlow;
}`;

const STAR_FS = /* glsl */ `
uniform float uFade;
varying vec3 vColor;
varying float vAlpha;
varying float vKind;
varying float vCorona;
varying float vSpin;
varying float vGlow;
void main() {
  vec2 p = gl_PointCoord - 0.5;
  float d = length(p) * 2.0;
  if (d > 1.0) discard;
  vec3 col;
  if (vKind < 0.5) {
    // Closed: a steady warm star with soft spikes.
    float core = smoothstep(0.28, 0.15, d);
    float glow = exp(-d * d * 9.0) * 0.7;
    float spikes = (exp(-abs(p.x) * 80.0) + exp(-abs(p.y) * 80.0)) * smoothstep(0.95, 0.1, d) * 0.35;
    col = vColor * (core * 1.25 + (glow + spikes) * vGlow);
  } else if (vKind < 1.5) {
    // Open: a dim protostar inside a faint dust halo.
    float core = smoothstep(0.2, 0.1, d) * 0.85;
    float halo = exp(-pow((d - 0.46) / 0.16, 2.0)) * 0.24;
    col = vColor * (core + exp(-d * d * 14.0) * 0.25) + vec3(0.62, 0.4, 0.26) * halo;
  } else {
    // In flight: a pulsar, two beams sweeping round; still under reduced motion.
    float core = smoothstep(0.22, 0.1, d) * 1.3;
    float ang = atan(p.y, p.x) - vSpin;
    float beam = pow(abs(cos(ang)), 28.0) * smoothstep(1.0, 0.15, d) * 0.85;
    float ring = smoothstep(0.045, 0.0, abs(d - 0.72)) * 0.55;
    col = vColor * (core + beam + ring + exp(-d * d * 10.0) * 0.5 * vGlow);
  }
  if (vCorona > 0.5) {
    vec3 cc = vCorona < 1.5 ? vec3(0.35, 0.95, 0.55) : vec3(1.0, 0.36, 0.36);
    col += cc * smoothstep(0.035, 0.0, abs(d - 0.36)) * 0.95;
  }
  gl_FragColor = vec4(col * vAlpha * uFade, 1.0);
}`;

const DARK_VS = /* glsl */ `
uniform float uPx;
uniform float uDpr;
uniform float uScale;
attribute float aSize;
attribute float aAlpha;
varying float vAlpha;
void main() {
  vec4 mv = modelViewMatrix * vec4(position, 1.0);
  gl_Position = projectionMatrix * mv;
  gl_PointSize = clamp(aSize * uScale * uPx / max(-mv.z, 0.001), 12.0 * uDpr, 300.0 * uDpr);
  vAlpha = aAlpha;
}`;

const DARK_FS = /* glsl */ `
uniform float uFade;
varying float vAlpha;
void main() {
  vec2 p = gl_PointCoord - 0.5;
  float d = length(p) * 2.0;
  if (d > 1.0) discard;
  float fill = smoothstep(0.7, 0.25, d) * 0.85;
  float dash = step(0.0, sin(atan(p.y, p.x) * 8.0));
  float ring = smoothstep(0.06, 0.0, abs(d - 0.78)) * dash;
  vec3 col = mix(vec3(0.03, 0.025, 0.03), vec3(0.58, 0.62, 0.76), ring);
  gl_FragColor = vec4(col, max(fill, ring * 0.9) * vAlpha * uFade);
}`;

const FLOW_VS = /* glsl */ `
uniform float uTime;
uniform float uFlow;
uniform float uPx;
uniform float uDpr;
uniform float uScale;
uniform float uSize;
attribute vec3 aB;
attribute float aPhase;
attribute float aHot;
attribute float aHi;
varying float vS;
varying float vHot;
varying float vHi;
void main() {
  float s = fract(aPhase + uTime * 0.16 * uFlow);
  vec3 p = mix(position, aB, s);
  vec4 mv = modelViewMatrix * vec4(p, 1.0);
  gl_Position = projectionMatrix * mv;
  float px = uSize * uScale * uPx / max(-mv.z, 0.001) * (aHot > 0.5 ? 1.5 : 1.0);
  gl_PointSize = clamp(px, 2.0 * uDpr, 18.0 * uDpr);
  vS = s;
  vHot = aHot;
  vHi = aHi;
}`;

const FLOW_FS = /* glsl */ `
uniform float uFade;
varying float vS;
varying float vHot;
varying float vHi;
void main() {
  float d = length(gl_PointCoord - 0.5) * 2.0;
  if (d > 1.0) discard;
  vec3 col = vHot > 0.5 ? vec3(1.0, 0.76, 0.32) : vec3(0.55, 0.78, 1.0);
  float a = exp(-d * d * 5.0) * (0.12 + 0.88 * vS * vS) * (vHot > 0.5 ? 1.3 : 0.8);
  gl_FragColor = vec4(col * a * vHi * uFade, 1.0);
}`;

// ---- procedural textures ------------------------------------------------------------------

function glowTexture(): THREE.Texture {
  const c = document.createElement("canvas");
  c.width = c.height = 128;
  const g = c.getContext("2d")!;
  const grad = g.createRadialGradient(64, 64, 0, 64, 64, 64);
  grad.addColorStop(0, "rgba(255,255,255,1)");
  grad.addColorStop(0.15, "rgba(255,255,255,0.5)");
  grad.addColorStop(0.45, "rgba(255,255,255,0.1)");
  grad.addColorStop(1, "rgba(255,255,255,0)");
  g.fillStyle = grad;
  g.fillRect(0, 0, 128, 128);
  const t = new THREE.CanvasTexture(c);
  t.colorSpace = THREE.SRGBColorSpace;
  return t;
}

function pointsOf(geo: THREE.BufferGeometry, mat: THREE.Material, order: number): THREE.Points {
  const p = new THREE.Points(geo, mat);
  p.frustumCulled = false;
  p.renderOrder = order;
  return p;
}

// ---- the scene ----------------------------------------------------------------------------

export class SkyScene {
  readonly renderer: THREE.WebGLRenderer;
  private scene = new THREE.Scene();
  private camera = new THREE.PerspectiveCamera(FOV, 1, 0.05, 6000);
  private systems = new Map<string, Sys>();
  private interior: Interior | null = null;
  private skyGroup = new THREE.Group();
  private field: THREE.ShaderMaterial;
  private discMat: THREE.ShaderMaterial;
  private dustMat: THREE.ShaderMaterial;
  private bulge: THREE.Sprite;
  private hole: { group: THREE.Group; disc: THREE.ShaderMaterial; lens: THREE.ShaderMaterial; lensMesh: THREE.Mesh } | null = null;
  private debris: { geo: THREE.BufferGeometry; mat: THREE.ShaderMaterial } | null = null;
  private bridges: THREE.ShaderMaterial | null = null;
  private glow: THREE.Texture;
  private disposables: { dispose(): void }[] = [];
  private skyDisposables: { dispose(): void }[] = [];
  private width = 1;
  private height = 1;
  private dpr = 1;
  private raf = 0;
  private running = false;
  private dirty = true;
  private last = performance.now();
  private lastInput = performance.now();
  private skip = false;
  private cam: Pose = { target: { x: 0, y: 0, z: 0 }, dist: 300, az: 0.5, elev: OVER_ELEV };
  private goal: Pose = { target: { x: 0, y: 0, z: 0 }, dist: 300, az: 0.5, elev: OVER_ELEV };
  private tween: { from: Pose; to: Pose; start: number; ms: number } | null = null;
  private placed = false;
  private moved = false;
  private open: string | null = null;
  private hovered: string | null = null;
  private focusStar: number | null = null;
  private picked: number | null = null;
  private lens: Lens = "natural";
  private tmp = new THREE.Vector3();
  private right = new THREE.Vector3();
  private time = 0;
  /** Called after each drawn frame: the overlay repositions itself. */
  onFrame: (() => void) | null = null;
  /** The camera entered or left a project by zooming. */
  onZone: ((z: ZoneEvent) => void) | null = null;

  constructor(
    canvas: HTMLCanvasElement,
    private reducedMotion: boolean,
  ) {
    this.renderer = new THREE.WebGLRenderer({ canvas, antialias: true, alpha: true, powerPreference: "default" });
    this.renderer.setClearColor(0x000000, 0);
    this.glow = glowTexture();
    this.disposables.push(this.glow);
    this.field = this.buildField();
    [this.discMat, this.dustMat] = this.buildDisc();
    const bulgeMat = new THREE.SpriteMaterial({ map: this.glow, color: new THREE.Color(1, 0.78, 0.5), opacity: 0.3, ...GLOW });
    this.bulge = new THREE.Sprite(bulgeMat);
    this.bulge.scale.setScalar(GAL_R * 0.55);
    this.bulge.renderOrder = 1;
    this.disposables.push(bulgeMat);
    this.scene.add(this.bulge, this.skyGroup);
  }

  // -- background: a far star shell -----------------------------------------------------------
  private buildField(): THREE.ShaderMaterial {
    const r = seeded("switchboard-sky-field");
    const n = 3600;
    const pos = new Float32Array(n * 3);
    const col = new Float32Array(n * 3);
    const size = new Float32Array(n);
    const phase = new Float32Array(n);
    for (let i = 0; i < n; i++) {
      const u = r() * 2 - 1;
      const t = r() * Math.PI * 2;
      const s = Math.sqrt(1 - u * u);
      const rad = 2400 + r() * 1200;
      pos.set([rad * s * Math.cos(t), rad * u, rad * s * Math.sin(t)], i * 3);
      const temp = r();
      const c = temp < 0.15 ? [1, 0.82, 0.66] : temp < 0.55 ? [0.78, 0.86, 1] : [0.92, 0.94, 1];
      const b = 0.25 + Math.pow(r(), 3) * 0.6;
      col.set([c[0] * b, c[1] * b, c[2] * b], i * 3);
      size[i] = 1.1 + Math.pow(r(), 6) * 2.4;
      phase[i] = r();
    }
    const geo = new THREE.BufferGeometry();
    geo.setAttribute("position", new THREE.BufferAttribute(pos, 3));
    geo.setAttribute("aColor", new THREE.BufferAttribute(col, 3));
    geo.setAttribute("aSize", new THREE.BufferAttribute(size, 1));
    geo.setAttribute("aPhase", new THREE.BufferAttribute(phase, 1));
    const mat = new THREE.ShaderMaterial({ vertexShader: FIELD_VS, fragmentShader: FIELD_FS, uniforms: { uDpr: { value: 1 }, uTime: { value: 0 } }, ...LIGHT });
    this.scene.add(pointsOf(geo, mat, 0));
    this.disposables.push(geo, mat);
    return mat;
  }

  // -- the galaxy: stars and glowing gas, then dark dust lanes over them ------------------------
  private buildDisc(): [THREE.ShaderMaterial, THREE.ShaderMaterial] {
    const uniforms = () => ({ uTime: { value: 0 }, uPx: { value: 900 }, uDpr: { value: 1 }, uSpin: { value: 1 }, uDim: { value: 1 } });
    const make = (p: ReturnType<typeof discParticles>) => {
      const geo = new THREE.BufferGeometry();
      geo.setAttribute("position", new THREE.BufferAttribute(new Float32Array(p.count * 3), 3));
      geo.setAttribute("aOrbit", new THREE.BufferAttribute(p.orbit, 4));
      geo.setAttribute("aColor", new THREE.BufferAttribute(p.colors, 3));
      geo.setAttribute("aPhase", new THREE.BufferAttribute(p.phase, 1));
      geo.setAttribute("aKind", new THREE.BufferAttribute(p.kind, 1));
      this.disposables.push(geo);
      return geo;
    };
    const discMat = new THREE.ShaderMaterial({ vertexShader: DISC_VS, fragmentShader: DISC_FS, uniforms: uniforms(), ...LIGHT });
    const dustMat = new THREE.ShaderMaterial({ vertexShader: DISC_VS, fragmentShader: DUST_FS, uniforms: uniforms(), ...SHADE });
    this.scene.add(pointsOf(make(discParticles("milky-way", DISC_COUNT)), discMat, 1));
    this.scene.add(pointsOf(make(dustParticles("milky-way", DUST_COUNT)), dustMat, 2));
    this.disposables.push(discMat, dustMat);
    return [discMat, dustMat];
  }

  // -- the black hole: a dark horizon, a hot accretion disc, a lensing ring -------------------
  private buildHole() {
    const group = new THREE.Group();
    const own: { dispose(): void }[] = [];
    const horizonGeo = new THREE.SphereGeometry(HOLE.horizon, 48, 24);
    const horizonMat = new THREE.MeshBasicMaterial({ color: 0x000000 });
    const horizon = new THREE.Mesh(horizonGeo, horizonMat);
    horizon.renderOrder = 0;
    const ringGeo = new THREE.RingGeometry(HOLE.horizon * 1.3, HOLE.discOuter, 160, 1);
    const disc = new THREE.ShaderMaterial({
      vertexShader: DISCRING_VS,
      fragmentShader: DISCRING_FS,
      uniforms: { uTime: { value: 0 }, uInner: { value: HOLE.horizon * 1.3 }, uOuter: { value: HOLE.discOuter }, uAlpha: { value: 1 }, uView: { value: new THREE.Vector2() } },
      side: THREE.DoubleSide,
      ...LIGHT,
      depthTest: true,
    });
    const discMesh = new THREE.Mesh(ringGeo, disc);
    discMesh.rotation.x = -Math.PI / 2;
    discMesh.rotation.z = 0.0;
    discMesh.renderOrder = 4;
    const half = HOLE.horizon * 3.2;
    const lensGeo = new THREE.PlaneGeometry(half * 2, half * 2);
    const lens = new THREE.ShaderMaterial({
      vertexShader: LENS_VS,
      fragmentShader: LENS_FS,
      uniforms: { uHalf: { value: half }, uHorizon: { value: HOLE.horizon }, uAlpha: { value: 1 }, uTime: { value: 0 } },
      ...LIGHT,
      depthTest: false,
    });
    const lensMesh = new THREE.Mesh(lensGeo, lens);
    lensMesh.renderOrder = 5;
    const coreMat = new THREE.SpriteMaterial({ map: this.glow, color: new THREE.Color(1, 0.72, 0.42), opacity: 0.3, ...GLOW });
    const core = new THREE.Sprite(coreMat);
    core.scale.setScalar(HOLE.discOuter * 5);
    core.renderOrder = 3;
    group.add(core, horizon, discMesh, lensMesh);
    own.push(horizonGeo, horizonMat, ringGeo, disc, lensGeo, lens, coreMat);
    this.skyGroup.add(group);
    this.skyDisposables.push(...own);
    this.hole = { group, disc, lens, lensMesh };
  }

  // -- the projects ---------------------------------------------------------------------------
  setSky(sky: SkyModel) {
    for (const d of this.skyDisposables) d.dispose();
    this.skyDisposables = [];
    this.skyGroup.clear();
    this.systems.clear();
    this.hole = null;
    this.debris = null;
    this.bridges = null;

    const places = placeSystems(sky.constellations);
    for (const c of sky.constellations) {
      const place = places.get(c.repo)!;
      const sys: Sys = { c, place, points: null, mat: null, core: null, debris: [0, 0], lens: 1 };
      if (place.centre) {
        this.buildHole();
      } else {
        const cond = condensation(c);
        const p = systemParticles(c.repo, cond, 140 + Math.round(360 * c.size));
        const geo = new THREE.BufferGeometry();
        geo.setAttribute("position", new THREE.BufferAttribute(p.local, 3));
        geo.setAttribute("aColor", new THREE.BufferAttribute(p.colors, 3));
        const size = new Float32Array(p.count);
        for (let i = 0; i < p.count; i++) size[i] = p.orbit[i * 4 + 3];
        geo.setAttribute("aSize", new THREE.BufferAttribute(size, 1));
        geo.setAttribute("aKind", new THREE.BufferAttribute(p.kind, 1));
        geo.setAttribute("aPhase", new THREE.BufferAttribute(p.phase, 1));
        const mat = new THREE.ShaderMaterial({
          vertexShader: SYS_VS,
          fragmentShader: SYS_FS,
          uniforms: { uTime: { value: 0 }, uPx: { value: 900 }, uDpr: { value: 1 }, uScale: { value: place.scale }, uAlpha: { value: 1 } },
          ...LIGHT,
        });
        const points = pointsOf(geo, mat, 3);
        points.position.set(place.x, place.y, place.z);
        points.scale.setScalar(place.scale);
        // A condensed system gets a bright heart; a nebula only a faint one.
        const coreMat = new THREE.SpriteMaterial({ map: this.glow, color: new THREE.Color(1, 0.9, 0.75), opacity: 0.05 + 0.6 * cond, ...GLOW });
        const core = new THREE.Sprite(coreMat);
        core.position.copy(points.position);
        core.scale.setScalar(place.scale * (0.9 + 0.6 * cond));
        core.renderOrder = 3;
        this.skyGroup.add(points, core);
        this.skyDisposables.push(geo, mat, coreMat);
        sys.points = points;
        sys.mat = mat;
        sys.core = core;
      }
      this.systems.set(c.repo, sys);
    }
    if (!this.hole) this.bulge.scale.setScalar(GAL_R * 0.55);
    this.buildDebris();
    this.buildBridges(sky);
    this.applyLens();

    if (!this.placed) {
      this.placed = true;
      this.cam = this.overviewPose();
      this.goal = { ...this.cam };
    }
    if (this.interior) {
      const repo = this.interior.repo;
      this.dropInterior();
      if (this.systems.has(repo)) this.buildInterior(repo);
    }
    if (this.open && !this.systems.has(this.open)) this.open = null;
    this.dirty = true;
  }

  /** SKY-5 as debris: amber grains orbiting each project, denser with more drift. */
  private buildDebris() {
    let total = 0;
    for (const s of this.systems.values()) total += debrisCount(s.c.drift.length);
    if (total === 0) return;
    const pos = new Float32Array(total * 3);
    const orbit = new Float32Array(total * 4);
    const alpha = new Float32Array(total);
    let i = 0;
    for (const s of this.systems.values()) {
      const n = debrisCount(s.c.drift.length);
      const r = seeded(`debris:${s.c.repo}`);
      const base = s.place.centre ? HOLE.discOuter * 1.5 : s.place.scale * 1.45;
      s.debris = [i, n];
      for (let k = 0; k < n; k++, i++) {
        pos.set([s.place.x, s.place.y, s.place.z], i * 3);
        orbit.set([base * (1 + gauss(r) * 0.08), r() * Math.PI * 2, (0.05 + r() * 0.08) * (r() < 0.5 ? 1 : 1), base * 0.05 * r()], i * 4);
        alpha[i] = 0.55 + r() * 0.45;
      }
    }
    const geo = new THREE.BufferGeometry();
    geo.setAttribute("position", new THREE.BufferAttribute(pos, 3));
    geo.setAttribute("aOrbit", new THREE.BufferAttribute(orbit, 4));
    geo.setAttribute("aAlpha", new THREE.BufferAttribute(alpha, 1));
    geo.setAttribute("aBase", new THREE.BufferAttribute(alpha.slice(), 1));
    const mat = new THREE.ShaderMaterial({
      vertexShader: DEBRIS_VS,
      fragmentShader: DEBRIS_FS,
      uniforms: { uTime: { value: 0 }, uDpr: { value: 1 }, uPx: { value: 900 }, uColor: { value: AMBER } },
      ...LIGHT,
    });
    this.skyGroup.add(pointsOf(geo, mat, 6));
    this.skyDisposables.push(geo, mat);
    this.debris = { geo, mat };
  }

  /** SKY-6 as light bridges arcing above the disc; amber when the reused project is parked. */
  private buildBridges(sky: SkyModel) {
    const nameOf = (repo: string) => repo.slice(repo.lastIndexOf("/") + 1);
    const segs: number[] = [];
    const ts: number[] = [];
    const cols: number[] = [];
    const STEPS = 48;
    for (const c of sky.constellations) {
      for (const r of c.reuses) {
        const target = sky.constellations.find((t) => t.repo === r.repo || nameOf(t.repo) === r.repo);
        if (!target || target === c) continue;
        const a = this.systems.get(c.repo)!.place;
        const b = this.systems.get(target.repo)!.place;
        const len = Math.hypot(a.x - b.x, a.z - b.z);
        const lift = 4 + len * 0.28;
        const col = r.warning ? AMBER : REUSE;
        const at = (t: number) => [a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t + Math.sin(Math.PI * t) * lift, a.z + (b.z - a.z) * t];
        for (let i = 0; i < STEPS; i++) {
          const t0 = i / STEPS;
          const t1 = (i + 1) / STEPS;
          segs.push(...at(t0), ...at(t1));
          ts.push(t0, t1);
          cols.push(col.r, col.g, col.b, col.r, col.g, col.b);
        }
      }
    }
    if (!segs.length) return;
    const geo = new THREE.BufferGeometry();
    geo.setAttribute("position", new THREE.BufferAttribute(new Float32Array(segs), 3));
    geo.setAttribute("aT", new THREE.BufferAttribute(new Float32Array(ts), 1));
    geo.setAttribute("aColor", new THREE.BufferAttribute(new Float32Array(cols), 3));
    const mat = new THREE.ShaderMaterial({ vertexShader: BRIDGE_VS, fragmentShader: BRIDGE_FS, uniforms: { uTime: { value: 0 }, uAlpha: { value: 0.7 } }, ...LIGHT });
    const lines = new THREE.LineSegments(geo, mat);
    lines.frustumCulled = false;
    lines.renderOrder = 6;
    this.skyGroup.add(lines);
    this.skyDisposables.push(geo, mat);
    this.bridges = mat;
  }

  // -- one project's constellation (SKY-3, SKY-4, SKY-7, SKY-8), built on approach ------------
  private buildInterior(repo: string) {
    const sys = this.systems.get(repo);
    if (!sys || sys.c.features.length === 0) return;
    const c = sys.c;
    const R = sys.place.R;
    const layout = layoutInterior(c);
    const group = new THREE.Group();
    group.position.set(sys.place.x, sys.place.y, sys.place.z);
    group.scale.setScalar(R);
    const fadeMats: Interior["fadeMats"] = [];
    const own: { dispose(): void }[] = [];
    const tints = { "1.0": new THREE.Color("#7dd3fc"), "after-1.0": new THREE.Color("#a5b4fc"), "after-2.0": new THREE.Color("#c4b5fd") } as const;

    // Milestone rings: orbital shells.
    for (const ring of layout.rings) {
      const pts: THREE.Vector3[] = [];
      for (let i = 0; i <= 200; i++) {
        const a = (i / 200) * Math.PI * 2;
        pts.push(new THREE.Vector3(ring.radius * Math.cos(a), 0, ring.radius * Math.sin(a)));
      }
      const geo = new THREE.BufferGeometry().setFromPoints(pts);
      const dashed = ring.release !== "1.0";
      const base = ring.release === "1.0" ? 0.4 : ring.release === "after-1.0" ? 0.26 : 0.18;
      const mat = dashed
        ? new THREE.LineDashedMaterial({ color: tints[ring.release], dashSize: 0.016, gapSize: 0.014, opacity: base, ...GLOW })
        : new THREE.LineBasicMaterial({ color: tints[ring.release], opacity: base, ...GLOW });
      const line = new THREE.Line(geo, mat);
      if (dashed) line.computeLineDistances();
      line.renderOrder = 7;
      group.add(line);
      fadeMats.push({ mat, base });
      own.push(geo, mat);
    }

    // Each feature: a faint glow and its figure (minimum spanning tree of its stars).
    const fig: number[] = [];
    const figCol: number[] = [];
    for (const f of c.features) {
      const at = layout.features.get(f.id)!;
      const closed = f.stars.filter((s) => s.state === "closed").length;
      const share = f.stars.length ? closed / f.stars.length : 0;
      if (f.stars.length) {
        const base = 0.1 + 0.18 * share;
        const mat = new THREE.SpriteMaterial({ map: this.glow, color: tints[f.release], opacity: base, ...GLOW });
        const s = new THREE.Sprite(mat);
        s.position.set(at.x, at.y, at.z);
        s.scale.setScalar(Math.max(0.08, at.r * 3.2));
        s.renderOrder = 7;
        group.add(s);
        fadeMats.push({ mat, base });
        own.push(mat);
      }
      const pts = f.stars.map((s) => layout.stars.get(s.number)!);
      for (const [a, b] of spanningTree(pts)) {
        fig.push(pts[a].x, pts[a].y, pts[a].z, pts[b].x, pts[b].y, pts[b].z);
        const t = tints[f.release];
        figCol.push(t.r, t.g, t.b, t.r, t.g, t.b);
      }
    }
    if (fig.length) {
      const geo = new THREE.BufferGeometry();
      geo.setAttribute("position", new THREE.BufferAttribute(new Float32Array(fig), 3));
      geo.setAttribute("color", new THREE.BufferAttribute(new Float32Array(figCol), 3));
      const mat = new THREE.LineBasicMaterial({ vertexColors: true, opacity: 0.42, ...GLOW });
      const lines = new THREE.LineSegments(geo, mat);
      lines.renderOrder = 8;
      group.add(lines);
      fadeMats.push({ mat, base: 0.42 });
      own.push(geo, mat);
    }

    // Issue stars, sized by magnitude, shaped by state.
    const stars: Interior["stars"] = [];
    const edged = new Set(c.edges.flat());
    const sp: number[] = [];
    for (const f of c.features)
      for (const s of f.stars) {
        const p = layout.stars.get(s.number)!;
        sp.push(p.x, p.y, p.z);
        stars.push({ number: s.number, index: stars.length, state: s.state, inFlight: s.inFlight, chain: c.chain.includes(s.number), edged: edged.has(s.number), size: 0 });
      }
    const n = stars.length;
    const starGeo = new THREE.BufferGeometry();
    starGeo.setAttribute("position", new THREE.BufferAttribute(new Float32Array(sp), 3));
    for (const [name, k] of [["aColor", 3], ["aSize", 1], ["aAlpha", 1], ["aKind", 1], ["aCorona", 1], ["aPhase", 1], ["aGlow", 1]] as const)
      starGeo.setAttribute(name, new THREE.BufferAttribute(new Float32Array(n * k), k));
    const all = c.features.flatMap((f) => f.stars);
    const kind = starGeo.getAttribute("aKind") as THREE.BufferAttribute;
    const corona = starGeo.getAttribute("aCorona") as THREE.BufferAttribute;
    const phase = starGeo.getAttribute("aPhase") as THREE.BufferAttribute;
    // A crowded figure's glows add up to a white blot, so each star's glow is shared out.
    const glow = starGeo.getAttribute("aGlow") as THREE.BufferAttribute;
    const crowd = c.features.flatMap((f) => f.stars.map(() => Math.min(1, Math.sqrt(8 / Math.max(1, f.stars.length)))));
    crowd.forEach((g, i) => glow.setX(i, g));
    const pr = seeded(`stars:${repo}`);
    all.forEach((s, i) => {
      const ph = starPhysics(s);
      kind.setX(i, ph.kind === "closed" ? 0 : ph.kind === "open" ? 1 : 2);
      corona.setX(i, ph.corona === "pass" ? 1 : ph.corona === "fail" ? 2 : 0);
      phase.setX(i, pr());
      stars[i].size = ph.magnitude;
    });
    const unif = () => ({ uPx: { value: 900 }, uDpr: { value: this.dpr }, uScale: { value: R }, uTime: { value: 0 }, uPulse: { value: this.reducedMotion ? 0 : 1 }, uFade: { value: 0 } });
    const starMat = new THREE.ShaderMaterial({ vertexShader: STAR_VS, fragmentShader: STAR_FS, uniforms: unif(), ...LIGHT });
    group.add(pointsOf(starGeo, starMat, 10));
    own.push(starGeo, starMat);

    // A feature with no issue: a dark dust knot with a dashed outline.
    const dark = c.features.filter((f) => f.stars.length === 0);
    let darkMat: THREE.ShaderMaterial | null = null;
    if (dark.length) {
      const geo = new THREE.BufferGeometry();
      const dp = new Float32Array(dark.length * 3);
      const ds = new Float32Array(dark.length);
      dark.forEach((f, i) => {
        const at = layout.features.get(f.id)!;
        dp.set([at.x, at.y, at.z], i * 3);
        ds[i] = Math.max(at.r * 2.4, layout.gap * 2.2);
      });
      geo.setAttribute("position", new THREE.BufferAttribute(dp, 3));
      geo.setAttribute("aSize", new THREE.BufferAttribute(ds, 1));
      geo.setAttribute("aAlpha", new THREE.BufferAttribute(new Float32Array(dark.length).fill(1), 1));
      darkMat = new THREE.ShaderMaterial({ vertexShader: DARK_VS, fragmentShader: DARK_FS, uniforms: unif(), ...SHADE });
      group.add(pointsOf(geo, darkMat, 9));
      own.push(geo, darkMat);
    }

    // Blocked-by: a faint filament with light flowing from blocker to blocked.
    const drawn = c.edges.filter(([a, b]) => layout.stars.has(a) && layout.stars.has(b));
    const chainPairs = new Set<string>();
    for (let i = 1; i < c.chain.length; i++) chainPairs.add(`${c.chain[i - 1]}-${c.chain[i]}`);
    let edgeGeo: THREE.BufferGeometry | null = null;
    let flowGeo: THREE.BufferGeometry | null = null;
    let flowMat: THREE.ShaderMaterial | null = null;
    const flowEdge: number[] = [];
    if (drawn.length) {
      edgeGeo = new THREE.BufferGeometry();
      const ep = new Float32Array(drawn.length * 6);
      const fa: number[] = [];
      const fb: number[] = [];
      const fph: number[] = [];
      const hot: number[] = [];
      const fr = seeded(`flow:${repo}`);
      drawn.forEach(([a, b], i) => {
        const pa = layout.stars.get(a)!;
        const pb = layout.stars.get(b)!;
        ep.set([pa.x, pa.y, pa.z, pb.x, pb.y, pb.z], i * 6);
        const isChain = chainPairs.has(`${a}-${b}`);
        const k = isChain ? 12 : 6;
        const off = fr();
        for (let j = 0; j < k; j++) {
          fa.push(pa.x, pa.y, pa.z);
          fb.push(pb.x, pb.y, pb.z);
          fph.push((off + j / k) % 1);
          hot.push(isChain ? 1 : 0);
          flowEdge.push(i);
        }
      });
      edgeGeo.setAttribute("position", new THREE.BufferAttribute(ep, 3));
      edgeGeo.setAttribute("color", new THREE.BufferAttribute(new Float32Array(drawn.length * 6), 3));
      const edgeMat = new THREE.LineBasicMaterial({ vertexColors: true, opacity: 1, ...GLOW });
      const lines = new THREE.LineSegments(edgeGeo, edgeMat);
      lines.renderOrder = 8;
      group.add(lines);
      fadeMats.push({ mat: edgeMat, base: 1 });
      own.push(edgeGeo, edgeMat);

      flowGeo = new THREE.BufferGeometry();
      flowGeo.setAttribute("position", new THREE.BufferAttribute(new Float32Array(fa), 3));
      flowGeo.setAttribute("aB", new THREE.BufferAttribute(new Float32Array(fb), 3));
      flowGeo.setAttribute("aPhase", new THREE.BufferAttribute(new Float32Array(fph), 1));
      flowGeo.setAttribute("aHot", new THREE.BufferAttribute(new Float32Array(hot), 1));
      flowGeo.setAttribute("aHi", new THREE.BufferAttribute(new Float32Array(hot.length).fill(1), 1));
      flowMat = new THREE.ShaderMaterial({
        vertexShader: FLOW_VS,
        fragmentShader: FLOW_FS,
        uniforms: { ...unif(), uFlow: { value: this.reducedMotion ? 0 : 1 }, uSize: { value: layout.gap * 0.35 } },
        ...LIGHT,
      });
      group.add(pointsOf(flowGeo, flowMat, 9));
      own.push(flowGeo, flowMat);
    }

    this.skyGroup.add(group);
    this.interior = { repo, group, layout, R, stars, starGeo, starMat, darkMat, flowGeo, flowMat, flowEdge, edgeGeo, edges: drawn, chainPairs, fadeMats, own, fade: 0 };
    this.paintInterior();
  }

  private dropInterior() {
    if (!this.interior) return;
    this.skyGroup.remove(this.interior.group);
    for (const d of this.interior.own) d.dispose();
    this.interior = null;
  }

  /** Colour, size and alpha of each star and filament, from state, chain, hover, pick, lens. */
  private paintInterior() {
    const it = this.interior;
    if (!it) return;
    const lit = this.focusStar === null ? null : highlightChain(it.edges, this.focusStar);
    const col = it.starGeo.getAttribute("aColor") as THREE.BufferAttribute;
    const size = it.starGeo.getAttribute("aSize") as THREE.BufferAttribute;
    const alpha = it.starGeo.getAttribute("aAlpha") as THREE.BufferAttribute;
    const core = it.layout.gap * 0.4; // a magnitude-1 star's core radius, in ring units
    for (const s of it.stars) {
      let c: [number, number, number] = s.state === "closed" ? [1, 0.9, 0.72] : [0.62, 0.74, 0.98];
      if (s.inFlight) c = [0.45, 1, 0.88];
      let mag = s.size;
      let a = s.inFlight ? 1 : s.state === "closed" ? 1 : 0.62;
      if (s.chain) c = [Math.min(1, c[0] + 0.08), c[1] * 0.9, c[2] * 0.62];
      if (lit) {
        if (lit.has(s.number)) {
          mag *= 1.25;
          a = Math.max(a, 1);
        } else a *= 0.35;
      }
      if (this.picked === s.number) {
        mag *= 1.35;
        a = 1;
      }
      a *= lensStar(this.lens, s);
      col.setXYZ(s.index, c[0], c[1], c[2]);
      // The sprite is drawn larger than the core (corona, spikes, beams): core at 0.28 of it.
      size.setX(s.index, ((core * mag) / 0.28) * 2);
      alpha.setX(s.index, a);
    }
    col.needsUpdate = size.needsUpdate = alpha.needsUpdate = true;

    const edgeLit = (a: number, b: number) => (lit ? (lit.has(a) && lit.has(b) ? 1.6 : 0.3) : 1);
    const lensEdge = this.lens === "blocked" ? 1.8 : this.lens === "natural" ? 1 : 0.3;
    if (it.edgeGeo) {
      const ec = it.edgeGeo.getAttribute("color") as THREE.BufferAttribute;
      it.edges.forEach(([a, b], i) => {
        const chain = it.chainPairs.has(`${a}-${b}`);
        const k = edgeLit(a, b) * lensEdge;
        const from = chain ? [0.35, 0.24, 0.08] : [0.08, 0.11, 0.18];
        const to = chain ? [0.9, 0.66, 0.28] : [0.28, 0.38, 0.55];
        ec.setXYZ(i * 2, from[0] * k, from[1] * k, from[2] * k);
        ec.setXYZ(i * 2 + 1, to[0] * k, to[1] * k, to[2] * k);
      });
      ec.needsUpdate = true;
    }
    if (it.flowGeo) {
      const hi = it.flowGeo.getAttribute("aHi") as THREE.BufferAttribute;
      it.flowEdge.forEach((e, j) => hi.setX(j, edgeLit(it.edges[e][0], it.edges[e][1]) * lensEdge));
      hi.needsUpdate = true;
    }
    this.dirty = true;
  }

  private applyLens() {
    const k = lensDisc(this.lens);
    this.discMat.uniforms.uDim.value = k;
    this.dustMat.uniforms.uDim.value = Math.max(k, 0.6);
    for (const s of this.systems.values()) s.lens = lensSystem(this.lens, systemFacts(s.c));
    if (this.debris) {
      const a = this.debris.geo.getAttribute("aAlpha") as THREE.BufferAttribute;
      const base = this.debris.geo.getAttribute("aBase") as THREE.BufferAttribute;
      const m = lensDebris(this.lens);
      for (let i = 0; i < a.count; i++) a.setX(i, base.getX(i) * m);
      a.needsUpdate = true;
    }
    if (this.bridges) this.bridges.uniforms.uAlpha.value = this.lens === "natural" ? 0.7 : 0.15;
    this.paintInterior();
    this.dirty = true;
  }

  // -- state from the overlay -----------------------------------------------------------------
  /** Open a project (fly in unless the camera already zoomed there), or close it. */
  setOpen(repo: string | null) {
    if (repo === this.open) return;
    const prev = this.open ? this.systems.get(this.open) : undefined;
    this.open = repo;
    this.focusStar = null;
    this.picked = null;
    if (repo === null) {
      // Escape or Back: fly out, unless the camera already zoomed out past the edge.
      const off = prev ? Math.hypot(this.goal.target.x - prev.place.x, this.goal.target.z - prev.place.z) : Infinity;
      if (prev && this.goal.dist < 3.8 * prev.place.R && off < 2.2 * prev.place.R) this.flyTo(this.overviewPose());
    } else {
      const sys = this.systems.get(repo);
      if (!sys) return;
      if (this.interior?.repo !== repo) {
        this.dropInterior();
        this.buildInterior(repo);
      }
      const off = Math.hypot(this.goal.target.x - sys.place.x, this.goal.target.z - sys.place.z);
      if (this.goal.dist > 3.0 * sys.place.R || off > 1.4 * sys.place.R) this.flyTo(this.systemPose(repo));
    }
    this.paintInterior();
    this.dirty = true;
  }

  setHovered(repo: string | null) {
    this.hovered = repo;
    this.dirty = true;
  }

  setFocusStar(n: number | null) {
    if (n === this.focusStar) return;
    this.focusStar = n;
    this.paintInterior();
  }

  setPicked(n: number | null) {
    if (n === this.picked) return;
    this.picked = n;
    this.paintInterior();
  }

  setLens(lens: Lens) {
    if (lens === this.lens) return;
    this.lens = lens;
    this.applyLens();
  }

  setReducedMotion(on: boolean) {
    this.reducedMotion = on;
    const it = this.interior;
    if (it) {
      it.starMat.uniforms.uPulse.value = on ? 0 : 1;
      if (it.flowMat) it.flowMat.uniforms.uFlow.value = on ? 0 : 1;
    }
    if (on) this.completeFlight();
    this.dirty = true;
  }

  // -- the camera -----------------------------------------------------------------------------
  private overDist(): number {
    return fitDistance(GAL_R * 0.9, FOV, this.width / this.height);
  }

  overviewPose(): Pose {
    // Aimed a little below the centre: the near side of a tilted disc looms larger, so this
    // keeps the whole galaxy in the frame.
    return { target: { x: 0, y: -GAL_R * 0.13, z: 0 }, dist: this.overDist(), az: this.goal.az, elev: OVER_ELEV };
  }

  systemPose(repo: string, k = SYS_DIST): Pose {
    const s = this.systems.get(repo)!.place;
    return { target: { x: s.x, y: s.y, z: s.z }, dist: k * s.R, az: this.goal.az, elev: SYS_ELEV };
  }

  /** Fly (about half a second; skippable) to a pose. */
  flyTo(to: Pose) {
    this.moved = true;
    const ms = flightMs(this.cam, to, this.reducedMotion);
    this.goal = { ...to, target: { ...to.target } };
    if (ms <= 0) {
      this.cam = { ...this.goal, target: { ...this.goal.target } };
      this.tween = null;
    } else this.tween = { from: { ...this.cam, target: { ...this.cam.target } }, to: this.goal, start: performance.now(), ms };
    this.lastInput = performance.now();
    this.dirty = true;
  }

  flyOverview() {
    this.flyTo(this.overviewPose());
  }

  flyToSystem(repo: string) {
    if (this.systems.has(repo)) this.flyTo(this.systemPose(repo));
  }

  private interiorWorld(p: Vec3): Vec3 {
    const it = this.interior!;
    const o = it.group.position;
    return { x: o.x + p.x * it.R, y: o.y + p.y * it.R, z: o.z + p.z * it.R };
  }

  flyToFeature(id: string) {
    const it = this.interior;
    const f = it?.layout.features.get(id);
    if (!it || !f) return;
    this.flyTo({ target: this.interiorWorld(f), dist: Math.max(MIN_DIST * 2, f.r * it.R * 7), az: this.goal.az, elev: SYS_ELEV });
  }

  flyToStar(n: number) {
    const it = this.interior;
    const p = it?.layout.stars.get(n);
    if (!it || !p) return;
    this.flyTo({ target: this.interiorWorld(p), dist: Math.max(MIN_DIST * 1.5, it.layout.gap * it.R * 22), az: this.goal.az, elev: SYS_ELEV });
  }

  isFlying(): boolean {
    return this.tween !== null;
  }

  /** Skip to the end of the flight in progress. */
  completeFlight() {
    if (!this.tween) return;
    this.cam = { ...this.tween.to, target: { ...this.tween.to.target } };
    this.goal = { ...this.cam, target: { ...this.cam.target } };
    this.tween = null;
    this.dirty = true;
  }

  /** Inside the open project, deeper than its overview pose (Escape steps back to it first). */
  deeperThanSystem(): boolean {
    const s = this.open ? this.systems.get(this.open) : undefined;
    return !!s && this.goal.dist < 1.25 * s.place.R;
  }

  private takeOver() {
    this.moved = true;
    // Input during a flight takes over from where the camera is.
    if (this.tween) {
      this.goal = { ...this.cam, target: { ...this.cam.target } };
      this.tween = null;
    }
    this.lastInput = performance.now();
    this.dirty = true;
  }

  /** The point on the disc plane (at the open project's height) under a screen point. */
  private planePoint(sx: number, sy: number): Vec3 | null {
    const s = this.open ? this.systems.get(this.open)?.place : undefined;
    const py = s ? s.y : 0;
    const ndc = new THREE.Vector3((sx / this.width) * 2 - 1, 1 - (sy / this.height) * 2, 0.5);
    ndc.unproject(this.camera);
    const o = this.camera.position;
    const dir = ndc.sub(o).normalize();
    if (Math.abs(dir.y) < 1e-4) return null;
    const t = (py - o.y) / dir.y;
    if (t <= 0) return null;
    const p = { x: o.x + dir.x * t, y: py, z: o.z + dir.z * t };
    // Far off the galaxy (a grazing ray): zoom about the centre of the view instead.
    if (Math.hypot(p.x, p.z) > GAL_R * 2.2) return null;
    return p;
  }

  /** Zoom by `factor` (above 1 out) toward a screen point, and enter or leave by distance. */
  zoomAt(factor: number, sx: number, sy: number) {
    this.takeOver();
    const anchor = factor < 1 ? this.planePoint(sx, sy) : null;
    this.goal = zoomToward(this.goal, factor, anchor ?? (factor < 1 ? null : { x: 0, y: 0, z: 0 }), MIN_DIST, this.overDist() * 1.6);
    if (factor > 1 && this.goal.dist > this.overDist() * 0.9) {
      // Backing out to the whole galaxy recentres it.
      const k = Math.min(1, (this.goal.dist - this.overDist() * 0.9) / (this.overDist() * 0.4));
      const home = this.overviewPose().target;
      const t = this.goal.target;
      this.goal.target = { x: t.x + (home.x - t.x) * k, y: t.y + (home.y - t.y) * k, z: t.z + (home.z - t.z) * k };
    }
    this.checkZone();
  }

  orbitBy(dx: number, dy: number) {
    this.takeOver();
    this.goal.az -= dx * 0.005;
    this.goal.elev = Math.max(0.2, Math.min(1.45, this.goal.elev + dy * 0.004));
  }

  private checkZone() {
    const zones = [...this.systems.values()].map((s) => ({ ...s.place, repo: s.c.repo }));
    const z = zoneStep(this.open, this.goal, zones);
    if (z) this.onZone?.(z);
  }

  /** Mark user input (stops the idle drift). */
  poke() {
    this.lastInput = performance.now();
    this.dirty = true;
  }

  resize(width: number, height: number) {
    this.width = Math.max(1, width);
    this.height = Math.max(1, height);
    this.dpr = Math.min(1.5, window.devicePixelRatio || 1);
    this.renderer.setPixelRatio(this.dpr);
    this.renderer.setSize(this.width, this.height, false);
    this.camera.aspect = this.width / this.height;
    this.camera.updateProjectionMatrix();
    // Until the camera has been moved, the overview follows the window's shape.
    if (!this.moved && !this.open && !this.tween) {
      this.cam = this.overviewPose();
      this.goal = { ...this.cam, target: { ...this.cam.target } };
    }
    this.dirty = true;
  }

  /** The camera has left its starting pose (by input, a flight, or the DEV hook). */
  hasMoved(): boolean {
    return this.moved;
  }

  /** DEV only: put the camera somewhere at once (for screenshots without a mouse). */
  devPlace(repo: string, k: number) {
    if (!this.systems.has(repo)) return;
    this.moved = true;
    this.cam = this.systemPose(repo, k);
    this.goal = { ...this.cam, target: { ...this.cam.target } };
    this.tween = null;
    this.dirty = true;
  }

  // -- where things are on screen (CSS pixels in the canvas box) ----------------------------
  private toScreen(v: Vec3, worldR: number): Screen {
    this.tmp.set(v.x, v.y, v.z).project(this.camera);
    const visible = this.tmp.z < 1 && this.tmp.z > -1;
    const x = ((this.tmp.x + 1) / 2) * this.width;
    const y = ((1 - this.tmp.y) / 2) * this.height;
    this.right.set(1, 0, 0).applyQuaternion(this.camera.quaternion).multiplyScalar(worldR).add(this.tmp.set(v.x, v.y, v.z)).project(this.camera);
    const rx = ((this.right.x + 1) / 2) * this.width;
    const ry = ((1 - this.right.y) / 2) * this.height;
    return { x, y, r: Math.hypot(rx - x, ry - y), visible };
  }

  galaxyScreen(repo: string): Screen | null {
    const s = this.systems.get(repo);
    if (!s) return null;
    return this.toScreen(s.place, s.place.centre ? HOLE.discOuter : s.place.scale * 0.8);
  }

  starScreen(n: number): Screen | null {
    const it = this.interior;
    const p = it?.layout.stars.get(n);
    if (!it || !p) return null;
    const mag = it.stars.find((s) => s.number === n)?.size ?? 1;
    return this.toScreen(this.interiorWorld(p), it.layout.gap * 0.4 * mag * it.R);
  }

  featureScreen(id: string): Screen | null {
    const it = this.interior;
    const p = it?.layout.features.get(id);
    return it && p ? this.toScreen(this.interiorWorld(p), p.r * it.R) : null;
  }

  /**
   * Where a milestone ring's name goes: the point on the ring nearest the near-left side that is
   * well inside the view, so all three names show even when a ring is larger than the screen.
   * With no such point, the ring point nearest the view (the overlay clamps it in).
   */
  ringScreen(release: string): Screen | null {
    const it = this.interior;
    const ring = it?.layout.rings.find((r) => r.release === release);
    if (!it || !ring) return null;
    const a0 = Math.PI / 2 - this.cam.az + 0.5;
    const W = this.width;
    const H = this.height;
    let best: Screen | null = null;
    let bestCost = Infinity;
    for (let i = 0; i < 72; i++) {
      const da = ((i % 2 ? 1 : -1) * Math.ceil(i / 2) * Math.PI) / 36;
      const a = a0 + da;
      const sc = this.toScreen(this.interiorWorld({ x: ring.radius * Math.cos(a), y: 0, z: ring.radius * Math.sin(a) }), 0);
      if (!sc.visible) continue;
      const out = Math.max(0, 70 - sc.x, sc.x - (W - 70)) + Math.max(0, 100 - sc.y, sc.y - (H - 90));
      const cost = out * 100 + Math.abs(da);
      if (cost < bestCost) {
        bestCost = cost;
        best = sc;
      }
    }
    return best;
  }

  interiorRepo(): string | null {
    return this.interior?.repo ?? null;
  }

  /** How much of the open constellation shows, 0..1 (the overlay fades its labels with it). */
  detail(): number {
    return this.interior ? this.interior.fade : 0;
  }

  /** The camera's distance over the overview's: 1 at the whole galaxy, small up close. */
  zoomShare(): number {
    return this.cam.dist / this.overDist();
  }

  /** The feature the camera has zoomed in on, if any (for the breadcrumb). */
  nearFeature(): string | null {
    const it = this.interior;
    if (!it || it.repo !== this.open || this.goal.dist > 0.9 * it.R) return null;
    let best: string | null = null;
    let bd = Infinity;
    for (const [id, f] of it.layout.features) {
      const w = this.interiorWorld(f);
      const d = Math.hypot(w.x - this.goal.target.x, w.z - this.goal.target.z);
      if (d < Math.max(f.r * it.R * 2.5, MIN_DIST) && d < bd) {
        bd = d;
        best = id;
      }
    }
    return best;
  }

  // -- the loop -------------------------------------------------------------------------------
  start() {
    if (this.running) return;
    this.running = true;
    const tick = () => {
      if (!this.running) return;
      this.raf = requestAnimationFrame(tick);
      this.frame();
    };
    this.raf = requestAnimationFrame(tick);
  }

  stop() {
    this.running = false;
    cancelAnimationFrame(this.raf);
  }

  /** Force a draw on the next frame (reduced motion draws only when something changed). */
  invalidate() {
    this.dirty = true;
  }

  private frame() {
    const now = performance.now();
    const dt = Math.min(0.05, Math.max(0, (now - this.last) / 1000));
    this.last = now;
    if (typeof document !== "undefined" && document.hidden) return;
    const moving = this.tween !== null || settling(this.cam, this.goal);
    if (this.reducedMotion && !this.dirty && !moving) return;
    const idle = now - this.lastInput;
    // Idle and still: half the frame rate is plenty for slow ambient motion.
    if (!moving && !this.dirty && idle > 3000) {
      this.skip = !this.skip;
      if (this.skip) return;
    }
    this.dirty = false;
    if (!this.reducedMotion) this.time += dt;
    const t = this.time;

    // The camera: a click flight, else easing toward the goal; after a while idle, a slow drift.
    if (this.tween) {
      const k = Math.min(1, (now - this.tween.start) / this.tween.ms);
      this.cam = blendPose(this.tween.from, this.tween.to, easeInOutCubic(k));
      if (k >= 1) {
        this.cam = { ...this.tween.to, target: { ...this.tween.to.target } };
        this.tween = null;
      }
    } else {
      if (!this.reducedMotion && idle > IDLE_MS) this.goal.az += dt * 0.012;
      this.cam = this.reducedMotion ? { ...this.goal, target: { ...this.goal.target } } : dampPose(this.cam, this.goal, dt);
    }
    const eye = poseEye(this.cam);
    this.camera.position.set(eye.x, eye.y, eye.z);
    this.camera.lookAt(this.cam.target.x, this.cam.target.y, this.cam.target.z);
    this.camera.updateMatrixWorld();

    // Build the constellation of the project the camera approaches (lazily), drop it when far.
    const want = this.open ?? this.approached(eye);
    if (want !== (this.interior?.repo ?? null)) {
      this.dropInterior();
      if (want) this.buildInterior(want);
    }

    const pxScale = (this.height * this.dpr) / (2 * Math.tan((FOV * Math.PI) / 360));
    for (const m of [this.discMat, this.dustMat]) {
      m.uniforms.uTime.value = t;
      m.uniforms.uPx.value = pxScale;
      m.uniforms.uDpr.value = this.dpr;
    }
    this.field.uniforms.uTime.value = t;
    this.field.uniforms.uDpr.value = this.dpr;
    const share = this.cam.dist / this.overDist();
    (this.bulge.material as THREE.SpriteMaterial).opacity = (this.hole ? 0.22 : 0.4) * lensDisc(this.lens) * Math.min(1, 0.1 + share * share);

    const it = this.interior;
    for (const s of this.systems.values()) {
      if (!s.mat || !s.core) continue;
      const hover = s.c.repo === this.hovered ? 1.45 : 1;
      const inside = it && it.repo === s.c.repo ? 1 - 0.75 * it.fade : 1;
      s.mat.uniforms.uTime.value = t;
      s.mat.uniforms.uPx.value = pxScale;
      s.mat.uniforms.uDpr.value = this.dpr;
      s.mat.uniforms.uAlpha.value = s.lens * hover * inside;
      const cond = condensation(s.c);
      (s.core.material as THREE.SpriteMaterial).opacity = (0.05 + 0.6 * cond) * s.lens * hover * inside;
    }
    if (this.hole) {
      const d = Math.hypot(eye.x, eye.z);
      this.hole.disc.uniforms.uTime.value = t;
      this.hole.disc.uniforms.uView.value.set(eye.x / Math.max(d, 1e-6), -eye.z / Math.max(d, 1e-6));
      this.hole.lens.uniforms.uTime.value = t;
      this.hole.lensMesh.quaternion.copy(this.camera.quaternion);
      const sw = this.systems.get([...this.systems.values()].find((s) => s.place.centre)!.c.repo)!;
      const hover = sw.c.repo === this.hovered ? 1.3 : 1;
      this.hole.disc.uniforms.uAlpha.value = Math.max(0.35, sw.lens) * hover * Math.min(1, 0.45 + share);
      this.hole.lens.uniforms.uAlpha.value = Math.max(0.35, sw.lens) * hover;
    }
    if (this.debris) {
      this.debris.mat.uniforms.uTime.value = t;
      this.debris.mat.uniforms.uPx.value = pxScale;
      this.debris.mat.uniforms.uDpr.value = this.dpr;
    }
    if (this.bridges) this.bridges.uniforms.uTime.value = t;

    // The constellation fades in by distance, never pops.
    if (it) {
      const o = it.group.position;
      it.fade = detailFade(Math.hypot(eye.x - o.x, eye.y - o.y, eye.z - o.z), it.R);
      it.group.visible = it.fade > 0.001;
      for (const m of [it.starMat, it.darkMat, it.flowMat]) {
        if (!m) continue;
        m.uniforms.uFade.value = it.fade;
        m.uniforms.uTime.value = t;
        m.uniforms.uPx.value = pxScale;
        m.uniforms.uDpr.value = this.dpr;
      }
      for (const f of it.fadeMats) f.mat.opacity = f.base * it.fade;
    }

    this.renderer.render(this.scene, this.camera);
    this.onFrame?.();
  }

  /** The charted project the camera is closing in on, if near enough to start showing it. */
  private approached(eye: Vec3): string | null {
    let best: string | null = null;
    let bd = Infinity;
    for (const s of this.systems.values()) {
      if (s.c.features.length === 0) continue;
      const d = Math.hypot(eye.x - s.place.x, eye.y - s.place.y, eye.z - s.place.z) / s.place.R;
      const off = Math.hypot(this.cam.target.x - s.place.x, this.cam.target.z - s.place.z) / s.place.R;
      if (d < 5.4 && off < 1.6 && d < bd) {
        bd = d;
        best = s.c.repo;
      }
    }
    return best;
  }

  dispose() {
    this.stop();
    this.onFrame = null;
    this.onZone = null;
    this.dropInterior();
    for (const d of this.skyDisposables) d.dispose();
    for (const d of this.disposables) d.dispose();
    this.skyDisposables = [];
    this.disposables = [];
    this.systems.clear();
    this.renderer.dispose();
    this.renderer.forceContextLoss();
  }
}
