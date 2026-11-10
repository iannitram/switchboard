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

// DEV only: open a galaxy and pick a star at start, for screenshots without a mouse. Set
// VITE_SKY_OPEN=owner/repo and VITE_SKY_STAR=<issue number> in app/.env.development.local
// (git-ignored). In a production build `import.meta.env.DEV` is false and this returns nulls.
export function devSkyStart(): { open: string | null; star: number | null } {
  if (!import.meta.env.DEV || import.meta.env.MODE === "test") return { open: null, star: null };
  const open = (import.meta.env.VITE_SKY_OPEN as string | undefined) || null;
  const star = Number(import.meta.env.VITE_SKY_STAR);
  return { open, star: Number.isInteger(star) && star > 0 ? star : null };
}
