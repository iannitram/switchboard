// The camera's fly-in and fly-out. A flight moves a value from 0 (the overview) to 1 (inside
// the open galaxy) or back. Quick by design; instant with reduced motion. A click during a
// flight completes it; Escape during a fly-in reverses it from where it is.

/** A full flight, overview to galaxy, in milliseconds. */
export const FLY_MS = 520;

export function flyDuration(reducedMotion: boolean): number {
  return reducedMotion ? 0 : FLY_MS;
}

export function easeInOutCubic(t: number): number {
  const x = Math.min(1, Math.max(0, t));
  return x < 0.5 ? 4 * x * x * x : 1 - Math.pow(-2 * x + 2, 3) / 2;
}

export type Flight = { from: number; to: 0 | 1; start: number; duration: number };

/** At rest at `at` (0 or 1). */
export function rest(at: 0 | 1): Flight {
  return { from: at, to: at, start: 0, duration: 0 };
}

/** The flight's raw (un-eased) value at `now`, 0..1. */
export function flightValue(f: Flight, now: number): number {
  if (f.duration <= 0 || now >= f.start + f.duration) return f.to;
  if (now <= f.start) return f.from;
  return f.from + (f.to - f.from) * ((now - f.start) / f.duration);
}

export function inFlight(f: Flight, now: number): boolean {
  return f.duration > 0 && now < f.start + f.duration && f.from !== f.to;
}

/** Fly toward `to` from wherever the camera is now; the time scales with the distance left. */
export function flyTo(f: Flight, now: number, to: 0 | 1, reducedMotion: boolean): Flight {
  const from = flightValue(f, now);
  return { from, to, start: now, duration: flyDuration(reducedMotion) * Math.abs(to - from) };
}

/** Skip to the end of the flight. */
export function complete(f: Flight): Flight {
  return rest(f.to);
}
