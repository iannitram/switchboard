// The camera's optics (#297, pass 3): the scene renders into a half-float HDR target, the black
// hole bends that image around its horizon, bright light blooms past a threshold, and a filmic
// curve maps it to the screen with an exposure that follows the zoom. A whisper of dither kills
// banding in the dark gradients; a very mild vignette. No flares, no chromatic tricks.
import * as THREE from "three";
import { EffectComposer } from "three/examples/jsm/postprocessing/EffectComposer.js";
import { RenderPass } from "three/examples/jsm/postprocessing/RenderPass.js";
import { ShaderPass } from "three/examples/jsm/postprocessing/ShaderPass.js";
import { UnrealBloomPass } from "three/examples/jsm/postprocessing/UnrealBloomPass.js";

const VS = /* glsl */ `
varying vec2 vUv;
void main() {
  vUv = uv;
  gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
}`;

/**
 * The black hole on screen. Gravitational lensing as a thin lens: a pixel at apparent radius r
 * shows the scene from r - E^2 / r, so the horizon (radius uRs) grows into a shadow of radius
 * S = uRs * XS, edged by a thin, sharp photon ring. The accretion disc is drawn here too, in
 * screen space from the camera's real angle to the galaxy plane each frame, so it and its lensed
 * image agree and turn as the camera orbits: a hot band, an ellipse that opens and closes, its near
 * side crossing in front of the shadow, white-hot within, orange outward, the approaching side
 * brighter and bluer (Doppler beaming); its far side, hidden behind the hole, bent up into an arc
 * over the top of the shadow (and a faint one under it). Calm: no flicker, no flare.
 */
const LENS_FS = /* glsl */ `
uniform sampler2D tDiffuse;
uniform vec2 uRes;
uniform vec2 uC;
uniform float uRs;
uniform float uOn;
uniform float uDisc;
uniform float uTime;
uniform vec2 uU;
uniform vec2 uV;
uniform float uTilt;
varying vec2 vUv;
const float E2 = 2.2;
const float XS = 2.06;      // the shadow's edge, where r - E2 / r = 1 (in horizon radii)
const float RIN = 2.3;      // the disc's inner edge, just outside the shadow
const float ROUT = 6.0;
const float TILT = 0.1;     // sine of the angle we see the disc at: nearly edge-on
vec3 heat(float k) {
  vec3 c = mix(vec3(0.9, 0.24, 0.05), vec3(1.0, 0.56, 0.2), smoothstep(0.0, 0.35, k));
  c = mix(c, vec3(1.0, 0.84, 0.62), smoothstep(0.3, 0.75, k));
  c = mix(c, vec3(1.0, 0.97, 0.93), smoothstep(0.7, 1.05, k));
  return mix(c, vec3(0.82, 0.9, 1.0), smoothstep(1.05, 1.35, k));
}
// Light from the disc at radius rd (horizon radii); side: +1 coming toward us, -1 going away.
vec3 disc(float rd, float side, float ang) {
  if (rd < RIN || rd > ROUT) return vec3(0.0);
  float temp = pow(rd / RIN, -0.75);
  float beta = 0.36 * sqrt(RIN / rd);
  float D = 1.0 / (1.0 - beta * side);
  float lum = pow(temp, 2.4) * pow(D, 3.0);
  float edge = smoothstep(RIN, RIN * 1.04, rd) * smoothstep(ROUT, RIN * 1.25, rd);
  float grain = 0.86 + 0.14 * sin(ang * 5.0 - log(rd) * 9.0 + uTime * 0.25);
  return heat(temp * D * 0.8) * min(lum, 2.0) * edge * grain;
}
void main() {
  vec2 px = vUv * uRes;
  vec2 d = px - uC;
  float r = length(d);
  float x = r / max(uRs, 1e-3);
  if (uOn < 0.5 || x > 14.0) {
    gl_FragColor = texture2D(tDiffuse, vUv);
    return;
  }
  vec3 col = vec3(0.0);
  float e2 = E2 * smoothstep(14.0, 5.0, x);
  if (x > 1.0) {
    float src = r - uRs * uRs * e2 / max(r, 1e-3);
    vec2 uv = (uC + d / max(r, 1e-3) * src) / uRes;
    col = texture2D(tDiffuse, clamp(uv, vec2(0.0), vec2(1.0))).rgb;
  }
  // The shadow, soft by a pixel at its edge, and the photon ring hugging it.
  float px1 = 1.0 / max(uRs, 1.0);
  col *= smoothstep(XS, XS + 1.5 * px1, x);
  col += vec3(1.0, 0.88, 0.7) * exp(-pow((x - XS - 0.6 * px1) / max(0.02, 0.8 * px1), 2.0)) * 1.6;
  if (uDisc > 0.0) {
    // The disc lies in the galaxy plane and is seen as the camera sees it this frame: uU is its
    // major axis on screen (the approaching side towards +uU), uV points to its far half, and
    // uTilt is how open it is (0 edge-on, 1 face-on). In disc coordinates a pixel is at
    // (a, b / tilt) from the centre.
    vec2 q = d / max(uRs, 1e-3);   // in horizon radii, y up
    float qa = dot(q, uU);
    float qb = dot(q, uV);
    float tilt = max(uTilt, 0.05);
    float sinI = sqrt(max(0.0, 1.0 - uTilt * uTilt));
    float rd = length(vec2(qa, qb / tilt));
    float ang = atan(qb / tilt, qa);
    float side = qa / max(rd, 1e-3) * sinI;
    // The far side, bent round the hole: up over the shadow's far edge as a bright arc and,
    // fainter, under the near edge. Strongest edge-on; face-on it closes into a thin ring.
    float over = x - XS;
    if (over > 0.0) {
      float up = qb / x;
      float along = -qa / x * sinI;
      float arcs = 1.0 - 0.75 * uTilt;
      float rdTop = RIN + over * mix(4.5, 3.0, uTilt);
      vec3 top = disc(rdTop, along, ang) * smoothstep(-0.2 * uTilt, 0.55, up) * 1.5;
      float rdLow = RIN + over * mix(6.0, 3.5, uTilt);
      vec3 low = disc(rdLow, along, ang) * smoothstep(-0.1, 0.7, -up) * 0.45;
      col += (top + low) * uDisc * arcs;
    }
    if (qb < 0.0) {
      // The near half, in front of everything, the shadow included.
      col += disc(rd, side, ang) * uDisc * 1.5;
    } else {
      // The far half, hidden where the shadow covers it.
      col += disc(rd, side, ang) * uDisc * 1.5 * smoothstep(XS, XS + 1.5 * px1, x);
    }
  }
  gl_FragColor = vec4(col, 1.0);
}`;

