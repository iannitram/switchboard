import { useEffect, useState } from "react";
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

// How often the panel reads the loadout again, as the core's probe read does.
const REFRESH_MS = 60_000;

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

// IT-1: the panel for one loadout. Before the first read, or with no probes, the empty state.
export function LoadoutPanel({ loadout }: { loadout: Loadout | null }) {
  const rows = loadout == null ? [] : probeRows(loadout);
  if (rows.length === 0) return <Empty view="Loadout" />;
  return (
    <section aria-label="Loadout">
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

// The Loadout view: reads `loadout` when it opens and every minute after. A core error is shown
// in place; a loadout already on the screen stays.
export function Loadout() {
  const [loadout, setLoadout] = useState<Loadout | null>(null);
  const [error, setError] = useState<CoreError | null>(null);

  useEffect(() => {
    let alive = true;
    const read = () =>
      call<Loadout>("loadout").then((r) => {
        if (!alive) return;
        if ("error" in r) setError(r.error);
        else { setLoadout(r.ok); setError(null); }
      });
    read();
    const timer = setInterval(read, REFRESH_MS);
    return () => {
      alive = false;
      clearInterval(timer);
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
      <LoadoutPanel loadout={loadout} />
    </>
  );
}
