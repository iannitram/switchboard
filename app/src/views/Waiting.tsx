import { useCallback, useEffect, useRef, useState } from "react";
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

// CORE-3, BAK-3 (#347): what the core's `confirm_guarded` command answers (confirm.rs `Done`).
export type ConfirmDone = { what: string; lines: string[]; cleared: boolean };

// The text of a guarded request (core's `Source::GuardedAction`), null for any other item.
export function guardedText(item: WaitingItemData): string | null {
  const source = item.source as { GuardedAction?: { text?: unknown } } | null;
  const text = source && typeof source === "object" ? source.GuardedAction?.text : undefined;
  return typeof text === "string" ? text : null;
}

// The Waiting on you view (feature:waiting, SPEC.md). Asks the core once; a core error shows in
// place of the list (SHELL-3). The action of a guarded request (a repo delete or a restore over
// a live repo asked for from the CLI or MCP) opens the confirm step; only its Confirm button
// sends `confirmed: true` to the core, and Cancel sends nothing (CORE-3, #347).
export function Waiting() {
  const [items, setItems] = useState<WaitingItemData[]>([]);
  const [error, setError] = useState<CoreError | null>(null);
  const [asking, setAsking] = useState<WaitingItemData | null>(null);
  const [busy, setBusy] = useState(false);
  const [done, setDone] = useState<ConfirmDone | null>(null);
  const [refused, setRefused] = useState<CoreError | null>(null);
  const live = useRef(true);

  const read = useCallback(
    () =>
      call<WaitingItemData[]>("waiting").then((res) => {
        if (!live.current) return;
        if ("error" in res) {
          setError(res.error);
        } else {
          setItems(res.ok);
          setError(null);
        }
      }),
    [],
  );

  useEffect(() => {
    live.current = true;
    void read();
    return () => {
      live.current = false;
    };
  }, [read]);

  const ask = (item: WaitingItemData) => {
    if (busy || guardedText(item) === null) return;
    setDone(null);
    setRefused(null);
    setAsking(item);
  };

  const confirm = async () => {
    const text = asking && guardedText(asking);
    if (!text || busy) return;
    setBusy(true);
    const res = await call<ConfirmDone>("confirm_guarded", { text, confirmed: true });
    if (!live.current) return;
    setBusy(false);
    setAsking(null);
    if ("error" in res) {
      setRefused(res.error);
    } else {
      setDone(res.ok);
    }
    await read();
  };

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
  return (
    <>
      {asking && (
        <section role="group" aria-label="Confirm">
          <p data-part="request">{asking.what}</p>
          <p>Confirm does it now, on the live repo on GitHub. Cancel leaves the request waiting.</p>
          <button type="button" disabled={busy} onClick={() => setAsking(null)}>
            Cancel
          </button>
          <button type="button" disabled={busy} onClick={() => void confirm()}>
            Confirm
          </button>
        </section>
      )}
      {refused && (
        <div role="alert">
          <p>{refused.command} failed</p>
          {refused.lines.map((line, i) => (
            <p key={i}>{line}</p>
          ))}
        </div>
      )}
      {done && (
        <div role="status">
          <p>{done.what}: done</p>
          {done.lines.map((line, i) => (
            <p key={i}>{line}</p>
          ))}
          {!done.cleared && <p>The request stays in Waiting on you.</p>}
        </div>
      )}
      <WaitingList items={items} onAction={ask} />
    </>
  );
}
