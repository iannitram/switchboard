// SPDX-License-Identifier: Apache-2.0
// Copyright the BroCrew authors (https://github.com/brocrew/brocrew).
// Lifted from BroCrew apps/desktop/src/renderer/src/components/WorkReportEntry.vue and apps/desktop/src/renderer/src/lib/work-report.ts at ebde9907.
// Changed: rewritten in React for Switchboard's report shape (the five REP-1 parts); the mark reads "Switchboard" for "This app"; no store, conversation buttons or files.

import type { ReactNode } from "react";

export type Move = { from_service: string; from_model: string; to_service: string; to_model: string; why: string };
export type Report = {
  id: string; title: string | null; project: string | null; at: number | null;
  outcome: string; failure_mode: string | null;
  changed: { pr: string | null; files: number | null; lines: number | null };
  checked: string[]; not_checked: string[];
  cost: { pool: string | null; tokens: number | null; usd: number | null; seconds: number | null };
  moves: Move[];
};                                     // the JSON of core's reports::Report (#44)

export const REPORT_MARK = "Switchboard · work you gave out";

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/** Unix seconds -> local "Nov 6, 4:45pm" (minutes left out on the hour). */
export function reportTime(at: number): string {
  const d = new Date(at * 1000);
  const h24 = d.getHours();
  const h = h24 % 12 === 0 ? 12 : h24 % 12;
  const mi = d.getMinutes();
  const clock = `${h}${mi === 0 ? "" : `:${String(mi).padStart(2, "0")}`}${h24 < 12 ? "am" : "pm"}`;
  return `${MONTHS[d.getMonth()]} ${d.getDate()}, ${clock}`;
}

/** Seconds, rounded to the second: "45s", "15m 31s", "1h 2m". */
export function duration(seconds: number): string {
  const total = Math.round(seconds);
  if (total < 60) return `${total}s`;
  if (total < 3600) return `${Math.floor(total / 60)}m ${total % 60}s`;
  return `${Math.floor(total / 3600)}h ${Math.floor((total % 3600) / 60)}m`;
}

function Lines({ part, items }: { part: string; items: string[] }) {
  return (
    <ul data-part={part}>
      {items.length > 0 ? items.map((t, i) => <li key={i}>{t}</li>) : <li>nothing</li>}
    </ul>
  );
}

// One report: the head (outcome first), then what changed, what was checked, what wasn't, and cost.
// Open unless the outcome is passed, so the ones to act on are expanded.
export function ReportEntry({ report, children }: { report: Report; children?: ReactNode }) {
  const heading = report.title ?? report.id;
  const outcome = report.failure_mode ? `${report.outcome} (${report.failure_mode})` : report.outcome;
  const { changed, cost } = report;
  return (
    <details className="report" data-outcome={report.outcome} aria-label={heading} open={report.outcome !== "passed"}>
      <summary>
        <span data-part="outcome">{outcome}</span>
        <strong data-part="heading">{heading}</strong>
        {report.project ? <span data-part="project">{report.project}</span> : null}
        <span data-part="mark">{REPORT_MARK}</span>
        {report.at !== null ? <time data-part="time">{reportTime(report.at)}</time> : null}
      </summary>
      <div data-part="changed">
        {changed.pr ? <a data-part="pr" href={changed.pr}>{changed.pr}</a> : <span data-part="pr">no PR</span>}
        <span data-part="files">{changed.files !== null ? `files ${changed.files}` : "files not recorded"}</span>
        <span data-part="lines">{changed.lines !== null ? `lines ${changed.lines}` : "lines not recorded"}</span>
      </div>
      <Lines part="checked" items={report.checked} />
      <Lines part="not-checked" items={report.not_checked} />
      <div data-part="cost">
        <span data-part="pool">{cost.pool ? cost.pool : "pool not recorded"}</span>
        <span data-part="tokens">{cost.tokens !== null ? `${cost.tokens} tokens` : "tokens not recorded"}</span>
        <span data-part="usd">{cost.usd !== null ? `$${cost.usd.toFixed(2)}` : "cost not recorded"}</span>
        <span data-part="seconds">{cost.seconds !== null ? duration(cost.seconds) : "time not recorded"}</span>
      </div>
      {children}
    </details>
  );
}
