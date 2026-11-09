import { useRef, useState } from "react";
import { Empty } from "./Empty";
import { ErrorBanner } from "../ErrorBanner";
import { PromptComposer } from "../brocrew/PromptComposer";
import type { ResolveMessageLink } from "../brocrew/message-links";
import { UserMessage } from "../brocrew/UserMessage";
import { AssistantMessage } from "../brocrew/AssistantMessage";
import { call, type CoreError, type CoreResult } from "../core";

// The Chat view (feature:frontdoor, SPEC.md). FD-1: the composer sends a message to the core's
// router and shows which step picked the model. FD-8: the "Open in Claude" button shows when the
// route's pick is a Claude handoff; the press hands the packet's prompt to the shell's opener.

/** The router's pick, as the core names it (`Pick` in `router.rs`). */
export type Pick =
  | { Service: { service: string; model: string } }
  | { ClaudeHandoff: string }
  | { Wait: string };

export type ChatHandoffProps = {
  pick: Pick;
  /** The packet as `as_prompt` builds it (FD-8). */
  prompt: string;
  /** The shell's opener: starts a Claude desktop session seeded with the prompt. Unset: the button copies the prompt. */
  openInClaude?: (prompt: string) => void;
  /** Logs an implicit signal (FD-9). The press logs `opened-in-claude`. */
  logImplicit?: (signal: "opened-in-claude") => void;
};

/** FD-8: the button for a Claude handoff; nothing for any other pick. */
export function ChatHandoff({ pick, prompt, openInClaude, logImplicit }: ChatHandoffProps) {
  if (!("ClaudeHandoff" in pick)) return null;
  const press = () => {
    if (openInClaude) {
      openInClaude(prompt);
    } else {
      const clipboard = navigator.clipboard;
      if (clipboard) void clipboard.writeText(prompt).catch(() => undefined);
    }
    logImplicit?.("opened-in-claude");
  };
  return (
    <div data-part="handoff">
      <p>{pick.ClaudeHandoff}</p>
      <button type="button" onClick={press}>
        Open in Claude
      </button>
    </div>
  );
}

/** FD-12: a request the route held because this machine's loadout misses some of its kind's needs. */
export type Held = { kind: string; request: string; unmet: string[] };

/** FD-12: queues the held request as a fleet job another machine pulls (`chat_send_as_job`). */
export const sendAsJob = (held: Held): Promise<CoreResult<unknown>> =>
  call<unknown>("chat_send_as_job", { kind: held.kind, request: held.request, unmet: held.unmet });

export type ChatHeldProps = { held: Held; send?: (held: Held) => Promise<CoreResult<unknown>> };

/** FD-12: the needs this machine lacks, and the "Send as a fleet job" button. Nothing when none are unmet. */
export function ChatHeld({ held, send = sendAsJob }: ChatHeldProps) {
  const [busy, setBusy] = useState(false);
  const [sent, setSent] = useState(false);
  const [error, setError] = useState<CoreError | null>(null);
  if (held.unmet.length === 0) return null;
  const press = async () => {
    if (busy || sent) return;
    setBusy(true);
    setError(null);
    const r = await send(held);
    setBusy(false);
    if ("ok" in r) setSent(true);
    else setError(r.error);
  };
  return (
    <div data-part="held">
      <p>{`Held: this machine lacks ${held.unmet.join(", ")}.`}</p>
      {!sent && (
        <button type="button" disabled={busy} onClick={() => void press()}>
          Send as a fleet job
        </button>
      )}
      {sent && <p role="status">Sent as a fleet job. Another machine will pull it.</p>}
      {error && <p role="alert">{`${error.command} failed: ${error.lines.join(" / ")}`}</p>}
    </div>
  );
}

/** The router's answer to one message (FD-1): the step that picked the model, and the pick. */
/**
 * The router's answer to one message (FD-1): the step that picked the model, and the pick. When the
 * answer text is there too (`text`), the route carries the answer turn: its service, model and handoff.
 */
export type ChatRoute = {
  step: string;
  kind: string | null;
  pick: unknown;
  text?: string;
  service?: string;
  model?: string;
  handoff?: string | null;
};

/** One turn in the thread: the person's message, or the answer to it. */
export type ChatTurn = { role: "user" | "assistant"; text: string; service?: string; model?: string; step?: string; handoff?: string | null };

/** Sends one message to the core's router. Default: the `chat_route` command. */
export type ChatSend = (message: string) => Promise<CoreResult<ChatRoute>>;

const defaultSend: ChatSend = (message) => call<ChatRoute>("chat_route", { message });

export function Chat({
  handoff,
  held,
  send = defaultSend,
  resolveLink,
}: { handoff?: ChatHandoffProps; held?: Held; send?: ChatSend; resolveLink?: ResolveMessageLink } = {}) {
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const [route, setRoute] = useState<ChatRoute | null>(null);
  const [turns, setTurns] = useState<ChatTurn[]>([]);
  const [errors, setErrors] = useState<CoreError[]>([]);
  const inFlight = useRef(false);

  const onSend = async (message: string) => {
    if (inFlight.current) return;
    inFlight.current = true;
    setBusy(true);
    // The sent message shows at once, before the answer arrives.
    const sent: ChatTurn = { role: "user", text: message };
    setTurns((current) => [...current, sent]);
    const result = await send(message);
    inFlight.current = false;
    setBusy(false);
    if ("error" in result) {
      // The text stays in the draft to send again, so its turn comes out of the thread.
      setTurns((current) => current.filter((turn) => turn !== sent));
      setErrors([result.error]);
      return;
    }
    setErrors([]);
    setRoute(result.ok);
    const answer = result.ok;
    if (answer.text !== undefined) {
      const { text, service, model, step, handoff: moved } = answer;
      setTurns((current) => [...current, { role: "assistant", text, service, model, step, handoff: moved ?? null }]);
    }
    // Clear only the text that was sent; anything typed while it was in flight stays.
    setDraft((current) => (current === message ? "" : current));
  };

  return (
    <div>
      <ErrorBanner errors={errors} />
      {handoff || held ? (
        <section aria-label="Chat">
          {handoff && <ChatHandoff {...handoff} />}
          {held && <ChatHeld held={held} />}
        </section>
      ) : turns.length === 0 ? (
        <Empty view="Chat" />
      ) : null}
      {turns.length > 0 && (
        <section aria-label="Messages">
          {turns.map((turn, i) =>
            turn.role === "user" ? (
              <UserMessage key={i} text={turn.text} resolveLink={resolveLink} />
            ) : (
              <AssistantMessage
                key={i}
                text={turn.text}
                service={turn.service}
                model={turn.model}
                step={turn.step}
                handoff={turn.handoff}
                resolveLink={resolveLink}
              />
            ),
          )}
        </section>
      )}
      <PromptComposer value={draft} onChange={setDraft} onSend={onSend} busy={busy} />
      {route && (
        <p>{`Picked by: ${route.step}${route.kind ? ` (${route.kind})` : ""}`}</p>
      )}
    </div>
  );
}
