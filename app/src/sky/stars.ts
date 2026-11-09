// SKY-4: a star's state at a glance. An open star is dim, a closed star is full, and a star
// with a fleet job in flight pulses, open or closed. The state comes from the core; this only
// names the look.

export const OPEN_OPACITY = 0.35;
export const CLOSED_OPACITY = 1;

export function starLook(star: { state: "open" | "closed"; inFlight: boolean }): { opacity: number; pulse: boolean } {
  return { opacity: star.state === "closed" ? CLOSED_OPACITY : OPEN_OPACITY, pulse: star.inFlight };
}
