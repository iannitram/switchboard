import { useEffect, useState } from "react";
import { call, type CoreResult } from "../core";
import { Empty } from "./Empty";
import { SkyMap } from "../sky/SkyMap";
import type { SkyModel } from "../sky/model";

// The Sky view (feature:sky, SPEC.md). Asks the core for the sky and paints what it answers.
export function Sky() {
  const [answer, setAnswer] = useState<CoreResult<SkyModel> | null>(null);

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

  return (
    <section aria-label="Sky">
      <SkyMap sky={sky} />
    </section>
  );
}
