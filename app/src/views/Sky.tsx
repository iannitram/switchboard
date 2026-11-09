import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { call, type CoreResult } from "../core";
import { Empty } from "./Empty";
import { ConstellationView } from "../sky/ConstellationView";
import { SkyMap } from "../sky/SkyMap";
import { DriftList } from "../sky/DriftList";
import { StarPanel } from "../sky/StarPanel";
import type { SkyModel, Star } from "../sky/model";

// BAK-1 as the `backup_repo` and `backup_all` commands answer it (#206).
export type Manifest = {
  repo: string;
  taken_at: string;
  refs: number;
  issues: number;
  open_issues: number;
  pulls: number;
  open_pulls: number;
  merged_pulls: number;
  comments: number;
  labels: number;
  label_assignments: number;
  milestones: number;
  releases: number;
  closed_without_pr: number;
  wiki: boolean;
};
export type BackupRow = {
  repo: string;
  outcome: "written" | "refused";
  folder: string;
  manifest: Manifest | null;
  reason: string | null;
};

// "2026-11-09 4:45pm", in local time.
function when(rfc3339: string): string {
  const d = new Date(rfc3339);
  if (Number.isNaN(d.getTime())) return rfc3339;
  const pad = (n: number) => String(n).padStart(2, "0");
  const h = d.getHours() % 12 || 12;
  const ampm = d.getHours() < 12 ? "am" : "pm";
  const min = d.getMinutes() === 0 ? "" : `:${pad(d.getMinutes())}`;
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${h}${min}${ampm}`;
}

// One line per backup: what was written, or why the last backup was kept (BAK-2).
export function backupLine(row: BackupRow): string {
  const m = row.manifest;
  if (row.outcome === "written" && m) {
    const wiki = m.wiki ? ", the wiki" : "";
    return `${row.repo} backed up at ${when(m.taken_at)}: ${m.refs} refs, ${m.issues} issues, ${m.pulls} PRs, ${m.comments} comments, ${m.labels} labels, ${m.milestones} milestones${wiki}, in ${row.folder}`;
  }
  return `${row.repo} not backed up, the last backup was kept: ${row.reason ?? "refused"}`;
}

// The back up buttons: one repo, or every repo in the sky. Shows each outcome, or the error.
function BackUp({ repo }: { repo?: string }) {
  const [busy, setBusy] = useState(false);
  const [lines, setLines] = useState<string[]>([]);
  const [failed, setFailed] = useState(false);

  async function run() {
    setBusy(true);
    setLines([]);
    const r = repo === undefined ? await call<BackupRow[]>("backup_all") : await call<BackupRow>("backup_repo", { repo });
    setBusy(false);
    if ("error" in r) {
      setFailed(true);
      setLines([`${r.error.command} failed: ${r.error.lines.join(" / ")}`]);
      return;
    }
    setFailed(false);
    const rows = Array.isArray(r.ok) ? r.ok : [r.ok];
    setLines(rows.length ? rows.map(backupLine) : ["No repos to back up."]);
  }

  return (
    <div data-testid="backup">
      <button onClick={run} disabled={busy}>
        {repo === undefined ? "Back up all" : "Back up"}
      </button>
      {busy && <p role="status">Backing up…</p>}
      {lines.length > 0 && (
        <ul role={failed ? "alert" : "status"} aria-label="Backup outcome">
          {lines.map((line) => (
            <li key={line}>{line}</li>
          ))}
        </ul>
      )}
    </div>
  );
}

// GH-1 (#240): the Refresh button. Reads every repo now, then asks the sky again (`onDone`).
function Refresh({ onDone }: { onDone: () => void }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function run() {
    setBusy(true);
    const r = await call<string[]>("refresh_all");
    setBusy(false);
    setError("error" in r ? `${r.error.command} failed: ${r.error.lines.join(" / ")}` : null);
    onDone();
  }

  return (
    <div data-testid="refresh">
      <button onClick={run} disabled={busy}>
        Refresh
      </button>
      {busy && <p role="status">Refreshing…</p>}
      {error && <p role="alert" aria-label="Refresh outcome">{error}</p>}
    </div>
  );
}

// The Sky view (feature:sky, SPEC.md). Asks the core for the sky and paints what it answers.
// It asks again on each `snapshot` event from the poll thread and after a Refresh (GH-1).
export function Sky() {
  const [answer, setAnswer] = useState<CoreResult<SkyModel> | null>(null);
  const [open, setOpen] = useState<string | null>(null);
  const [picked, setPicked] = useState<Star | null>(null);
  const alive = useRef(true);
  const load = useCallback(() => {
    call<SkyModel>("sky").then((r) => {
      if (alive.current) setAnswer(r);
    });
  }, []);

  useEffect(() => {
    alive.current = true;
    load();
    // Without the Tauri runtime listen rejects; the view keeps its sky.
    const stop = listen<string[]>("snapshot", () => load()).catch(() => () => {});
    return () => {
      alive.current = false;
      stop.then((f) => f());
    };
  }, [load]);

  if (answer && "error" in answer) {
    const { error } = answer;
    return (
      <>
        <Empty view="Sky" />
        <Refresh onDone={load} />
        <p role="alert">
          {error.command} failed: {error.lines.join(" / ")}
        </p>
      </>
    );
  }

  const sky = answer?.ok;
  if (!sky?.constellations?.length) {
    return (
      <>
        <Empty view="Sky" />
        {answer && <Refresh onDone={load} />}
      </>
    );
  }

  const zoomed = open === null ? undefined : sky.constellations.find((c) => c.repo === open);
  if (zoomed) {
    return (
      <>
        <ConstellationView
          c={zoomed}
          onBack={() => {
            setOpen(null);
            setPicked(null);
          }}
          onStar={setPicked}
        />
        <BackUp key={zoomed.repo} repo={zoomed.repo} />
        {picked && <StarPanel star={picked} onClose={() => setPicked(null)} />}
      </>
    );
  }

  return (
    <section aria-label="Sky">
      <Refresh onDone={load} />
      <BackUp />
      <SkyMap sky={sky} onOpen={setOpen} />
      <DriftList drift={sky.constellations.flatMap((c) => c.drift.map((d) => ({ ...d, repo: c.repo })))} />
    </section>
  );
}
