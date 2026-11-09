import { useEffect, useState } from "react";
import { call, type CoreResult } from "../core";
import { Empty } from "./Empty";
import { ConstellationView } from "../sky/ConstellationView";
import { SkyMap } from "../sky/SkyMap";
import { StarPanel } from "../sky/StarPanel";
import type { SkyModel, Star } from "../sky/model";

// The Sky view (feature:sky, SPEC.md). Asks the core for the sky and paints what it answers.
export function Sky() {
  const [answer, setAnswer] = useState<CoreResult<SkyModel> | null>(null);
  const [open, setOpen] = useState<string | null>(null);
  const [picked, setPicked] = useState<Star | null>(null);

  useEffect(() => {
    let live = true;
    call<SkyModel>("sky").then((r) => {
      if (live) setAnswer(r);
    });
    return () => {
      live = false;
    };
  }, []);

  if (answer && "error" in answer) {
    const { error } = answer;
    return (
      <>
        <Empty view="Sky" />
        <p role="alert">
          {error.command} failed: {error.lines.join(" / ")}
        </p>
      </>
    );
  }

  const sky = answer?.ok;
  if (!sky?.constellations?.length) {
    return <Empty view="Sky" />;
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
        {picked && <StarPanel star={picked} onClose={() => setPicked(null)} />}
      </>
    );
  }

  return (
    <section aria-label="Sky">
      <SkyMap sky={sky} onOpen={setOpen} />
    </section>
  );
}
