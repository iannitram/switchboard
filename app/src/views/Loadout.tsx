import { useEffect, useState } from "react";
import { call, type CoreError } from "../core";
import { Empty } from "./Empty";

// IT-1 (#93): the Loadout panel. The JSON of core's Loadout (crates/core/src/it.rs). The panel
// shows what the core read; it runs no probes of its own.
export type Loadout = {
  read_at: number; clis: [string, boolean][]; models_loaded: string[]; toolchains: string[];
  busy: boolean; recent_errors: string[];
};

// One row per probe: its name, its state and its detail.
type Probe = { key: string; name: string; state: string; detail: string };

// The rows of a loadout, in the order the core reads them: CLIs, models, toolchains, busy or idle,
// then each recent error with its text. None when every list is empty: nothing was probed.
function probesOf(loadout: Loadout): Probe[] {
  if (loadout.clis.length + loadout.models_loaded.length + loadout.toolchains.length
    + loadout.recent_errors.length === 0) return [];
  return [
    ...loadout.clis.map(([name, signedIn]) => ({
      key: "cli:" + name, name, state: signedIn ? "signed in" : "not signed in", detail: "",
    })),
    ...loadout.models_loaded.map((model) => ({
      key: "model:" + model, name: model, state: "loaded", detail: "",
    })),
    ...loadout.toolchains.map((tool) => ({
      key: "toolchain:" + tool, name: tool, state: "installed", detail: "",
    })),
    { key: "busy", name: "fleet status", state: loadout.busy ? "busy" : "idle", detail: "" },
    ...loadout.recent_errors.map((text, i) => ({
      key: "error:" + i, name: "recent error", state: "error", detail: text,
    })),
  ];
}

// IT-1 (#93): the panel for one loadout. No loadout yet, or no probes, shows the empty state.
export function LoadoutPanel({ loadout }: { loadout: Loadout | null }) {
  const probes = loadout == null ? [] : probesOf(loadout);
  if (probes.length === 0) return <Empty view="Loadout" />;
  return (
    <section aria-label="Loadout">
      <ul>
        {probes.map((p) => (
          <li key={p.key} aria-label={p.name}>
            <strong>{p.name}</strong>{" "}
            <span data-part="state">{p.state}</span>{" "}
            {p.detail !== "" && <span data-part="detail">{p.detail}</span>}
          </li>
        ))}
      </ul>
    </section>
  );
}

// The Loadout view: loads `it_loadout` when it opens and again every minute, as the core reads
// it. A core error is shown in place.
export function Loadout() {
  const [loadout, setLoadout] = useState<Loadout | null>(null);
  const [error, setError] = useState<CoreError | null>(null);

  useEffect(() => {
    let alive = true;
    const read = () => call<Loadout>("it_loadout").then((r) => {
      if (!alive) return;
      if ("error" in r) setError(r.error);
      else { setLoadout(r.ok); setError(null); }
    });
    read();
    const timer = setInterval(read, 60_000);
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
