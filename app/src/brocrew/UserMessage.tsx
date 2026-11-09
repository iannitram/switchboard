// SPDX-License-Identifier: Apache-2.0
// Copyright the BroCrew authors (https://github.com/brocrew/brocrew).
// Lifted from BroCrew apps/desktop/src/renderer/src/components/UserMessage.vue at ebde9907.
// Changed: rewritten in React for Switchboard's ChatTurn (the person's text only); no transcript item, clock, provider-transcript mark, moved-from button, path finding, attachments or model-switch note; no store, queue, receipts or attachments; links in the text go through message-links.ts.

import { useState } from "react";
import {
  LINK_FAILED,
  LINK_UNAVAILABLE,
  MESSAGE_LINK_ATTRIBUTE,
  linkFailureReason,
  type ResolveMessageLink,
} from "./message-links";

export type UserMessageProps = {
  text: string;
  /** Resolves a brocrew:// link in the text. Unset: the link stays plain text. */
  resolveLink?: ResolveMessageLink;
};

const LINK_TOKEN = /(brocrew:\/\/[^\s<>"']+)/;

/** The text as typed, with each brocrew:// link as a link control (when a resolver is given). */
export function MessageText({ text, resolveLink }: { text: string; resolveLink?: ResolveMessageLink }) {
  if (!resolveLink) return <>{text}</>;
  // split with a capture group: the odd entries are the links.
  return (
    <>
      {text.split(LINK_TOKEN).map((part, i) =>
        i % 2 === 1 ? <MessageLink key={i} target={part} resolve={resolveLink} /> : <span key={i}>{part}</span>,
      )}
    </>
  );
}

function MessageLink({ target, resolve }: { target: string; resolve: ResolveMessageLink }) {
  const [failure, setFailure] = useState<string | null>(null);
  const agent = /^brocrew:\/\/agent\//i.test(target);
  const press = async () => {
    try {
      const answer = await resolve(target);
      setFailure(answer.state === "available" ? null : answer.state === "unavailable" ? LINK_UNAVAILABLE : linkFailureReason(answer));
    } catch {
      setFailure(LINK_FAILED);
    }
  };
  return (
    <>
      <button
        type="button"
        role="link"
        className="message-link"
        data-cy="message-link"
        aria-label={agent ? "Agent link" : "Conversation link"}
        {...{ [MESSAGE_LINK_ATTRIBUTE]: target }}
        onClick={() => void press()}
      >
        {target}
      </button>
      {failure ? <span role="status">{failure}</span> : null}
    </>
  );
}

// One message the person sent: their words as typed, labelled "You", in the order sent.
export function UserMessage({ text, resolveLink }: UserMessageProps) {
  return (
    <article className="msg user" data-role="user" aria-label="You">
      <strong>You</strong>
      <div className="msg-body" data-cy="user-message">
        <MessageText text={text} resolveLink={resolveLink} />
      </div>
    </article>
  );
}
