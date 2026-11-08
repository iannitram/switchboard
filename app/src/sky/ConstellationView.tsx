import { useEffect } from "react";
import type { Constellation, Star } from "./model";
import { layoutConstellation, RINGS, VIEW } from "./geometry";

// SKY-3: one constellation zoomed in. A cluster per feature on its milestone's ring, a star per
// issue, a dim dark star for a feature with no issue, a line per `Blocked by #N`. The face holds
// no rules; it paints the Constellation as the core sent it.
export function ConstellationView({
  c,
  onBack,
  onStar,
}: {
  c: Constellation;
  onBack: () => void;
  onStar?: (star: Star) => void;
}) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onBack();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onBack]);

  const at = layoutConstellation(c);

  return (
    <section aria-label={`Constellation ${c.repo}`}>
      <button onClick={onBack}>Back to the sky</button>
      <h2>{c.repo}</h2>
      <svg viewBox={`0 0 ${VIEW} ${VIEW}`} role="img" aria-label={c.repo}>
        <circle data-testid="ring" data-milestone="1.0" cx={VIEW / 2} cy={VIEW / 2} r={RINGS["1.0"]} fill="none" stroke="currentColor" strokeOpacity={0.3} />
        <circle data-testid="ring" data-milestone="After 1.0" cx={VIEW / 2} cy={VIEW / 2} r={RINGS["after-1.0"]} fill="none" stroke="currentColor" strokeOpacity={0.3} />
        {c.edges.map(([blocker, blocked]) => {
          const from = at.stars.get(blocker);
          const to = at.stars.get(blocked);
          if (!from || !to) return null;
          return (
            <line
              key={`${blocker}-${blocked}`}
              data-testid="blocked-by"
              data-from={blocker}
              data-to={blocked}
              x1={from.x}
              y1={from.y}
              x2={to.x}
              y2={to.y}
              stroke="currentColor"
            />
          );
        })}
        {c.features.map((f) => {
          const p = at.features.get(f.id)!;
          return (
            <g key={f.id} data-testid="feature" data-feature={f.id} data-release={f.release}>
              <text x={p.x} y={p.y + 30} textAnchor="middle">
                {f.title}
              </text>
              {f.stars.length === 0 ? (
                <circle
                  data-testid="dark-star"
                  cx={p.x}
                  cy={p.y}
                  r={14}
                  fill="currentColor"
                  fillOpacity={0.15}
                  stroke="currentColor"
                  strokeDasharray="3 3"
                />
              ) : (
                f.stars.map((s) => {
                  const q = at.stars.get(s.number)!;
                  return (
                    <circle
                      key={s.number}
                      data-testid="star"
                      data-number={s.number}
                      cx={q.x}
                      cy={q.y}
                      r={10}
                      fill="currentColor"
                      role="button"
                      aria-label={`#${s.number} ${s.title}`}
                      onClick={() => onStar?.(s)}
                    />
                  );
                })
              )}
            </g>
          );
        })}
      </svg>
    </section>
  );
}
