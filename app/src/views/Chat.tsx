import { useEffect, useRef, useState } from "react";
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
// FD-9 (#378): the view logs the implicit signals through `chat_implicit`: `opened-in-claude` on
// the button's press, `re-asked` when a message is sent again word for word, `copied-answer` when
// an answer's text is copied, and `abandoned` when the view closes on an answer that got no
// rating. Each needs a stored thread; a failed log is not shown (it is not something I did).
// FD-3 (#377): a reply the local Qwen lane gave because the picked pool was braked says so.
// FD-5 (#378): a turn that moved the chat shows the move's line, as the work tree does.

/** The router's pick, as the core names it (`Pick` in `router.rs`). */
export type Pick =
  | { Service: { service: string; model: string } }
  | { ClaudeHandoff: string }
  | { Wait: string };

/** The route's pick as the core serialises it on the wire (`ChatPick` in `frontdoor/chat.rs`). */
export type RoutePick =
  | { to: "service"; service: string; model: string }
  | { to: "claude-handoff"; reason: string; model?: string }
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

/**
 * FD-12: queues the held request as a fleet job another machine pulls (`chat_send_as_job`). With
 * `handoffId` (the open chat's thread) the job carries the chat's packet (FD-6).
 */
export const sendAsJob = (held: Held, handoffId?: string): Promise<CoreResult<unknown>> =>
  call<unknown>("chat_send_as_job", {
    kind: held.kind,
    request: held.request,
    unmet: held.unmet,
    ...(handoffId === undefined ? {} : { handoffId }),
  });

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
  /** The thread the answer was stored in (serde snake case, as the core names it). Send it back to continue the thread. */
  handoff_id?: string;
  text?: string;
  service?: string;
  model?: string;
  handoff?: string | null;
  /** FD-3: nothing was in budget, so the ask is queued in its thread (`handoff_id`); why. */
  queued?: string;
  /** FD-6 (#378): this turn moved the chat off another model, named `<service> <model>`. */
  moved_from?: string;
  /** FD-5 (#378): why the chat moved; with `moved_from` and the pick, the move's work-tree line. */
  moved_why?: string;
  /** FD-3 (#377): the picked pool was braked, so the local Qwen lane answered this turn; why. */
  fallback?: string;
  /** FD-8 (#378): a Claude handoff that moved an open chat: the packet the Claude session starts from. */
  packet?: string;
};

/** One turn in the thread: the person's message, or the answer to it. */
export type ChatTurn = { role: "user" | "assistant"; text: string; service?: string; model?: string; step?: string; handoff?: string | null; handoff_id?: string; kind?: string | null; unstored?: boolean };

/** Sends one message to the core's router. Default: the `chat_route` command. */
export type ChatSend = (message: string, handoffId?: string) => Promise<CoreResult<ChatRoute>>;

// `handoffId` names the open thread, so a follow-up continues it; none starts a new thread.
const defaultSend: ChatSend = (message, handoffId) =>
  call<ChatRoute>("chat_route", handoffId === undefined ? { message } : { message, handoffId });

/** What a thumbs press sends: the turn, its rating, and the answer's thread, service, model and kind. */
export type ChatRating = {
  turnIndex: number;
  rating: "up" | "down";
  handoffId: string;
  service: string;
  model: string;
  kind?: string;
};

/** Logs a thumbs press on an answer (FD-9). Default: the `chat_rate` command. */
export type ChatRate = (args: ChatRating) => Promise<CoreResult<null>>;

const defaultRate: ChatRate = (args) => call<null>("chat_rate", args);

/** The implicit signals the core logs (FD-9; `Implicit::name` in `sticky.rs`). */
export type ImplicitSignal = "opened-in-claude" | "re-asked" | "abandoned" | "copied-answer";

/** What an implicit signal sends: the thread, the signal, and the answer it is about. */
export type ChatImplicitArgs = { handoffId: string; signal: ImplicitSignal; service?: string; model?: string; kind?: string };

/** Logs an implicit signal (FD-9). Default: the `chat_implicit` command. */
export type ChatImplicit = (args: ChatImplicitArgs) => Promise<CoreResult<null>>;

const defaultImplicit: ChatImplicit = (args) => call<null>("chat_implicit", args);

/** FD-5: the move a reply made, as the work tree writes a line: `from → to: why`. Null: no move. */
export function moveLine(route: ChatRoute): string | null {
  if (!route.moved_from) return null;
  const pick = route.pick;
  const claude = pick?.to === "claude-handoff" ? (pick.model ? `claude/${pick.model}` : "claude") : null;
  const to = pick?.to === "service" ? `${pick.service}/${pick.model}` : claude ?? "not recorded";
  return `${route.moved_from.replace(" ", "/")} → ${to}: ${route.moved_why ?? "not recorded"}`;
}

