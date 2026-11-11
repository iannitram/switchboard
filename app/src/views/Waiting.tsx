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

// The action the core gives a restore request whose first confirm held something back
// (confirm.rs `HELD_ACTION`): its confirm only does what was held back.
export const HELD_ACTION = "Queue what the restore held back";

export type GuardedRequest = { kind: "archive" | "restore" | "held"; repo: string };

// What a guarded request asks for and on which repo, as the core reads it back (confirm.rs
// `guarded_request`): the text ends in ` to archive <owner/name>` (or ` to delete <owner/name>`,
// the wording before #347's SPEC change) or ` to restore over <owner/name>` and the link is that
// repo's GitHub address. Null for anything else. Switchboard never deletes a repo (#347).
export function guardedRequest(item: WaitingItemData): GuardedRequest | null {
  const text = guardedText(item);
  const found = text === null ? null : /^.* to (archive|delete|restore over) ([A-Za-z0-9._-]+\/[A-Za-z0-9._-]+)$/.exec(text);
  if (!found || item.link !== `https://github.com/${found[2]}`) return null;
  const kind = found[1] !== "restore over" ? "archive" : item.action === HELD_ACTION ? "held" : "restore";
  return { kind, repo: found[2] };
}

// What Confirm does, in the confirm step.
export function confirmLine(request: GuardedRequest): string {
  switch (request.kind) {
    case "archive":
      return `Confirm archives ${request.repo} on GitHub: it becomes read-only. Nothing is deleted.`;
    case "restore":
      return `Confirm restores the backup over ${request.repo} on GitHub now: the backup's branches and tags are pushed over the live ones, and the labels, milestones, issues and comments it lacks are created.`;
    case "held":
      return `Confirm only queues what the restore over ${request.repo} held back (comments, closes). Nothing is pushed again.`;
  }
}

// The Waiting on you view (feature:waiting, SPEC.md). Asks the core once; a core error shows in
// place of the list (SHELL-3). The action of a guarded request (a repo archive or a restore over
// a live repo asked for from the CLI or MCP) opens the confirm step. Its Confirm button is
// enabled only while the box holds the repo's `owner/name`, letter for letter; only it sends
// `confirmed: true`, with the name typed, and the core checks the name again. Cancel sends
// nothing (CORE-3, #347).
export function Waiting() {
  const [items, setItems] = useState<WaitingItemData[]>([]);
  const [error, setError] = useState<CoreError | null>(null);
  const [asking, setAsking] = useState<WaitingItemData | null>(null);
  const [typed, setTyped] = useState("");
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
    if (busy || guardedRequest(item) === null) return;
    setDone(null);
    setRefused(null);
    setTyped("");
    setAsking(item);
  };

  const request = asking && guardedRequest(asking);
  const matches = !!request && typed === request.repo;

  const confirm = async () => {
    const text = asking && guardedText(asking);
    if (!text || busy || !matches) return;
    setBusy(true);
    const res = await call<ConfirmDone>("confirm_guarded", { text, confirmed: true, typed });
    if (!live.current) return;
    setBusy(false);
    setAsking(null);
    setTyped("");
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
      {asking && request && (
        <section role="group" aria-label="Confirm">
          <p data-part="request">{asking.what}</p>
          <p data-part="does">{confirmLine(request)}</p>
          <p>Cancel leaves the request waiting.</p>
          <label>
            Type {request.repo} to confirm
            <input
              type="text"
              value={typed}
              disabled={busy}
              autoComplete="off"
              spellCheck={false}
              onChange={(e) => setTyped(e.target.value)}
            />
          </label>
          <button type="button" disabled={busy} onClick={() => setAsking(null)}>
            Cancel
          </button>
          <button type="button" disabled={busy || !matches} onClick={() => void confirm()}>
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
