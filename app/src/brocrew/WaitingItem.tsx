// SPDX-License-Identifier: Apache-2.0
// Copyright the BroCrew authors (https://github.com/brocrew/brocrew).
// Ported from BroCrew apps/desktop/src/renderer/src/components/WaitingOnWhoWrote.vue at ebde9907
// (the Waiting on you item row). Changed: React, not Vue; the row shows what, why, a source link
// and since, with the one action button the core names (WAIT-1, SHELL-1).

/** One item from the core's `waiting::Item`, as JSON (WAIT-1). */
export type WaitingItemData = { what: string; why: string; source: unknown; link: string; action: string; since: number };

/** One item: what it is, why it waits on you, where it comes from, and its one action. */
export function WaitingItem({ item, onAction }: { item: WaitingItemData; onAction?: (item: WaitingItemData) => void }) {
  return (
    <li className="waiting-item">
      <p className="waiting-what" title={item.what}>
        {item.what}
      </p>
      <p className="waiting-why">{item.why}</p>
      <p className="waiting-since">
        <time dateTime={new Date(item.since * 1000).toISOString()}>
          {new Date(item.since * 1000).toLocaleString()}
        </time>
      </p>
      <a href={item.link} target="_blank" rel="noreferrer">
        source
      </a>
      <button type="button" onClick={() => onAction?.(item)}>
        {item.action}
      </button>
    </li>
  );
}
