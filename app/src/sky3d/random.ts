// Seeded randomness for the 3D sky. Every galaxy's look comes from a hash of its repo name, so
// the same project always draws the same galaxy, on every machine and every launch.

/** FNV-1a, 32 bits: a stable unsigned hash of a string. */
export function hashString(s: string): number {
  let h = 0x811c9dc5;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 0x01000193);
  }
  return h >>> 0;
}

/** mulberry32: a small, fast PRNG. Returns numbers in [0, 1), the same sequence per seed. */
export function mulberry32(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/** A PRNG seeded by a string (a repo name plus an optional salt). */
export function seeded(key: string): () => number {
  return mulberry32(hashString(key));
}

/** A normal deviate (Box-Muller) from a uniform source. */
export function gauss(rand: () => number): number {
  const u = Math.max(rand(), 1e-9);
  const v = rand();
  return Math.sqrt(-2 * Math.log(u)) * Math.cos(2 * Math.PI * v);
}
