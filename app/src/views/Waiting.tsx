import { useEffect, useState } from "react";
import { call } from "../core";
import type { CoreError } from "../core";
import { WaitingItem, type WaitingItemData } from "../brocrew/WaitingItem";

export type { WaitingItemData } from "../brocrew/WaitingItem";

export const NOTHING_WAITING = "Nothing is waiting on you.";

// WAIT-1 (#60): the core's merged items in the order given (the core sorts, newest first), each a
// BroCrew-ported item. An empty list says so, never a blank pane (SHELL-1).
export function WaitingList({ items, onAction }: { items: WaitingItemData[]; onAction?: (item: WaitingItemData) => void }) {
  return (
    <section aria-label="Waiting on you">
      {items.length === 0 ? (
        <p>{NOTHING_WAITING}</p>
      ) : (
        <ol>
          {items.map((item, i) => (
            <li key={`${i}-${item.link}`}>
              <WaitingItem item={item} onAction={onAction} />
            </li>
          ))}
        </ol>
      )}
    </section>
  );
}

// The Waiting on you view (feature:waiting, SPEC.md). Asks the core once; a core error shows in
// place of the list (SHELL-3).
export function Waiting() {
  const [items, setItems] = useState<WaitingItemData[]>([]);
  const [error, setError] = useState<CoreError | null>(null);

  useEffect(() => {
    let live = true;
    call<WaitingItemData[]>("waiting").then((res) => {
      if (!live) return;
      if ("error" in res) {
        setError(res.error);
      } else {
        setItems(res.ok);
        setError(null);
      }
    });
    return () => {
      live = false;
    };
  }, []);

  if (error) {
    return (
      <div role="alert">
        <p>{error.command} failed</p>
        {error.lines.map((line, i) => (
          <p key={i}>{line}</p>
        ))}
      </div>
    );
  }
  return <WaitingList items={items} />;
}
