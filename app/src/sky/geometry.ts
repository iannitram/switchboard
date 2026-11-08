// The sky's geometry: where a constellation sits on the view, how big it is, how lit.
// Placement comes from the core (SKY-1); this only turns it into view coordinates.

export const VIEW = 1000;

const CENTRE = VIEW / 2;
const REACH = 450;

/** Radius 0 is the centre; radius 1 is the rim. Size 0 is r 12, size 1 is r 48. */
export function project(c: { radius: number; angle: number; size: number }): { x: number; y: number; r: number } {
  return {
    x: CENTRE + REACH * c.radius * Math.cos(c.angle),
    y: CENTRE + REACH * c.radius * Math.sin(c.angle),
    r: 12 + 36 * c.size,
  };
}

/** Brightness 0..1 to fill opacity 0.25..1 (SKY-2). */
export function glow(brightness: number): number {
  return 0.25 + 0.75 * brightness;
}
