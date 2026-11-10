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
// FD-9 (#179): each answer has a thumbs up and a thumbs down; a press calls `rate` (the core's
// rating row) and marks the button; a failed call shows the banner and clears the mark.

/** The router's pick, as the core names it (`Pick` in `router.rs`). */
export type Pick =
  | { Service: { service: string; model: string } }
  | { ClaudeHandoff: string }
  | { Wait: string };

/** The route's pick as the core serialises it on the wire (`ChatPick` in `frontdoor/chat.rs`). */
export type RoutePick =
  | { to: "service"; service: string; model: string }
  | { to: "claude-handoff"; reason: string }
  | { to: "wait"; reason: string };

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
  pick: RoutePick | null;
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

/** Logs a thumbs press on the answer at `turnIndex` (FD-9). Default: the `chat_rate` command. */
export type ChatRate = (turnIndex: number, rating: "up" | "down") => Promise<CoreResult<null>>;

const defaultRate: ChatRate = (turnIndex, rating) => call<null>("chat_rate", { turnIndex, rating });

export function Chat({
  handoff,
  held,
  send = defaultSend,
  rate = defaultRate,
  resolveLink,
}: { handoff?: ChatHandoffProps; held?: Held; send?: ChatSend; rate?: ChatRate; resolveLink?: ResolveMessageLink } = {}) {
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const [route, setRoute] = useState<ChatRoute | null>(null);
  // FD-8: the handoff the last route made, with the message that made it. The `handoff` prop wins.
  const [routed, setRouted] = useState<{ reason: string; prompt: string } | null>(null);
  const [turns, setTurns] = useState<ChatTurn[]>([]);
  const [errors, setErrors] = useState<CoreError[]>([]);
  // One rating per turn: the last press. Keyed by the turn's index in the thread.
  const [ratings, setRatings] = useState<Record<number, "up" | "down">>({});
  const inFlight = useRef(false);

  const onRate = async (turnIndex: number, rating: "up" | "down") => {
    if (ratings[turnIndex] === rating) return;
    setRatings((current) => ({ ...current, [turnIndex]: rating }));
    const result = await rate(turnIndex, rating);
    if ("error" in result) {
      // The mark goes, unless a later press has already moved it.
      setRatings((current) => {
        if (current[turnIndex] !== rating) return current;
        const next = { ...current };
        delete next[turnIndex];
        return next;
      });
      setErrors([result.error]);
    }
  };

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
    const pick = result.ok.pick;
    setRouted(pick && pick.to === "claude-handoff" ? { reason: pick.reason, prompt: message } : null);
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
      {handoff || routed || held ? (
        <section aria-label="Chat">
          {handoff ? (
            <ChatHandoff {...handoff} />
          ) : (
            routed && <ChatHandoff pick={{ ClaudeHandoff: routed.reason }} prompt={routed.prompt} />
          )}
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
                rating={ratings[i]}
                onRate={(rating) => void onRate(i, rating)}
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
