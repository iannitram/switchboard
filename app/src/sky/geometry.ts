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

/** SKY-3: the milestone rings of a zoomed constellation, by radius in view units. */
export const RINGS = { "1.0": 250, "after-1.0": 350, "after-2.0": 450 } as const;

/** Point i of k spread evenly on a ring, point 0 at the top, the rest clockwise on screen. */
export function onRing(i: number, k: number, ring: number, cx = VIEW / 2, cy = VIEW / 2): { x: number; y: number } {
  const a = -Math.PI / 2 + (2 * Math.PI * i) / k;
  return { x: cx + ring * Math.cos(a), y: cy + ring * Math.sin(a) };
}

/** SKY-3: where each feature and each of its stars sits in a zoomed constellation. */
export function layoutConstellation(c: {
  features: { id: string; release: "1.0" | "after-1.0" | "after-2.0"; stars: { number: number }[] }[];
}): { features: Map<string, { x: number; y: number }>; stars: Map<number, { x: number; y: number }> } {
  const features = new Map<string, { x: number; y: number }>();
  const stars = new Map<number, { x: number; y: number }>();
  for (const release of ["1.0", "after-1.0", "after-2.0"] as const) {
    const group = c.features.filter((f) => f.release === release);
    group.forEach((f, i) => {
      const at = onRing(i, group.length, RINGS[release]);
      features.set(f.id, at);
      f.stars.forEach((s, j) => stars.set(s.number, onRing(j, f.stars.length, 40, at.x, at.y)));
    });
  }
  return { features, stars };
}
