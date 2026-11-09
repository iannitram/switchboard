// SPDX-License-Identifier: Apache-2.0
// Copyright the BroCrew authors (https://github.com/brocrew/brocrew).
// Ported from BroCrew apps/desktop/src/renderer/src/components/PromptComposer.vue at ebde9907.
// Changed: rewritten in React; only the text box and Send are kept (Enter sends, Shift+Enter is a newline, Send waits while a send is in flight and says why it cannot go). No attachments, drag and drop, voice, queue or interrupt, no store, no conversation buttons.

import type { KeyboardEvent } from "react";
import type { SendBlock } from "./composer-notice";

export type PromptComposerProps = {
  /** The draft, owned by the caller so a failed send can keep it. */
  value: string;
  onChange: (value: string) => void;
  /** Called once per send with the draft text as typed (not trimmed). */
  onSend: (text: string) => void;
  /** True while a send is in flight: Send is disabled. */
  busy?: boolean;
};

/** Why Send cannot go right now, or null when it can. */
export function sendBlock(value: string, busy: boolean): SendBlock | null {
  if (busy) return { words: "Sending..." };
  if (value.trim() === "") return { words: "Type a message to send it." };
  return null;
}

export function PromptComposer({ value, onChange, onSend, busy = false }: PromptComposerProps) {
  const block = sendBlock(value, busy);
  const send = () => {
    if (block === null) onSend(value);
  };
  const onKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    // Enter sends; Shift+Enter falls through and adds a newline. An IME composition is left alone.
    if (event.key !== "Enter" || event.shiftKey || event.nativeEvent.isComposing) return;
    event.preventDefault();
    send();
  };
  return (
    <form
      className="prompt-composer"
      onSubmit={(event) => {
        event.preventDefault();
        send();
      }}
    >
      <textarea
        aria-label="Message"
        aria-describedby={block ? "composer-block" : undefined}
        value={value}
        onChange={(event) => onChange(event.target.value)}
        onKeyDown={onKeyDown}
        rows={3}
      />
      {block && <p id="composer-block" role="note">{block.words}</p>}
      <button type="submit" disabled={block !== null}>
        Send
      </button>
    </form>
  );
}
