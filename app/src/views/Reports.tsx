import { useEffect, useState } from "react";
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

// The Reports view (feature:reports, SPEC.md). Asks the core once; a core error shows in place (SHELL-3).
export function Reports() {
  const [reports, setReports] = useState<Report[]>([]);
  const [error, setError] = useState<CoreError | null>(null);

  useEffect(() => {
    let live = true;
    call<Report[]>("reports").then((res) => {
      if (!live) return;
      if ("error" in res) {
        setError(res.error);
      } else {
        setReports(res.ok);
        setError(null);
      }
    });
    return () => {
      live = false;
    };
  }, []);

  return (
    <>
      {error ? (
        <div role="alert">
          <p>{error.command} failed</p>
          {error.lines.map((line, i) => (
            <p key={i}>{line}</p>
          ))}
        </div>
      ) : null}
      <ReportList reports={reports} />
    </>
  );
}
