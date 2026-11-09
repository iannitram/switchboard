import { useEffect, useState, type ReactNode } from "react";
import { listen } from "@tauri-apps/api/event";
import { call, type CoreError } from "../core";
import { Empty } from "./Empty";

// BUD-1 (#24): the Budget panel. One row per pool, the JSON of core's PoolRow (#23).
export type PoolRow = {
  pool: string; pct: number | null; pace: number | null; delta: number | null;
  verdict: string; reading_age_min: number | null; stale: boolean; why: string;
};

// BUD-2 (#26): the JSON of core's PoolEdit (#25). A key left out is None there.
export type PoolEdit = { pool: string; plan?: string; cap?: number; reset?: string; cost_usd?: number };

const clamp = (n: number) => Math.min(100, Math.max(0, n));
const pctText = (n: number | null) => (n == null ? "not recorded" : n.toFixed(1) + "%");
const deltaText = (n: number | null) =>
  n == null ? "not recorded" : (n >= 0 ? "+" : "") + n.toFixed(1) + " pts";
const isAmount = (s: string) => Number.isFinite(Number(s)) && Number(s) >= 0;

// BUD-2 (#26): the editor under one row. Enter or Save sends the filled-in fields; Escape or Cancel closes.
export function PoolEditor({ pool, onSave, onCancel }: {
  pool: string; onSave: (edit: PoolEdit) => void; onCancel: () => void;
}) {
  const [plan, setPlan] = useState("");
  const [cap, setCap] = useState("");
  const [reset, setReset] = useState("");
  const [cost, setCost] = useState("");
  const [refused, setRefused] = useState<string | null>(null);

  function submit() {
    const t = { plan: plan.trim(), cap: cap.trim(), reset: reset.trim(), cost: cost.trim() };
    if (t.cap !== "" && !isAmount(t.cap)) return setRefused("Cap must be a number 0 or more");
    if (t.cost !== "" && !isAmount(t.cost)) return setRefused("Cost must be a number 0 or more");
    const edit: PoolEdit = { pool };
    if (t.plan !== "") edit.plan = t.plan;
    if (t.cap !== "") edit.cap = Number(t.cap);
    if (t.reset !== "") edit.reset = t.reset;
    if (t.cost !== "") edit.cost_usd = Number(t.cost);
    if (Object.keys(edit).length === 1) return setRefused("Nothing to save");
    setRefused(null);
    onSave(edit);
  }

  return (
    <form aria-label={"Edit " + pool}
      onSubmit={(e) => { e.preventDefault(); submit(); }}
      onKeyDown={(e) => {
        if (e.key === "Escape") onCancel();
        else if (e.key === "Enter") { e.preventDefault(); submit(); }
      }}>
      <label>Plan <input value={plan} onChange={(e) => setPlan(e.target.value)} /></label>{" "}
      <label>Cap ($) <input value={cap} onChange={(e) => setCap(e.target.value)} /></label>{" "}
      <label>Reset <input placeholder="Sun 6:00am" value={reset} onChange={(e) => setReset(e.target.value)} /></label>{" "}
      <label>Cost ($) <input value={cost} onChange={(e) => setCost(e.target.value)} /></label>{" "}
      <button type="button" onClick={submit}>Save</button>{" "}
      <button type="button" onClick={onCancel}>Cancel</button>
      {refused && <p role="alert">{refused}</p>}
    </form>
  );
}

function PoolRowView({ row, onEdit, editor }: { row: PoolRow; onEdit?: () => void; editor?: ReactNode }) {
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
      {onEdit && <button type="button" onClick={onEdit}>{"Edit " + row.pool}</button>}
      {editor}
    </li>
  );
}

export function BudgetPanel({ rows, onSave }: { rows: PoolRow[]; onSave?: (edit: PoolEdit) => void }) {
  // One editor open at a time: the pool whose row holds it.
  const [editing, setEditing] = useState<string | null>(null);
  if (rows.length === 0) return <Empty view="Budget" />;
  return (
    <section aria-label="Budget">
      <ul>
        {rows.map((row) => (
          <PoolRowView key={row.pool} row={row}
            onEdit={onSave ? () => setEditing(row.pool) : undefined}
            editor={onSave && editing === row.pool ? (
              <PoolEditor pool={row.pool}
                onSave={(edit) => { setEditing(null); onSave(edit); }}
                onCancel={() => setEditing(null)} />
            ) : null} />
        ))}
      </ul>
    </section>
  );
}

// The Budget view: loads `budget_rows`, takes pushed `budget-rows` events, saves edits through
// `budget_save_edit`. A core error is shown in place; with rows on the screen they stay.
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

  function save(edit: PoolEdit) {
    call<PoolRow[]>("budget_save_edit", { edit }).then((r) => {
      if ("error" in r) setError(r.error);
      else { setRows(r.ok); setError(null); }
    });
  }

  return (
    <>
      {error && (
        <div role="alert">
          <p>{`${error.command} failed`}</p>
          {error.lines.map((line, i) => <p key={i}>{line}</p>)}
        </div>
      )}
      {(error == null || rows.length > 0) && <BudgetPanel rows={rows} onSave={save} />}
    </>
  );
}
