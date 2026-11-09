// SPDX-License-Identifier: Apache-2.0
// Copyright the BroCrew authors (https://github.com/brocrew/brocrew).
// Lifted from BroCrew apps/desktop/src/renderer/src/components/AttentionQueue.vue (the Waiting on you row) at ebde9907.
// Changed: rewritten in React for Switchboard's waiting::Item shape (what, why, source, link, action, since); one action button and a link to the source in place of the decisions and the open button; no store, scope lines, warnings, or age clock.

import { reportTime } from "./ReportEntry";

export type WaitingItemData = { what: string; why: string; source: unknown; link: string; action: string; since: number }; // the JSON of core's waiting::Item (#49, #50)

// One Waiting on you item: what needs me, since when, why, a link to its source and its one action.
export function WaitingItem({ item, onAction }: { item: WaitingItemData; onAction?: (item: WaitingItemData) => void }) {
  return (
    <article className="waiting-item" aria-label={item.what}>
      <header data-part="head">
        <strong data-part="what">{item.what}</strong>
        <time data-part="since">{reportTime(item.since)}</time>
      </header>
      <p data-part="why">{item.why}</p>
      <div data-part="actions">
        <a data-part="link" href={item.link} title={item.link}>source</a>
        <button type="button" data-part="action" onClick={() => onAction?.(item)}>{item.action}</button>
      </div>
    </article>
  );
}