/** Exposure, ACES filmic curve, a deep-space floor, a mild vignette and interleaved-gradient dither. */
const FINISH_FS = /* glsl */ `
uniform sampler2D tDiffuse;
uniform vec2 uRes;
uniform float uExposure;
varying vec2 vUv;
vec3 aces(vec3 x) {
  return clamp((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14), 0.0, 1.0);
}
float ign(vec2 p) {
  return fract(52.9829189 * fract(dot(p, vec2(0.06711056, 0.00583715))));
}
void main() {
  vec3 hdr = texture2D(tDiffuse, vUv).rgb;
  vec3 c = aces(hdr * uExposure * 1.35);
  // The scene's colours were set as display values: a gentle lift keeps faint light faint
  // without crushing it, where a full sRGB encode would wash the dark sky out.
  c = pow(c, vec3(0.86));
  float aspect = uRes.x / max(uRes.y, 1.0);
  vec2 q = (vUv - vec2(0.5, 0.5)) * vec2(aspect, 1.0);
  float rq = length(q);
  vec3 sky = mix(vec3(0.016, 0.022, 0.05), vec3(0.008, 0.011, 0.026), smoothstep(0.0, 0.9, rq));
  c = c + sky * (1.0 - c);
  c *= 1.0 - 0.14 * smoothstep(0.45, 1.05, rq);
  c += (ign(gl_FragCoord.xy) - 0.5) / 255.0;
  gl_FragColor = vec4(c, 1.0);
}`;

export type Optics = {
  composer: EffectComposer;
  lens: ShaderPass;
  bloom: UnrealBloomPass;
  finish: ShaderPass;
  setSize(width: number, height: number, dpr: number): void;
  dispose(): void;
};

export function makeOptics(renderer: THREE.WebGLRenderer, scene: THREE.Scene, camera: THREE.Camera, width: number, height: number, dpr: number): Optics {
  const size = (n: number) => Math.max(1, Math.round(n * dpr));
  const composer = new EffectComposer(renderer, new THREE.WebGLRenderTarget(size(width), size(height), { type: THREE.HalfFloatType }));
  // The scene pass draws into the read buffer; only that one is multisampled (lines and the
  // point stars keep clean edges), the ping-pong buffer for the later passes stays plain.
  composer.renderTarget2.dispose();
  composer.renderTarget2 = new THREE.WebGLRenderTarget(size(width), size(height), { type: THREE.HalfFloatType, samples: 4 });
  composer.readBuffer = composer.renderTarget2;
  composer.setPixelRatio(dpr);
  composer.setSize(width, height);

  const render = new RenderPass(scene, camera);
  render.clearColor = new THREE.Color(0, 0, 0);
  render.clearAlpha = 0;
  const lens = new ShaderPass(
    new THREE.ShaderMaterial({
      vertexShader: VS,
      fragmentShader: LENS_FS,
      uniforms: {
        tDiffuse: { value: null },
        uRes: { value: new THREE.Vector2(1, 1) },
        uC: { value: new THREE.Vector2() },
        uRs: { value: 0 },
        uOn: { value: 0 },
        uDisc: { value: 0 },
        uTime: { value: 0 },
        uU: { value: new THREE.Vector2(1, 0) },
        uV: { value: new THREE.Vector2(0, 1) },
        uTilt: { value: 0.1 },
      },
    }),
  );
  lens.enabled = false;
  const bloom = new UnrealBloomPass(new THREE.Vector2(width * dpr, height * dpr), 0.62, 0.42, 0.72);
  const finish = new ShaderPass(
    new THREE.ShaderMaterial({
      vertexShader: VS,
      fragmentShader: FINISH_FS,
      uniforms: { tDiffuse: { value: null }, uRes: { value: new THREE.Vector2(1, 1) }, uExposure: { value: 1 } },
    }),
  );
  composer.addPass(render);
  composer.addPass(lens);
  composer.addPass(bloom);
  composer.addPass(finish);

  const setSize = (w: number, h: number, ratio: number) => {
    composer.setPixelRatio(ratio);
    composer.setSize(w, h);
    lens.uniforms.uRes.value.set(w * ratio, h * ratio);
    finish.uniforms.uRes.value.set(w * ratio, h * ratio);
  };
  setSize(width, height, dpr);
  return {
    composer,
    lens,
    bloom,
    finish,
    setSize,
    dispose() {
      lens.material.dispose();
      finish.material.dispose();
      bloom.dispose();
      composer.dispose();
    },
  };
}
