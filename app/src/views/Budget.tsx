import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { call, type CoreError } from "../core";
import { Empty } from "./Empty";

// BUD-1 (#24): the Budget panel. One row per pool, the JSON of core's PoolRow (#23).
export type PoolRow = {
  pool: string; pct: number | null; pace: number | null; delta: number | null;
  verdict: string; reading_age_min: number | null; stale: boolean; why: string;
};

const clamp = (n: number) => Math.min(100, Math.max(0, n));
const pctText = (n: number | null) => (n == null ? "not recorded" : n.toFixed(1) + "%");
const deltaText = (n: number | null) =>
  n == null ? "not recorded" : (n >= 0 ? "+" : "") + n.toFixed(1) + " pts";

function PoolRowView({ row }: { row: PoolRow }) {
  return (
    <li>
      <details aria-label={row.pool} className={row.stale ? "stale" : undefined} open={row.stale}
        style={row.stale ? { opacity: 0.5 } : undefined}>
        <summary>
          <strong>{row.pool}</strong>{" "}
          <span data-part="pct">{pctText(row.pct)}</span>{" "}
          <span data-part="pace">{row.pace == null ? "pace not recorded" : `pace ${row.pace.toFixed(1)}%`}</span>{" "}
          <span data-part="delta">{deltaText(row.delta)}</span>{" "}
          <span data-part="verdict">{row.verdict}</span>{" "}
          <span data-part="age">{row.reading_age_min == null ? "not recorded" : `${row.reading_age_min} min old`}</span>{" "}
          {row.stale && <span data-part="stale">stale</span>}
          <span data-part="track" style={{ display: "block", position: "relative", height: "0.5em" }}>
            <span data-part="bar"
              style={{ display: "block", height: "100%", width: `${row.pct == null ? 0 : clamp(row.pct)}%` }} />
            {row.pace != null && (
              <span data-part="paceline"
                style={{ position: "absolute", top: 0, bottom: 0, left: `${clamp(row.pace)}%`, borderLeft: "2px solid" }} />
            )}
          </span>
        </summary>
        <p data-part="why">{row.why}</p>
      </details>
    </li>
  );
}

export function BudgetPanel({ rows }: { rows: PoolRow[] }) {
  if (rows.length === 0) return <Empty view="Budget" />;
  return (
    <section aria-label="Budget">
      <ul>
        {rows.map((row) => <PoolRowView key={row.pool} row={row} />)}
      </ul>
    </section>
  );
}

// The Budget view: loads `budget_rows`, takes pushed `budget-rows` events, shows a core error in place.
export function Budget() {
  const [rows, setRows] = useState<PoolRow[]>([]);
  const [error, setError] = useState<CoreError | null>(null);

  useEffect(() => {
    let alive = true;
    // Without the Tauri runtime (plain browser, other views' tests) listen rejects; the view keeps its rows.
    const stop = listen<PoolRow[]>("budget-rows", (e) => {
      if (!alive) return;
      setRows(e.payload);
      setError(null);
    }).catch(() => () => {});
    call<PoolRow[]>("budget_rows").then((r) => {
      if (!alive) return;
      if ("error" in r) setError(r.error);
      else { setRows(r.ok); setError(null); }
    });
    return () => {
      alive = false;
      stop.then((f) => f());
    };
  }, []);

  if (error) {
    return (
      <section aria-label="Budget">
        <div role="alert">
          <p>{`${error.command} failed`}</p>
          {error.lines.map((line, i) => <p key={i}>{line}</p>)}
        </div>
      </section>
    );
  }
  return <BudgetPanel rows={rows} />;
}