export function Chat({
  handoff,
  held,
  send = defaultSend,
  rate = defaultRate,
  implicit = defaultImplicit,
  resolveLink,
}: {
  handoff?: ChatHandoffProps;
  held?: Held;
  send?: ChatSend;
  rate?: ChatRate;
  implicit?: ChatImplicit;
  resolveLink?: ResolveMessageLink;
} = {}) {
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
  // The thread the answers were stored in; sent back so the next message continues it.
  const [threadId, setThreadId] = useState<string | undefined>(undefined);

  // FD-9: log an implicit signal about `about` (an answer turn) in the open thread. Without a
  // stored thread there is nothing to log it in.
  const logImplicit = (signal: ImplicitSignal, about: ChatTurn | undefined, thread: string | undefined) => {
    const handoffId = about?.handoff_id ?? thread;
    if (!handoffId) return;
    const args: ChatImplicitArgs = { handoffId, signal };
    if (about?.service) args.service = about.service;
    if (about?.model) args.model = about.model;
    if (about?.kind) args.kind = about.kind;
    void implicit(args).catch(() => undefined);
  };
  const lastAnswer = (all: ChatTurn[]) => [...all].reverse().find((turn) => turn.role === "assistant");

  // The view's state as the unmount sees it.
  const latest = useRef({ turns, ratings, threadId, logImplicit });
  latest.current = { turns, ratings, threadId, logImplicit };
  useEffect(
    () => () => {
      // FD-9: closing the view on an answer I did not rate is abandoning the chat.
      const now = latest.current;
      const last = now.turns.length - 1;
      if (last >= 0 && now.turns[last].role === "assistant" && now.ratings[last] === undefined) {
        now.logImplicit("abandoned", now.turns[last], now.threadId);
      }
    },
    [],
  );

  const onRate = async (turnIndex: number, rating: "up" | "down") => {
    if (ratings[turnIndex] === rating) return;
    setRatings((current) => ({ ...current, [turnIndex]: rating }));
    const turn = turns[turnIndex];
    if (!turn || !turn.handoff_id || !turn.service || !turn.model) return;
    // FD-9 (#372): the core checks the index against the turns its thread stored, and a message
    // handed to Claude is shown here but not stored there, so those are not counted.
    const storedIndex = turns.slice(0, turnIndex).filter((t) => !t.unstored).length;
    const args: ChatRating = { turnIndex: storedIndex, rating, handoffId: turn.handoff_id, service: turn.service, model: turn.model };
    if (turn.kind) args.kind = turn.kind;
    const result = await rate(args);
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
    const result = await send(message, threadId);
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
    setRouted(pick && pick.to === "claude-handoff" ? { reason: pick.reason, prompt: result.ok.packet ?? message } : null);
    const answer = result.ok;
    // Any reply that names its thread continues it, a queued ask's too (FD-3).
    if (answer.handoff_id) setThreadId(answer.handoff_id);
    // FD-9: the same message sent again, word for word, is a re-ask of the answer it got.
    const before = turns.filter((turn) => turn.role === "user").pop();
    if (before && before.text.trim() === message.trim()) {
      logImplicit("re-asked", lastAnswer(turns), answer.handoff_id ?? threadId);
    }
    // A Claude handoff stores no turn in the thread (FD-8); an answer or a queued ask does.
    if (answer.text === undefined && !answer.queued) {
      setTurns((current) => current.map((turn) => (turn === sent ? { ...sent, unstored: true } : turn)));
    }
    if (answer.text !== undefined) {
      const { text, service, model, step, handoff: moved, handoff_id, kind } = answer;
      setTurns((current) => [
        ...current,
        { role: "assistant", text, service, model, step, handoff: moved ?? null, handoff_id, kind },
      ]);
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
            routed && (
              <ChatHandoff
                pick={{ ClaudeHandoff: routed.reason }}
                prompt={routed.prompt}
                logImplicit={(signal) => logImplicit(signal, lastAnswer(turns), threadId)}
              />
            )
          )}
          {held && <ChatHeld held={held} send={(job) => sendAsJob(job, threadId)} />}
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
              // FD-9: copying from an answer is a signal about that answer.
              <div key={i} data-part="answer" onCopy={() => logImplicit("copied-answer", turn, threadId)}>
              <AssistantMessage
                text={turn.text}
                service={turn.service}
                model={turn.model}
                step={turn.step}
                handoff={turn.handoff}
                resolveLink={resolveLink}
                rating={ratings[i]}
                onRate={
                  turn.handoff_id && turn.service && turn.model ? (rating) => void onRate(i, rating) : undefined
                }
              />
              </div>
            ),
          )}
        </section>
      )}
      <PromptComposer value={draft} onChange={setDraft} onSend={onSend} busy={busy} />
      {route && (
        <p>{`Picked by: ${route.step}${route.kind ? ` (${route.kind})` : ""}`}</p>
      )}
      {route?.queued && <p role="status">{`Queued: ${route.queued}. The ask is kept in this chat.`}</p>}
      {route?.fallback && <p role="status" data-part="fallback">{`Qwen answered: ${route.fallback}.`}</p>}
      {route && moveLine(route) && <p data-part="move">{`Moved: ${moveLine(route)}`}</p>}
    </div>
  );
}
