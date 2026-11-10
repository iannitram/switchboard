// The 3D sky's renderer: a deep star field, one procedural galaxy per project, and the opened
// galaxy resolved into its constellations. It draws only; the React overlay (Sky3D.tsx) holds
// the labels, buttons and panels and asks this scene where things are on screen.
import * as THREE from "three";
import type { Constellation, SkyModel } from "../sky/model";
import { highlightChain } from "../sky/chain";
import { starLook } from "../sky/stars";
import { galaxyParticles, galaxyShape, particleCount } from "./galaxy";
import { blendPose, fitDistance, galaxyPlacement, layoutInterior, poseEye, type InteriorLayout, type Pose, type Vec3 } from "./layout";
import { complete, easeInOutCubic, flightValue, flyTo, inFlight, rest, type Flight } from "./motion";
import { gauss, seeded } from "./random";

const FOV = 42;
/** The opened galaxy's outer ring, in world units. */
export const INTERIOR_R = 26;
const OVERVIEW_ELEV = 0.86;
const GALAXY_ELEV = 1.06;
const REUSE = new THREE.Color("#94a3b8");
const WARNING = new THREE.Color("#d97706");

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

export type Screen = { x: number; y: number; r: number; visible: boolean };

type Galaxy = {
  c: Constellation;
  place: Vec3 & { scale: number };
  group: THREE.Group;
  tilt: THREE.Group;
  tiltQ: THREE.Quaternion;
  spinner: THREE.Group;
  points: THREE.Points;
  mat: THREE.ShaderMaterial;
  core: THREE.Sprite;
  spin: number;
  lit: number;
};

type Interior = {
  repo: string;
  group: THREE.Group;
  layout: InteriorLayout;
  stars: { number: number; index: number; state: "open" | "closed"; inFlight: boolean; chain: boolean }[];
  starGeo: THREE.BufferGeometry;
  starMat: THREE.ShaderMaterial;
  edgeGeo: THREE.BufferGeometry | null;
  edgeMat: THREE.LineBasicMaterial;
  edges: [number, number][];
  chainPairs: Set<string>;
  fadeMats: { mat: THREE.Material & { opacity: number }; base: number }[];
};

// ---- shaders ------------------------------------------------------------------------------

const GALAXY_VS = /* glsl */ `
uniform float uSize;
uniform float uPxScale;
attribute float aSize;
attribute vec3 aColor;
varying vec3 vColor;
varying float vFade;
void main() {
  vec4 mv = modelViewMatrix * vec4(position, 1.0);
  gl_Position = projectionMatrix * mv;
  float px = aSize * uSize * uPxScale / max(-mv.z, 0.001);
  vFade = clamp(px / 2.5, 0.2, 1.0);
  gl_PointSize = clamp(px, 1.5, 40.0);
  vColor = aColor;
}`;

