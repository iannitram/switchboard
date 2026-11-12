import { useEffect, useRef, useState } from "react";
import { call } from "../core";
import type { CoreError } from "../core";
import { ReportEntry, type Report } from "../brocrew/ReportEntry";
import { Empty } from "./Empty";
import { WorkTree } from "./WorkTree";

export type { Move, Report } from "../brocrew/ReportEntry";

// REP-1 (#45): the fixed reports the core sends, newest first. Each entry is a BroCrew-ported report entry.
export function ReportList({ reports }: { reports: Report[] }) {
  if (reports.length === 0) return <Empty view="Reports" />;
  return (
    <section aria-label="Reports">
      <ul>
        {reports.map((r, i) => (
          <li key={`${i}-${r.id}`}>
            <ReportEntry report={r}><WorkTree moves={r.moves} /></ReportEntry>
          </li>
        ))}
      </ul>
    </section>
  );
}

/** REP-3: the JSON of core's `reports::Filter`; null means All. */
export type Filter = { project: string | null; outcome: string | null; pool: string | null };
export const NO_FILTER: Filter = { project: null, outcome: null, pool: null };

/** The reports command's answer, including non-fatal thread read warnings. */
export type ReportsAnswer = { reports: Report[]; warnings: string[] };

// Accept the earlier list-only answer as well (older shells and report fixtures).
function answer(value: ReportsAnswer | Report[]): ReportsAnswer {
  return Array.isArray(value) ? { reports: value, warnings: [] } : value;
}

const FIELDS: { field: keyof Filter; label: string; valueOf: (r: Report) => string | null }[] = [
  { field: "project", label: "Project", valueOf: (r) => r.project },
  { field: "outcome", label: "Outcome", valueOf: (r) => r.outcome },
  { field: "pool", label: "Pool", valueOf: (r) => r.cost.pool },
];

// REP-3 (#48): one select per field, All then the distinct values the loaded reports carry, sorted.
export function FilterBar({ reports, filter, onChange }: { reports: Report[]; filter: Filter; onChange: (f: Filter) => void }) {
  return (
    <form aria-label="Filter reports" onSubmit={(e) => e.preventDefault()}>
      {FIELDS.map(({ field, label, valueOf }) => {
        const values = [...new Set(reports.map(valueOf).filter((v): v is string => v !== null))].sort();
        return (
          <label key={field}>
            {label}
            <select
              aria-label={label}
              value={filter[field] ?? ""}
              onChange={(e) => onChange({ ...filter, [field]: e.target.value === "" ? null : e.target.value })}
            >
              <option value="">All</option>
              {values.map((v) => (
                <option key={v} value={v}>{v}</option>
              ))}
            </select>
          </label>
        );
      })}
    </form>
  );
}

// The Reports view (feature:reports, SPEC.md). Asks the core once for all reports; a filter asks
// the core for the matching ones (REP-3); a core error shows in place (SHELL-3).
export function Reports() {
  const [all, setAll] = useState<Report[]>([]);
  const [shown, setShown] = useState<Report[] | null>(null); // null: show all
  const [filter, setFilter] = useState<Filter>(NO_FILTER);
  const [error, setError] = useState<CoreError | null>(null);
  const [allWarnings, setAllWarnings] = useState<string[]>([]);
  const [warnings, setWarnings] = useState<string[]>([]);
  const latest = useRef(0);

  useEffect(() => {
    let live = true;
    call<ReportsAnswer | Report[]>("reports").then((res) => {
      if (!live) return;
      if ("error" in res) {
        setError(res.error);
      } else {
        const loaded = answer(res.ok);
        setAll(loaded.reports);
        setAllWarnings(loaded.warnings);
        setWarnings(loaded.warnings);
        setError(null);
      }
    });
    return () => {
      live = false;
    };
  }, []);

  const choose = (next: Filter) => {
    setFilter(next);
    const request = ++latest.current;
    if (next.project === null && next.outcome === null && next.pool === null) {
      setShown(null);
      setError(null);
      setWarnings(allWarnings);
      return;
    }
    call<ReportsAnswer | Report[]>("reports", { filter: next }).then((res) => {
      if (request !== latest.current) return; // an answer for an older filter
      if ("error" in res) {
        setError(res.error);
      } else {
        const loaded = answer(res.ok);
        setShown(loaded.reports);
        setWarnings(loaded.warnings);
        setError(null);
      }
    });
  };

  return (
    <>
      {error || warnings.length > 0 ? (
        <div role="alert">
          <p>{error ? `${error.command} failed` : "reports warnings"}</p>
          {error?.lines.map((line, i) => (
            <p key={i}>{line}</p>
          ))}
          {warnings.map((line, i) => <p key={`warning-${i}`}>{line}</p>)}
        </div>
      ) : null}
      {all.length > 0 ? <FilterBar reports={all} filter={filter} onChange={choose} /> : null}
      {shown !== null && shown.length === 0 ? <p>no reports match this filter</p> : <ReportList reports={shown ?? all} />}
    </>
  );
}
