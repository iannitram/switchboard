import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { call, type CoreError } from "../core";
import { Empty } from "./Empty";

// IT-1 (#93, #205): the JSON of core's `Loadout` as the `loadout` command sends it. `clis` is a
// list of [name, signed_in] pairs. The panel shows what the core read; it runs no probes.
export type Loadout = {
  read_at: number;
  clis: [string, boolean][];
  models_loaded: string[];
  toolchains: string[];
  busy: boolean;
  recent_errors: string[];
};

// The toolchains the core probes (it.rs `LOADOUT_TOOLCHAINS`). The core leaves a missing one out
// of `toolchains`, so the panel names it "not found" instead of dropping its row.
const TOOLCHAINS = ["cargo", "node", "python3"];

// How often the age line is worked out again, so a stalled core timer shows as an older read.
const AGE_TICK_MS = 30_000;

// IT-1 (#244): how long ago the loadout was read, in whole minutes or hours, rounded down.
export function ageText(readAt: number, now: number): string {
  const secs = Math.max(0, Math.floor(now - readAt));
  if (secs < 60) return "read just now";
  if (secs < 3600) return `read ${Math.floor(secs / 60)} min ago`;
  return `read ${Math.floor(secs / 3600)} h ago`;
}

// One row per probe: its name, its state, and its detail ("" when it has none). A failed probe
// has `failed` set; the core gives no error text, so none is shown.
export type ProbeRow = { name: string; state: string; detail: string; failed: boolean };

function hasProbes(l: Loadout): boolean {
  return l.clis.length + l.toolchains.length + l.models_loaded.length + l.recent_errors.length > 0 || l.busy;
}

// The rows, in the core's probe order: CLIs, toolchains, llama-swap, fleet status, fleet errors.
export function probeRows(l: Loadout): ProbeRow[] {
  if (!hasProbes(l)) return [];
  const toolchains = [...TOOLCHAINS, ...l.toolchains.filter((t) => !TOOLCHAINS.includes(t))];
  return [
    ...l.clis.map(([name, signedIn]) => ({
      name, state: signedIn ? "signed in" : "not signed in", detail: "", failed: !signedIn,
    })),
    ...toolchains.map((name) => {
      const found = l.toolchains.includes(name);
      return { name, state: found ? "found" : "not found", detail: "", failed: !found };
    }),
    {
      name: "llama-swap",
      state: l.models_loaded.length > 0 ? `${l.models_loaded.length} loaded` : "no models loaded",
      detail: l.models_loaded.join(", "),
      failed: false,
    },
    { name: "fleet status", state: l.busy ? "busy" : "idle", detail: "", failed: false },
    {
      name: "fleet errors",
      state: l.recent_errors.length > 0 ? `${l.recent_errors.length} recent` : "none",
      detail: l.recent_errors.join("\n"),
      failed: false,
    },
  ];
}

// IT-1: the panel for one loadout, with its age. Before the first read, or with no probes, the
// empty state. `now` is Unix seconds; absent, it is the clock now.
export function LoadoutPanel({ loadout, now }: { loadout: Loadout | null; now?: number }) {
  const rows = loadout == null ? [] : probeRows(loadout);
  if (loadout == null || rows.length === 0) return <Empty view="Loadout" />;
  return (
    <section aria-label="Loadout">
      <p data-part="age">{ageText(loadout.read_at, now ?? Date.now() / 1000)}</p>
      <ul>
        {rows.map((row) => (
          <li key={row.name} aria-label={row.name} className={row.failed ? "failed" : undefined}>
            <strong>{row.name}</strong>{" "}
            <span data-part="state">{row.state}</span>
            {row.detail !== "" && (
              <span data-part="detail" style={{ display: "block", whiteSpace: "pre-line" }}>{row.detail}</span>
            )}
          </li>
        ))}
      </ul>
    </section>
  );
}

// The Loadout view: reads `loadout` when it opens, then redraws on each `loadout` event the core's
// minute read pushes. It runs no read timer of its own. A core error is shown in place; a loadout
// already on the screen stays.
export function Loadout() {
  const [loadout, setLoadout] = useState<Loadout | null>(null);
  const [error, setError] = useState<CoreError | null>(null);
  const [now, setNow] = useState(() => Date.now() / 1000);

  useEffect(() => {
    let alive = true;
    call<Loadout>("loadout").then((r) => {
      if (!alive) return;
      setNow(Date.now() / 1000);
      if ("error" in r) setError(r.error);
      else { setLoadout(r.ok); setError(null); }
    });
    const stop = listen<Loadout>("loadout", (e) => {
      if (!alive) return;
      setLoadout(e.payload);
      setError(null);
      setNow(Date.now() / 1000);
    }).catch(() => () => {});
    const tick = setInterval(() => setNow(Date.now() / 1000), AGE_TICK_MS);
    return () => {
      alive = false;
      clearInterval(tick);
      stop.then((f) => f());
    };
  }, []);

  return (
    <>
      {error && (
        <div role="alert">
          <p>{`${error.command} failed`}</p>
          {error.lines.map((line, i) => <p key={i}>{line}</p>)}
        </div>
      )}
      <LoadoutPanel loadout={loadout} now={now} />
    </>
  );
}
