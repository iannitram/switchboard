// SPDX-License-Identifier: Apache-2.0
// Copyright the BroCrew authors (https://github.com/brocrew/brocrew).
// Lifted from BroCrew apps/desktop/src/renderer/src/components/AssistantMessage.vue at ebde9907.
// Changed: rewritten in React for Switchboard's ChatTurn (the answer's text, and the line that says which service and model wrote it and which router step picked it, plus the handoff reason); plain text only, no Markdown, streaming, questions, documents, path finding, work report or model-switch notes; no store, queue, receipts or attachments; links in the text go through message-links.ts.

import type { ResolveMessageLink } from "./message-links";
import { MessageText } from "./UserMessage";

export type AssistantMessageProps = {
  text: string;
  service?: string;
  model?: string;
  /** The router step that picked the model (FD-1). */
  step?: string;
  /** Set when the chat moved to another service; the reason it moved. */
  handoff?: string | null;
  /** Resolves a brocrew:// link in the text. Unset: the link stays plain text. */
  resolveLink?: ResolveMessageLink;
  /** The rating this answer has (FD-9): the button that is pressed. */
  rating?: "up" | "down";
  /** Set: the answer shows a thumbs up and a thumbs down that call this. Unset: no buttons. */
  onRate?: (rating: "up" | "down") => void;
};

/** "gemini · flash · picked by rules"; a part that is not known is left out. */
export function answerLine({ service, model, step }: Pick<AssistantMessageProps, "service" | "model" | "step">): string {
  return [service, model, step ? `picked by ${step}` : undefined].filter(Boolean).join(" · ");
}

// One answer: its text, the line naming the service, model and router step, and the handoff reason when the chat moved.
export function AssistantMessage({ text, service, model, step, handoff, resolveLink, rating, onRate }: AssistantMessageProps) {
  const line = answerLine({ service, model, step });
  return (
    <article className="msg assistant" data-role="assistant" aria-label="Answer">
      <div className="msg-body" data-cy="assistant-message">
        <MessageText text={text} resolveLink={resolveLink} />
      </div>
      {line ? <p data-part="picked">{line}</p> : null}
      {handoff ? <p data-part="handoff">{`moved: ${handoff}`}</p> : null}
      {onRate ? (
        <div data-part="rating">
          {/* Icons are SVG with no text, so the answer's text content stays the answer alone. Paths: Feather icons (MIT). */}
          <button type="button" aria-label="Thumbs up" aria-pressed={rating === "up"} onClick={() => onRate("up")}>
            <svg aria-hidden="true" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <path d="M14 9V5a3 3 0 0 0-3-3l-4 9v11h11.28a2 2 0 0 0 2-1.7l1.38-9a2 2 0 0 0-2-2.3zM7 22H4a2 2 0 0 1-2-2v-7a2 2 0 0 1 2-2h3" />
            </svg>
          </button>
          <button type="button" aria-label="Thumbs down" aria-pressed={rating === "down"} onClick={() => onRate("down")}>
            <svg aria-hidden="true" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <path d="M10 15v4a3 3 0 0 0 3 3l4-9V2H5.72a2 2 0 0 0-2 1.7l-1.38 9a2 2 0 0 0 2 2.3zm7-13h2.67A2.31 2.31 0 0 1 22 4v7a2.31 2.31 0 0 1-2.33 2H17" />
            </svg>
          </button>
        </div>
      ) : null}
    </article>
  );
}
