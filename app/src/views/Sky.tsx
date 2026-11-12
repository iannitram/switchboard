import { lazy, Suspense, useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { call, type CoreResult } from "../core";
import { Empty } from "./Empty";
import { ConstellationView } from "../sky/ConstellationView";
import { SkyMap } from "../sky/SkyMap";
import { DriftList } from "../sky/DriftList";
import { StarPanel } from "../sky/StarPanel";
import type { SkyModel, Star } from "../sky/model";
import { devSkyStart, hasWebGL } from "../sky3d/support";

// The 3D sky (#297) loads only where WebGL works; elsewhere the flat SVG map below is the sky.
const Sky3D = lazy(() => import("../sky3d/Sky3D"));

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
  // The repo GitHub answered for, when the backed-up name was renamed or moved.
  resolved_repo?: string;
};
export type BackupRow = {
  repo: string;
  outcome: "written" | "refused" | "failed";
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

// One line per backup: what was written, why the last backup was kept (BAK-2), or the error
// that stopped this repo's backup while the others went on.
export function backupLine(row: BackupRow): string {
  const m = row.manifest;
  if (row.outcome === "written" && m) {
    const wiki = m.wiki ? ", the wiki" : "";
    const renamed = m.resolved_repo ? ` (GitHub answered for ${m.resolved_repo}: renamed or moved)` : "";
    return `${row.repo}${renamed} backed up at ${when(m.taken_at)}: ${m.refs} refs, ${m.issues} issues, ${m.pulls} PRs, ${m.comments} comments, ${m.labels} labels, ${m.milestones} milestones${wiki}, in ${row.folder}`;
  }
  if (row.outcome === "failed") {
    return `${row.repo} backup failed, the last backup (if there was one) was kept: ${row.reason ?? "no reason given"}`;
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

  const label = repo === undefined ? "Back up all" : "Back up this repo";
  return (
    <div data-testid="backup">
      <button className="btn" onClick={run} disabled={busy} aria-label={label}>
        {label}
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
      <button className="btn" onClick={run} disabled={busy}>
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
  const [open, setOpen] = useState<string | null>(() => devSkyStart().open);
  const [picked, setPicked] = useState<Star | null>(null);
  const [webgl, setWebgl] = useState(hasWebGL);
  const devStar = useRef(devSkyStart().star);
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

  // SKY-8 (#299): the picked star's panel is scrolled into view. jsdom has no scrollIntoView.
  const panel = useRef<HTMLDivElement>(null);
  useEffect(() => {
    panel.current?.scrollIntoView?.({ block: "nearest" });
  }, [picked]);

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

  // DEV only (see devSkyStart): pick the star named in VITE_SKY_STAR once the sky is in.
  if (devStar.current !== null && zoomed) {
    const n = devStar.current;
    devStar.current = null;
    const star = zoomed.features.flatMap((f) => f.stars).find((s) => s.number === n);
    if (star) queueMicrotask(() => setPicked(star));
  }

  if (webgl) {
    const back = () => {
      setOpen(null);
      setPicked(null);
    };
    return (
      <Suspense fallback={<p role="status">Drawing the sky…</p>}>
        <Sky3D
          sky={sky}
          open={zoomed ? zoomed.repo : null}
          onOpen={(repo) => {
            setPicked(null);
            setOpen(repo);
          }}
          onBack={back}
          picked={picked}
          onPick={setPicked}
          onFail={() => setWebgl(false)}
          toolbar={
            zoomed ? (
              <BackUp key={zoomed.repo} repo={zoomed.repo} />
            ) : (
              <>
                <Refresh onDone={load} />
                <BackUp />
              </>
            )
          }
        />
      </Suspense>
    );
  }

  if (zoomed) {
    return (
      <div style={{ display: "flex", flexWrap: "wrap", gap: "1em", alignItems: "flex-start" }}>
        <div style={{ flex: "1 1 0", minWidth: 0 }}>
          <ConstellationView
            c={zoomed}
            onBack={() => {
              setOpen(null);
              setPicked(null);
            }}
            onStar={setPicked}
          />
          <BackUp key={zoomed.repo} repo={zoomed.repo} />
        </div>
        {picked && (
          <div ref={panel} style={{ flex: "0 0 22em" }}>
            <StarPanel star={picked} onClose={() => setPicked(null)} />
          </div>
        )}
      </div>
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
