import type { SkyModel } from "./model";
import { glow, project, VIEW } from "./geometry";
import { ReuseLines, reuseLines } from "./ReuseLines";

// SKY-1 and SKY-2: one constellation per repo, at the place the core gave it, lit by its progress.
// The face holds no rules; it paints the SkyModel as the core sent it.
export function SkyMap({ sky, onOpen }: { sky: SkyModel; onOpen?: (repo: string) => void }) {
  return (
    <svg viewBox={`0 0 ${VIEW} ${VIEW}`} role="img" aria-label="Sky">
      <ReuseLines lines={reuseLines(sky)} />
      {sky.constellations.map((c) => {
        const p = project(c);
        const name = c.repo.slice(c.repo.lastIndexOf("/") + 1);
        return (
          <g
            key={c.repo}
            data-testid="constellation"
            data-repo={c.repo}
            role="button"
            aria-label={c.repo}
            onClick={() => onOpen?.(c.repo)}
          >
            <circle cx={p.x} cy={p.y} r={p.r} fill="currentColor" fillOpacity={glow(c.brightness)} />
            <text x={p.x} y={p.y + p.r + 24} textAnchor="middle">
              {name}
            </text>
            {c.drift.length > 0 && (
              <text data-testid="drift-count" x={p.x + p.r} y={p.y - p.r} textAnchor="start">
                {c.drift.length}
              </text>
            )}
          </g>
        );
      })}
      <text data-testid="ages" x={20} y={VIEW - 20}>
        {sky.priorityAgeDays === null
          ? `snapshot ${sky.ageMinutes} min old · no priority copy`
          : `snapshot ${sky.ageMinutes} min old · priority copy ${sky.priorityAgeDays} days old`}
      </text>
    </svg>
  );
}
