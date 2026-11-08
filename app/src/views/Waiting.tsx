import { useEffect, useState } from "react";
import { call, type CoreError } from "../core";
import { WaitingItem, type WaitingItemData } from "../brocrew/WaitingItem";

// The Waiting view (feature:waiting, SPEC.md, WAIT-1): the core's merged items, newest first,
// each as a BroCrew "Waiting on you" item. The core sorts; the list shows them in the order given.

/** The list in the order given, or the empty text when nothing waits on you. */
export function WaitingList({ items, onAction }: { items: WaitingItemData[]; onAction?: (item: WaitingItemData) => void }) {
  if (items.length === 0) {
    return (
      <section aria-label="Waiting on you">
        <p>Nothing is waiting on you.</p>
      </section>
    );
  }
  return (
    <section aria-label="Waiting on you">
      <ol>
        {items.map((item, i) => (
          <WaitingItem key={`${i}-${item.link}`} item={item} onAction={onAction} />
        ))}
      </ol>
    </section>
  );
}

type State = { status: "loading" } | { status: "error"; text: string } | { status: "ok"; items: WaitingItemData[] };

function errorText(error: CoreError): string {
  return error.lines.length > 0 ? error.lines.join("\n") : `${error.command} failed`;
}

export function Waiting() {
  const [state, setState] = useState<State>({ status: "loading" });

  useEffect(() => {
    let live = true;
    call<WaitingItemData[]>("waiting").then(
      (result) => {
        if (!live) return;
        if ("error" in result) setState({ status: "error", text: errorText(result.error) });
        else setState({ status: "ok", items: result.ok });
      },
      (e: unknown) => {
        if (live) setState({ status: "error", text: e instanceof Error ? e.message : String(e) });
      },
    );
    return () => {
      live = false;
    };
  }, []);

  if (state.status === "loading") return <section aria-label="Waiting on you" />;
  if (state.status === "error") {
    return (
      <section aria-label="Waiting on you">
        <p role="alert">{state.text}</p>
      </section>
    );
  }
  return <WaitingList items={state.items} />;
}
