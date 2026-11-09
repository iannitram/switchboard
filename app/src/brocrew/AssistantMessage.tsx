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
};

/** "gemini · flash · picked by rules"; a part that is not known is left out. */
export function answerLine({ service, model, step }: Pick<AssistantMessageProps, "service" | "model" | "step">): string {
  return [service, model, step ? `picked by ${step}` : undefined].filter(Boolean).join(" · ");
}

// One answer: its text, the line naming the service, model and router step, and the handoff reason when the chat moved.
export function AssistantMessage({ text, service, model, step, handoff, resolveLink }: AssistantMessageProps) {
  const line = answerLine({ service, model, step });
  return (
    <article className="msg assistant" data-role="assistant" aria-label="Answer">
      <div className="msg-body" data-cy="assistant-message">
        <MessageText text={text} resolveLink={resolveLink} />
      </div>
      {line ? <p data-part="picked">{line}</p> : null}
      {handoff ? <p data-part="handoff">{`moved: ${handoff}`}</p> : null}
    </article>
  );
}
