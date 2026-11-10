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
 * Gravitational lensing as a thin lens: a pixel at apparent radius r shows the scene from
 * r - E^2 / r (inside the Einstein radius E that is the far side, mirrored), black inside the
 * shadow, and a thin sharp photon ring on its edge. The bend fades out well away from the hole.
 */
const LENS_FS = /* glsl */ `
uniform sampler2D tDiffuse;
uniform vec2 uRes;
uniform vec2 uC;
uniform float uRs;
uniform float uOn;
varying vec2 vUv;
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
  if (x > 1.0) {
    float e2 = 2.2 * smoothstep(14.0, 5.0, x);
    float src = r - uRs * uRs * e2 / max(r, 1e-3);
    vec2 uv = (uC + d / max(r, 1e-3) * src) / uRes;
    col = texture2D(tDiffuse, clamp(uv, vec2(0.0), vec2(1.0))).rgb;
    // Light that skims the horizon piles up into a thin, sharp ring.
    float ring = exp(-pow((x - 1.035) / 0.014, 2.0));
    col += vec3(1.0, 0.86, 0.66) * ring * 1.6 * uOn;
    // The edge of the shadow is soft by a pixel or so, not a cut-out disc.
    col *= smoothstep(1.0, 1.0 + 1.5 / max(uRs, 1.0), x);
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
      uniforms: { tDiffuse: { value: null }, uRes: { value: new THREE.Vector2(1, 1) }, uC: { value: new THREE.Vector2() }, uRs: { value: 0 }, uOn: { value: 0 } },
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
