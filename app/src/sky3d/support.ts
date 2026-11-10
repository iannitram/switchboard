/// <reference types="vite/client" />
// Can this window draw the 3D sky, and does the person want motion? Without WebGL (jsdom, or a
// machine without it) the Sky falls back to the flat SVG map.

export function hasWebGL(): boolean {
  if (typeof window === "undefined" || typeof document === "undefined") return false;
  if (/jsdom/i.test(navigator.userAgent)) return false;
  try {
    const canvas = document.createElement("canvas");
    return !!(canvas.getContext("webgl2") ?? canvas.getContext("webgl"));
  } catch {
    return false;
  }
}

export function prefersReducedMotion(): boolean {
  try {
    return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  } catch {
    return false;
  }
}

// DEV only: start the sky somewhere, for screenshots without a mouse. In app/.env.development.local
// (git-ignored): VITE_SKY_OPEN=owner/repo opens a project, VITE_SKY_STAR=<issue> picks a star,
// VITE_SKY_NEAR=owner/repo with VITE_SKY_DIST=<radii> parks the camera near one without opening,
// VITE_SKY_LENS=<lens id> and VITE_SKY_SEARCH=<query> start with a lens or the search open.
// In a production build `import.meta.env.DEV` is false and this returns nothing set.
export type DevStart = { open: string | null; star: number | null; near: string | null; dist: number | null; lens: string | null; search: string | null };
export function devSkyStart(): DevStart {
  const none: DevStart = { open: null, star: null, near: null, dist: null, lens: null, search: null };
  if (!import.meta.env.DEV || import.meta.env.MODE === "test") return none;
  const env = import.meta.env;
  const str = (v: unknown) => (typeof v === "string" && v ? v : null);
  const star = Number(env.VITE_SKY_STAR);
  const dist = Number(env.VITE_SKY_DIST);
  return {
    open: str(env.VITE_SKY_OPEN),
    star: Number.isInteger(star) && star > 0 ? star : null,
    near: str(env.VITE_SKY_NEAR),
    dist: Number.isFinite(dist) && dist > 0 ? dist : null,
    lens: str(env.VITE_SKY_LENS),
    search: str(env.VITE_SKY_SEARCH),
  };
}