const GALAXY_FS = /* glsl */ `
uniform float uAlpha;
varying vec3 vColor;
varying float vFade;
void main() {
  float d = length(gl_PointCoord - 0.5) * 2.0;
  if (d > 1.0) discard;
  float a = exp(-d * d * 5.0) * 0.55 + smoothstep(0.4, 0.0, d) * 0.6;
  gl_FragColor = vec4(vColor * a * uAlpha * vFade, 1.0);
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
  float tw = 0.85 + 0.15 * sin(uTime * (0.6 + aPhase) + aPhase * 40.0);
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

const STAR_VS = /* glsl */ `
uniform float uDpr;
uniform float uTime;
uniform float uPulse;
attribute vec3 aColor;
attribute float aSize;
attribute float aAlpha;
attribute float aKind;
attribute float aFlight;
varying vec3 vColor;
varying float vAlpha;
varying float vKind;
varying float vFlight;
void main() {
  vec4 mv = modelViewMatrix * vec4(position, 1.0);
  gl_Position = projectionMatrix * mv;
  float pulse = aFlight * uPulse * (0.5 + 0.5 * sin(uTime * 4.2));
  gl_PointSize = aSize * uDpr * (1.0 + 0.45 * pulse);
  vColor = aColor;
  vAlpha = aAlpha * (1.0 + 0.5 * pulse);
  vKind = aKind;
  vFlight = aFlight;
}`;

const STAR_FS = /* glsl */ `
uniform float uFade;
varying vec3 vColor;
varying float vAlpha;
varying float vKind;
varying float vFlight;
void main() {
  vec2 p = gl_PointCoord - 0.5;
  float d = length(p) * 2.0;
  if (d > 1.0) discard;
  float a;
  if (vKind < 0.5) {
    a = exp(-d * d * 9.0) * 0.75 + smoothstep(0.26, 0.1, d);
  } else {
    // A dark star: a dim dashed ring.
    float ring = smoothstep(0.1, 0.0, abs(d - 0.6));
    float dash = step(0.0, sin(atan(p.y, p.x) * 6.0));
    a = ring * dash * 0.9 + exp(-d * d * 10.0) * 0.12;
  }
  // A fleet job in flight: a steady ring as well, so it reads without motion.
  a += vFlight * smoothstep(0.07, 0.0, abs(d - 0.86)) * 0.8;
  gl_FragColor = vec4(vColor * a * vAlpha * uFade, 1.0);
}`;

// ---- procedural textures ------------------------------------------------------------------

function glowTexture(): THREE.Texture {
  const c = document.createElement("canvas");
  c.width = c.height = 128;
  const g = c.getContext("2d")!;
  const grad = g.createRadialGradient(64, 64, 0, 64, 64, 64);
  grad.addColorStop(0, "rgba(255,255,255,1)");
  grad.addColorStop(0.15, "rgba(255,255,255,0.55)");
  grad.addColorStop(0.45, "rgba(255,255,255,0.12)");
  grad.addColorStop(1, "rgba(255,255,255,0)");
  g.fillStyle = grad;
  g.fillRect(0, 0, 128, 128);
  const t = new THREE.CanvasTexture(c);
  t.colorSpace = THREE.SRGBColorSpace;
  return t;
}

function nebulaTexture(key: string): THREE.Texture {
  const c = document.createElement("canvas");
  c.width = c.height = 256;
  const g = c.getContext("2d")!;
  const r = seeded(key);
  for (let i = 0; i < 14; i++) {
    const x = 128 + gauss(r) * 34;
    const y = 128 + gauss(r) * 26;
    const rad = 30 + r() * 70;
    const grad = g.createRadialGradient(x, y, 0, x, y, rad);
    grad.addColorStop(0, `rgba(255,255,255,${0.1 + r() * 0.12})`);
    grad.addColorStop(1, "rgba(255,255,255,0)");
    g.fillStyle = grad;
    g.fillRect(0, 0, 256, 256);
  }
  const t = new THREE.CanvasTexture(c);
  t.colorSpace = THREE.SRGBColorSpace;
  return t;
}

// ---- the scene ----------------------------------------------------------------------------

export class SkyScene {
  readonly renderer: THREE.WebGLRenderer;
  private scene = new THREE.Scene();
  private camera = new THREE.PerspectiveCamera(FOV, 1, 0.1, 5000);
  private galaxies = new Map<string, Galaxy>();
  private interior: Interior | null = null;
  private skyGroup = new THREE.Group();
  private reuseGroup = new THREE.Group();
  private field: THREE.ShaderMaterial;
  private glow: THREE.Texture;
  private disposables: { dispose(): void }[] = [];
  private skyDisposables: { dispose(): void }[] = [];
  private width = 1;
  private height = 1;
  private dpr = 1;
  private raf = 0;
  private running = false;
  private dirty = true;
  private clock0 = performance.now();
  private flight: Flight = rest(0);
  private open: string | null = null;
  private hovered: string | null = null;
  private focusStar: number | null = null;
  private picked: number | null = null;
  private userAz = 0;
  private userElev = 0;
  private zoom = 1;
  private blend = 0;
  private tmp = new THREE.Vector3();
  private right = new THREE.Vector3();
  /** Called after each drawn frame: the overlay repositions itself. */
  onFrame: (() => void) | null = null;

  constructor(
    canvas: HTMLCanvasElement,
    private reducedMotion: boolean,
  ) {
    this.renderer = new THREE.WebGLRenderer({ canvas, antialias: true, alpha: true, powerPreference: "default" });
    this.renderer.setClearColor(0x000000, 0);
    this.glow = glowTexture();
    this.disposables.push(this.glow);
    this.field = this.buildField();
    this.buildNebulae();
    this.scene.add(this.skyGroup);
    this.skyGroup.add(this.reuseGroup);
  }

  // -- background: two star layers (far shell, nearer dust for parallax) and soft nebulae --
  private buildField(): THREE.ShaderMaterial {
    const r = seeded("switchboard-sky-field");
    const far = 3200;
    const near = 1400;
    const n = far + near;
    const pos = new Float32Array(n * 3);
    const col = new Float32Array(n * 3);
    const size = new Float32Array(n);
    const phase = new Float32Array(n);
    for (let i = 0; i < n; i++) {
      let x: number;
      let y: number;
      let z: number;
      if (i < far) {
        const u = r() * 2 - 1;
        const t = r() * Math.PI * 2;
        const s = Math.sqrt(1 - u * u);
        const rad = 1400 + r() * 900;
        x = rad * s * Math.cos(t);
        y = rad * u;
        z = rad * s * Math.sin(t);
      } else {
        const rad = 160 + Math.pow(r(), 0.7) * 520;
        const t = r() * Math.PI * 2;
        x = rad * Math.cos(t);
        z = rad * Math.sin(t);
        y = gauss(r) * 70 - 40;
      }
      pos.set([x, y, z], i * 3);
      const temp = r();
      const c = temp < 0.15 ? [1, 0.82, 0.66] : temp < 0.55 ? [0.78, 0.86, 1] : [0.92, 0.94, 1];
      const b = i < far ? 0.35 + Math.pow(r(), 3) * 0.65 : 0.25 + r() * 0.35;
      col.set([c[0] * b, c[1] * b, c[2] * b], i * 3);
      size[i] = i < far ? 1.2 + Math.pow(r(), 6) * 2.6 : 1.3 + r() * 1.4;
      phase[i] = r();
    }
    const geo = new THREE.BufferGeometry();
    geo.setAttribute("position", new THREE.BufferAttribute(pos, 3));
    geo.setAttribute("aColor", new THREE.BufferAttribute(col, 3));
    geo.setAttribute("aSize", new THREE.BufferAttribute(size, 1));
    geo.setAttribute("aPhase", new THREE.BufferAttribute(phase, 1));
    const mat = new THREE.ShaderMaterial({
      vertexShader: FIELD_VS,
      fragmentShader: FIELD_FS,
      uniforms: { uDpr: { value: 1 }, uTime: { value: 0 } },
      ...LIGHT,
    });
    const pts = new THREE.Points(geo, mat);
    pts.frustumCulled = false;
    this.scene.add(pts);
    this.disposables.push(geo, mat);
    return mat;
  }

  private buildNebulae() {
    const r = seeded("switchboard-sky-nebulae");
    const tints = ["#1e3a8a", "#4c1d95", "#0e7490", "#312e81", "#701a75", "#134e4a"];
    for (let i = 0; i < 7; i++) {
      const tex = nebulaTexture(`nebula-${i}`);
      const mat = new THREE.SpriteMaterial({
        map: tex,
        color: new THREE.Color(tints[i % tints.length]),
        opacity: 0.32 + r() * 0.2,
        ...GLOW,
      });
      const s = new THREE.Sprite(mat);
      const t = (i / 7) * Math.PI * 2 + r() * 0.6;
      const rad = 700 + r() * 500;
      s.position.set(rad * Math.cos(t), -260 + r() * 300, rad * Math.sin(t));
      const k = 900 + r() * 700;
      s.scale.set(k * (1.2 + r() * 0.6), k, 1);
      this.scene.add(s);
      this.disposables.push(tex, mat);
    }
  }

  // -- the projects --------------------------------------------------------------------------
  setSky(sky: SkyModel) {
    for (const d of this.skyDisposables) d.dispose();
    this.skyDisposables = [];
    for (const g of this.galaxies.values()) this.skyGroup.remove(g.group);
    this.galaxies.clear();
    this.reuseGroup.clear();

    for (const c of sky.constellations) {
      const place = galaxyPlacement(c);
      const shape = galaxyShape(c.repo);
      const p = galaxyParticles(c.repo, shape, particleCount(c.size));
      const geo = new THREE.BufferGeometry();
      geo.setAttribute("position", new THREE.BufferAttribute(p.positions, 3));
      geo.setAttribute("aColor", new THREE.BufferAttribute(p.colors, 3));
      geo.setAttribute("aSize", new THREE.BufferAttribute(p.sizes, 1));
      const lit = 0.6 + 0.4 * Math.min(1, Math.max(0, c.brightness));
      const mat = new THREE.ShaderMaterial({
        vertexShader: GALAXY_VS,
        fragmentShader: GALAXY_FS,
        uniforms: { uSize: { value: 0.03 }, uPxScale: { value: 500 }, uAlpha: { value: lit } },
        ...LIGHT,
      });
      const points = new THREE.Points(geo, mat);
      points.frustumCulled = false;
      const spinner = new THREE.Group();
      spinner.add(points);
      const tilt = new THREE.Group();
      const tiltQ = new THREE.Quaternion().setFromEuler(new THREE.Euler(shape.tiltX, 0, shape.tiltZ));
      tilt.quaternion.copy(tiltQ);
      tilt.add(spinner);
      const coreMat = new THREE.SpriteMaterial({
        map: this.glow,
        color: new THREE.Color(1, 0.84, 0.62),
        opacity: 0.25 + 0.75 * c.brightness,
        ...GLOW,
      });
      const core = new THREE.Sprite(coreMat);
      const group = new THREE.Group();
      group.position.set(place.x, place.y, place.z);
      group.add(tilt, core);
      // SKY-5: a small amber flag on a project with drift.
      if (c.drift.length > 0) {
        const flagMat = new THREE.SpriteMaterial({
          map: this.glow,
          color: new THREE.Color("#f5a524"),
          opacity: 0.9,
          ...GLOW,
        });
        const flag = new THREE.Sprite(flagMat);
        flag.position.set(place.scale * 0.85, place.scale * 0.45, 0);
        flag.scale.setScalar(Math.max(1.6, place.scale * 0.22));
        flag.name = "drift-flag";
        group.add(flag);
        this.skyDisposables.push(flagMat);
      }
      this.skyGroup.add(group);
      this.skyDisposables.push(geo, mat, coreMat);
      this.galaxies.set(c.repo, { c, place, group, tilt, tiltQ, spinner, points, mat, core, spin: shape.spin, lit });
    }

    // SKY-6: dashed reuse lines, amber when the reused project is Parked or archived.
    const nameOf = (repo: string) => repo.slice(repo.lastIndexOf("/") + 1);
    for (const c of sky.constellations) {
      for (const r of c.reuses) {
        const target = sky.constellations.find((t) => t.repo === r.repo || nameOf(t.repo) === r.repo);
        if (!target || target === c) continue;
        const a = this.galaxies.get(c.repo)!.place;
        const b = this.galaxies.get(target.repo)!.place;
        const geo = new THREE.BufferGeometry().setFromPoints([new THREE.Vector3(a.x, a.y, a.z), new THREE.Vector3(b.x, b.y, b.z)]);
        const mat = new THREE.LineDashedMaterial({
          color: r.warning ? WARNING : REUSE,
          dashSize: 2.2,
          gapSize: 1.6,
          opacity: r.warning ? 0.85 : 0.5,
          ...GLOW,
        });
        const line = new THREE.Line(geo, mat);
        line.computeLineDistances();
        this.reuseGroup.add(line);
        this.skyDisposables.push(geo, mat);
      }
    }

    // A galaxy that left the sky closes.
    if (this.interior && !this.galaxies.has(this.interior.repo)) this.dropInterior();
    else if (this.interior) {
      const repo = this.interior.repo;
      this.dropInterior();
      this.buildInterior(repo);
    }
    this.dirty = true;
  }

  // -- the opened galaxy (SKY-3, SKY-4, SKY-7) ----------------------------------------------
  private buildInterior(repo: string) {
    const g = this.galaxies.get(repo);
    if (!g) return;
    const c = g.c;
    const layout = layoutInterior(c);
    const group = new THREE.Group();
    group.position.set(g.place.x, g.place.y, g.place.z);
    group.scale.setScalar(INTERIOR_R);
    const fadeMats: Interior["fadeMats"] = [];
    const own: { dispose(): void }[] = [];

    // A ring for each milestone.
    const ringColors = { "1.0": "#7dd3fc", "after-1.0": "#a5b4fc", "after-2.0": "#c4b5fd" } as const;
    for (const ring of layout.rings) {
      const pts: THREE.Vector3[] = [];
      for (let i = 0; i <= 160; i++) {
        const a = (i / 160) * Math.PI * 2;
        pts.push(new THREE.Vector3(ring.radius * Math.cos(a), 0, ring.radius * Math.sin(a)));
      }
      const geo = new THREE.BufferGeometry().setFromPoints(pts);
      const dashed = ring.release !== "1.0";
      const base = ring.release === "1.0" ? 0.42 : ring.release === "after-1.0" ? 0.3 : 0.22;
      const mat = dashed
        ? new THREE.LineDashedMaterial({ color: ringColors[ring.release], dashSize: 0.018, gapSize: 0.014, opacity: base, ...GLOW })
        : new THREE.LineBasicMaterial({ color: ringColors[ring.release], opacity: base, ...GLOW });
      const line = new THREE.Line(geo, mat);
      if (dashed) line.computeLineDistances();
      group.add(line);
      fadeMats.push({ mat, base });
      own.push(geo, mat);
    }

    // A soft glow under each feature's cluster, lit by how much of it is closed.
    for (const f of c.features) {
      const at = layout.features.get(f.id)!;
      const closed = f.stars.filter((s) => s.state === "closed").length;
      const share = f.stars.length ? closed / f.stars.length : 0;
      const tint = f.release === "1.0" ? "#60a5fa" : f.release === "after-1.0" ? "#818cf8" : "#a78bfa";
      const base = f.stars.length ? 0.16 + 0.3 * share : 0.08;
      const mat = new THREE.SpriteMaterial({ map: this.glow, color: new THREE.Color(tint), opacity: base, ...GLOW });
      const s = new THREE.Sprite(mat);
      s.position.set(at.x, at.y, at.z);
      s.scale.setScalar(Math.max(0.09, at.r * 3.4));
      group.add(s);
      fadeMats.push({ mat, base });
      own.push(mat);
    }

    // Stars and dark stars in one Points object.
    const stars: Interior["stars"] = [];
    const entries: { p: Vec3; kind: number }[] = [];
    for (const f of c.features) {
      if (f.stars.length === 0) {
        entries.push({ p: layout.features.get(f.id)!, kind: 1 });
        continue;
      }
      for (const s of f.stars) {
        stars.push({ number: s.number, index: entries.length, state: s.state, inFlight: s.inFlight, chain: c.chain.includes(s.number) });
        entries.push({ p: layout.stars.get(s.number)!, kind: 0 });
      }
    }
    const n = entries.length;
    const starGeo = new THREE.BufferGeometry();
    const pos = new Float32Array(n * 3);
    const kind = new Float32Array(n);
    const flightA = new Float32Array(n);
    entries.forEach((e, i) => {
      pos.set([e.p.x, e.p.y, e.p.z], i * 3);
      kind[i] = e.kind;
    });
    for (const s of stars) flightA[s.index] = s.inFlight ? 1 : 0;
    starGeo.setAttribute("position", new THREE.BufferAttribute(pos, 3));
    starGeo.setAttribute("aColor", new THREE.BufferAttribute(new Float32Array(n * 3), 3));
    starGeo.setAttribute("aSize", new THREE.BufferAttribute(new Float32Array(n), 1));
    starGeo.setAttribute("aAlpha", new THREE.BufferAttribute(new Float32Array(n), 1));
    starGeo.setAttribute("aKind", new THREE.BufferAttribute(kind, 1));
    starGeo.setAttribute("aFlight", new THREE.BufferAttribute(flightA, 1));
    const starMat = new THREE.ShaderMaterial({
      vertexShader: STAR_VS,
      fragmentShader: STAR_FS,
      uniforms: { uDpr: { value: this.dpr }, uTime: { value: 0 }, uPulse: { value: this.reducedMotion ? 0 : 1 }, uFade: { value: 0 } },
      ...LIGHT,
    });
    const starPts = new THREE.Points(starGeo, starMat);
    starPts.frustumCulled = false;
    starPts.renderOrder = 2;
    group.add(starPts);
    own.push(starGeo, starMat);

    // A line for each `Blocked by #N` between two drawn stars.
    const drawn = c.edges.filter(([a, b]) => layout.stars.has(a) && layout.stars.has(b));
    let edgeGeo: THREE.BufferGeometry | null = null;
    const edgeMat = new THREE.LineBasicMaterial({ vertexColors: true, opacity: 0, ...GLOW });
    own.push(edgeMat);
    if (drawn.length) {
      edgeGeo = new THREE.BufferGeometry();
      const ep = new Float32Array(drawn.length * 6);
      drawn.forEach(([a, b], i) => {
        const pa = layout.stars.get(a)!;
        const pb = layout.stars.get(b)!;
        ep.set([pa.x, pa.y, pa.z, pb.x, pb.y, pb.z], i * 6);
      });
      edgeGeo.setAttribute("position", new THREE.BufferAttribute(ep, 3));
      edgeGeo.setAttribute("color", new THREE.BufferAttribute(new Float32Array(drawn.length * 6), 3));
      const lines = new THREE.LineSegments(edgeGeo, edgeMat);
      lines.renderOrder = 1;
      group.add(lines);
      own.push(edgeGeo);
    }
    fadeMats.push({ mat: edgeMat, base: 1 });

    const chainPairs = new Set<string>();
    for (let i = 1; i < c.chain.length; i++) chainPairs.add(`${c.chain[i - 1]}-${c.chain[i]}`);

    this.scene.add(group);
    this.interior = { repo, group, layout, stars, starGeo, starMat, edgeGeo, edgeMat, edges: drawn, chainPairs, fadeMats };
    this.interiorOwn = own;
    this.paintInterior();
  }

  private interiorOwn: { dispose(): void }[] = [];

  private dropInterior() {
    if (!this.interior) return;
    this.scene.remove(this.interior.group);
    for (const d of this.interiorOwn) d.dispose();
    this.interiorOwn = [];
    this.interior = null;
  }

  /** Colour, size and alpha of each star and line, from state, chain, hover and pick. */
  private paintInterior() {
    const it = this.interior;
    if (!it) return;
    const lit = this.focusStar === null ? null : highlightChain(it.edges, this.focusStar);
    const col = it.starGeo.getAttribute("aColor") as THREE.BufferAttribute;
    const size = it.starGeo.getAttribute("aSize") as THREE.BufferAttribute;
    const alpha = it.starGeo.getAttribute("aAlpha") as THREE.BufferAttribute;
    const kind = it.starGeo.getAttribute("aKind") as THREE.BufferAttribute;
    for (let i = 0; i < kind.count; i++) {
      if (kind.getX(i) === 1) {
        col.setXYZ(i, 0.6, 0.68, 0.86);
        size.setX(i, 15);
        alpha.setX(i, lit ? 0.3 : 0.6);
      }
    }
    for (const s of it.stars) {
      const look = starLook(s);
      let c: [number, number, number] = s.state === "closed" ? [1, 0.93, 0.78] : [0.62, 0.74, 0.95];
      if (s.inFlight) c = [0.4, 0.95, 0.85];
      let sz = s.state === "closed" ? 13 : 12;
      let a = look.opacity;
      if (s.chain) {
        c = [Math.min(1, c[0] + 0.1), c[1] * 0.92, c[2] * 0.6];
        sz += 2;
      }
      if (lit) {
        if (lit.has(s.number)) {
          sz += 4;
          a = Math.max(a, 0.95);
        } else a *= 0.4;
      }
      if (this.picked === s.number) {
        sz += 6;
        a = 1;
      }
      col.setXYZ(s.index, c[0], c[1], c[2]);
      size.setX(s.index, sz);
      alpha.setX(s.index, a);
    }
    col.needsUpdate = size.needsUpdate = alpha.needsUpdate = true;

    if (it.edgeGeo) {
      const ec = it.edgeGeo.getAttribute("color") as THREE.BufferAttribute;
      it.edges.forEach(([a, b], i) => {
        let c = [0.24, 0.32, 0.46];
        if (it.chainPairs.has(`${a}-${b}`)) c = [0.85, 0.62, 0.22];
        if (lit) c = lit.has(a) && lit.has(b) ? [0.75, 0.92, 1] : [c[0] * 0.4, c[1] * 0.4, c[2] * 0.4];
        ec.setXYZ(i * 2, c[0], c[1], c[2]);
        ec.setXYZ(i * 2 + 1, c[0], c[1], c[2]);
      });
      ec.needsUpdate = true;
    }
    this.dirty = true;
  }

  // -- state from the overlay -----------------------------------------------------------------
  setOpen(repo: string | null) {
    if (repo === this.open) return;
    const now = performance.now();
    if (repo === null) {
      this.open = null;
      this.flight = flyTo(this.flight, now, 0, this.reducedMotion);
    } else {
      if (this.interior && this.interior.repo !== repo) {
        this.dropInterior();
        this.flight = rest(0);
      }
      this.open = repo;
      this.zoom = 1;
      this.userElev = 0;
      if (!this.interior) this.buildInterior(repo);
      this.flight = flyTo(this.flight, now, 1, this.reducedMotion);
    }
    this.focusStar = null;
    this.picked = null;
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

  setReducedMotion(on: boolean) {
    this.reducedMotion = on;
    if (this.interior) this.interior.starMat.uniforms.uPulse.value = on ? 0 : 1;
    if (on) this.flight = complete(this.flight);
    this.dirty = true;
  }

  isFlying(): boolean {
    return inFlight(this.flight, performance.now());
  }

  /** Skip to the end of the flight in progress. */
  completeFlight() {
    this.flight = complete(this.flight);
    this.dirty = true;
  }

  /** The open-ness of the view, 0 (overview) to 1 (inside a galaxy), eased. */
  openBlend(): number {
    return this.blend;
  }

  orbitBy(dx: number, dy: number) {
    this.userAz = Math.max(-0.5, Math.min(0.5, this.userAz - dx * 0.004));
    this.userElev = Math.max(-0.32, Math.min(0.32, this.userElev + dy * 0.003));
    this.dirty = true;
  }

  zoomBy(deltaY: number) {
    this.zoom = Math.max(0.55, Math.min(1.45, this.zoom * Math.exp(deltaY * 0.0012)));
    this.dirty = true;
  }

  resize(width: number, height: number) {
    this.width = Math.max(1, width);
    this.height = Math.max(1, height);
    this.dpr = Math.min(2, window.devicePixelRatio || 1);
    this.renderer.setPixelRatio(this.dpr);
    this.renderer.setSize(this.width, this.height, false);
    this.camera.aspect = this.width / this.height;
    this.camera.updateProjectionMatrix();
    this.field.uniforms.uDpr.value = this.dpr;
    if (this.interior) this.interior.starMat.uniforms.uDpr.value = this.dpr;
    this.dirty = true;
  }

  // -- where things are on screen (CSS pixels in the canvas box) ----------------------------
  private toScreen(v: THREE.Vector3, worldR: number): Screen {
    this.tmp.copy(v).project(this.camera);
    const visible = this.tmp.z < 1 && this.tmp.z > -1;
    const x = ((this.tmp.x + 1) / 2) * this.width;
    const y = ((1 - this.tmp.y) / 2) * this.height;
    this.right.set(1, 0, 0).applyQuaternion(this.camera.quaternion).multiplyScalar(worldR).add(v).project(this.camera);
    const rx = ((this.right.x + 1) / 2) * this.width;
    const ry = ((1 - this.right.y) / 2) * this.height;
    return { x, y, r: Math.hypot(rx - x, ry - y), visible };
  }

  galaxyScreen(repo: string): Screen | null {
    const g = this.galaxies.get(repo);
    if (!g) return null;
    return this.toScreen(g.group.position, g.place.scale);
  }

  private interiorPoint(p: Vec3, r: number): Screen | null {
    const it = this.interior;
    if (!it) return null;
    const v = new THREE.Vector3(p.x, p.y, p.z).multiplyScalar(INTERIOR_R).add(it.group.position);
    return this.toScreen(v, r * INTERIOR_R);
  }

  starScreen(n: number): Screen | null {
    const p = this.interior?.layout.stars.get(n);
    return p ? this.interiorPoint(p, 0.012) : null;
  }

  featureScreen(id: string): Screen | null {
    const p = this.interior?.layout.features.get(id);
    return p ? this.interiorPoint(p, p.r) : null;
  }

  /** A point on a milestone ring at angle `a`, for the ring's name. */
  ringScreen(release: string, a: number): Screen | null {
    const ring = this.interior?.layout.rings.find((r) => r.release === release);
    return ring ? this.interiorPoint({ x: ring.radius * Math.cos(a), y: 0, z: ring.radius * Math.sin(a) }, 0) : null;
  }

  interiorRepo(): string | null {
    return this.interior?.repo ?? null;
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
    const flying = inFlight(this.flight, now);
    if (this.reducedMotion && !this.dirty && !flying) return;
    this.dirty = false;
    const t = this.reducedMotion ? 0 : (now - this.clock0) / 1000;
    const e = easeInOutCubic(flightValue(this.flight, now));
    this.blend = e;
    if (!flying && this.flight.to === 0 && this.interior && e === 0) this.dropInterior();

    // Camera: the overview pose blended to the opened galaxy's pose.
    const aspect = this.width / this.height;
    const sway = this.reducedMotion ? 0 : 0.07 * Math.sin((t * 2 * Math.PI) / 90);
    const az = sway + this.userAz;
    const over: Pose = {
      target: { x: 0, y: -4, z: 4 },
      dist: fitDistance(104, FOV, aspect) * 0.74 * this.zoom,
      az,
      elev: OVERVIEW_ELEV + this.userElev,
    };
    let pose = over;
    const it = this.interior;
    const g = it ? this.galaxies.get(it.repo) : undefined;
    if (g) {
      const inside: Pose = {
        target: { x: g.place.x, y: g.place.y, z: g.place.z },
        dist: fitDistance(INTERIOR_R * 1.12, FOV, aspect) * (this.open ? this.zoom : 1),
        az,
        elev: GALAXY_ELEV + this.userElev * 0.5,
      };
      pose = blendPose(over, inside, e);
    }
    const eye = poseEye(pose);
    this.camera.position.set(eye.x, eye.y, eye.z);
    this.camera.lookAt(pose.target.x, pose.target.y, pose.target.z);
    this.camera.updateMatrixWorld();

    // Galaxies: spin, light, hover; the opened one flattens and spreads into its constellation.
    const pxScale = (this.height * this.dpr) / (2 * Math.tan((FOV * Math.PI) / 360));
    for (const gal of this.galaxies.values()) {
      const isOpen = !!it && gal.c.repo === it.repo;
      const hover = gal.c.repo === this.hovered && e < 0.5 ? 1.35 : 1;
      let alpha = gal.lit * hover * (1 - 0.88 * e);
      let scale = gal.place.scale;
      gal.tilt.quaternion.copy(gal.tiltQ);
      if (isOpen) {
        alpha = gal.lit * (1 - 0.84 * e);
        scale = gal.place.scale + (INTERIOR_R * 1.18 - gal.place.scale) * e;
        gal.tilt.quaternion.slerpQuaternions(gal.tiltQ, new THREE.Quaternion(), e);
      }
      gal.spinner.rotation.y = gal.spin * t;
      gal.points.scale.setScalar(scale);
      gal.mat.uniforms.uAlpha.value = alpha;
      // Opened, the dust spreads but stays fine, so the constellation reads above it.
      gal.mat.uniforms.uSize.value = 0.03 * (gal.place.scale + (scale - gal.place.scale) * 0.3);
      gal.mat.uniforms.uPxScale.value = pxScale;
      const coreMat = gal.core.material as THREE.SpriteMaterial;
      coreMat.opacity = (0.22 + 0.7 * gal.c.brightness) * hover * (1 - (isOpen ? 0.85 : 0.9) * e);
      gal.core.scale.setScalar(scale * (isOpen ? 0.7 : 0.95));
      const flag = gal.group.getObjectByName("drift-flag") as THREE.Sprite | undefined;
      if (flag) (flag.material as THREE.SpriteMaterial).opacity = 0.9 * (1 - e);
    }
    for (const line of this.reuseGroup.children as THREE.Line[]) {
      const m = line.material as THREE.LineDashedMaterial;
      m.visible = e < 0.98;
    }

    // The constellation fades in over the second half of the flight.
    if (it) {
      const fade = Math.min(1, Math.max(0, (e - 0.35) / 0.65));
      it.group.visible = fade > 0;
      it.starMat.uniforms.uFade.value = fade;
      it.starMat.uniforms.uTime.value = t;
      for (const f of it.fadeMats) f.mat.opacity = f.base * fade;
    }
    this.field.uniforms.uTime.value = t;

    this.renderer.render(this.scene, this.camera);
    this.onFrame?.();
  }

  dispose() {
    this.stop();
    this.onFrame = null;
    this.dropInterior();
    for (const d of this.skyDisposables) d.dispose();
    for (const d of this.disposables) d.dispose();
    this.skyDisposables = [];
    this.disposables = [];
    this.galaxies.clear();
    this.renderer.dispose();
    this.renderer.forceContextLoss();
  }
}
