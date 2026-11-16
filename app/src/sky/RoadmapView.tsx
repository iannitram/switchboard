import type { Constellation, Star } from "./model";
import { COLUMN_WIDTH, HEADER_HEIGHT, ROW_HEIGHT, layoutRoadmap, type RoadmapColumn } from "./roadmap";
import { starLook } from "./stars";
import { StarPanel } from "./StarPanel";
import "./roadmap.css";

function heading({ layer, stage }: RoadmapColumn): string {
  if (layer !== undefined) return `Layer ${layer}`;
  return stage ? `${stage.number}. ${stage.name}` : "Unstaged";
}

/**
 * The project Roadmap lens: the same stars and blocked-by links, in development order. With a
 * nodes: block (`nodeLayers`) the columns are dependency layers, then the stages; without one, the
 * stages alone.
 */
export function RoadmapView({ c, picked, onPick }: { c: Constellation; picked: Star | null; onPick: (star: Star | null) => void }) {
  const at = layoutRoadmap(c);
  if (!c.nodeLayers && !c.stages?.length) return <div className="roadmap-lens" role="status">ROADMAP.md has no Development stages table.</div>;
  return (
    <div className="roadmap-lens" aria-label="Roadmap">
      <div className="roadmap-scroll" tabIndex={0} role="region" aria-label="Development stages">
        <div className="roadmap-columns" style={{ width: at.width, minHeight: at.height }}>
          <svg className="roadmap-links" width={at.width} height={at.height} aria-label="Blocked-by links">
            {c.edges.map(([blocker, blocked]) => {
              const a = at.positions.get(blocker);
              const b = at.positions.get(blocked);
              return a && b ? <line key={`${blocker}-${blocked}`} data-testid="blocked-by" data-from={blocker} data-to={blocked} x1={a.x} y1={a.y} x2={b.x} y2={b.y} stroke="currentColor" /> : null;
            })}
          </svg>
          {at.columns.map((column, index) => (
            <section key={index} className="roadmap-column" data-testid="roadmap-column" data-layer={column.layer} data-complete={column.stage?.complete || undefined} style={{ left: column.x, width: COLUMN_WIDTH }}>
              <header>
                <h3>{heading(column)}</h3>
                {column.stage && <p>{column.stage.done} / {column.stage.total} issues{column.stage.complete ? " · Complete" : ""}</p>}
              </header>
              {column.ids.map((id) => {
                const star = at.stars.get(id)!;
                const look = starLook(star);
                return (
                  <button key={id} className="roadmap-star" data-testid="star" data-number={id} data-state={star.state} data-pulse={look.pulse || undefined} aria-pressed={picked?.number === id} aria-label={`#${id} ${star.title}`} onClick={() => onPick(star)} style={{ top: at.positions.get(id)!.y - 18 }}>
                    <span className="roadmap-dot" style={{ opacity: look.opacity }} aria-hidden="true">✦</span>
                    <span><b>#{id}</b> {star.title}<small>{at.featureOf.get(id)?.title}</small></span>
                  </button>
                );
              })}
              {column.dark.map((f, i) => <p key={f.id} className="roadmap-dark" style={{ top: HEADER_HEIGHT + (column.ids.length + i) * ROW_HEIGHT }}>{f.title} · no issues</p>)}
            </section>
          ))}
        </div>
      </div>
      {picked && <aside className="roadmap-panel"><StarPanel star={picked} onClose={() => onPick(null)} /></aside>}
    </div>
  );
}
