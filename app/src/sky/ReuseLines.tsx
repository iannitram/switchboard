import type { SkyModel } from "./model";
import { project } from "./geometry";

// SKY-6: a dashed line from each project to every project its code map says it reuses.
// Amber when the reused one is Parked or archived. The warning comes from the core (Reuse.warning).
export const REUSE_STROKE = "#94a3b8";
export const WARNING_STROKE = "#d97706";

export type ReuseLine = {
  from: { x: number; y: number };
  to: { x: number; y: number };
  label: string;
  warning: boolean;
};

const nameOf = (repo: string) => repo.slice(repo.lastIndexOf("/") + 1);

// One line per reuse whose target is on the sky and is not the project itself.
export function reuseLines(sky: SkyModel): ReuseLine[] {
  const lines: ReuseLine[] = [];
  for (const c of sky.constellations) {
    for (const r of c.reuses) {
      const target = sky.constellations.find((t) => t.repo === r.repo || nameOf(t.repo) === r.repo);
      if (!target || target === c) continue;
      lines.push({
        from: project(c),
        to: project(target),
        label: `${nameOf(c.repo)} reuses ${nameOf(target.repo)}`,
        warning: r.warning,
      });
    }
  }
  return lines;
}

// Drawn under the constellations, so it is rendered first inside the sky's <svg>.
export function ReuseLines({ lines }: { lines: ReuseLine[] }) {
  return (
    <g>
      {lines.map((l, i) => (
        <line
          key={i}
          data-testid="reuse-line"
          x1={l.from.x}
          y1={l.from.y}
          x2={l.to.x}
          y2={l.to.y}
          strokeDasharray="6 4"
          stroke={l.warning ? WARNING_STROKE : REUSE_STROKE}
          data-warning={l.warning ? "true" : "false"}
        >
          <title>{l.label}</title>
        </line>
      ))}
    </g>
  );
}
