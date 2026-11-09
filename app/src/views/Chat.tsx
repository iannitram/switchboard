import { useRef, useState } from "react";
import { Empty } from "./Empty";
import { ErrorBanner } from "../ErrorBanner";
import { PromptComposer } from "../brocrew/PromptComposer";
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

/** The router's answer to one message (FD-1): the step that picked the model, and the pick. */
export type ChatRoute = { step: string; kind: string | null; pick: unknown };

/** Sends one message to the core's router. Default: the `chat_route` command. */
export type ChatSend = (message: string) => Promise<CoreResult<ChatRoute>>;

const defaultSend: ChatSend = (message) => call<ChatRoute>("chat_route", { message });

export function Chat({ handoff, send = defaultSend }: { handoff?: ChatHandoffProps; send?: ChatSend } = {}) {
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const [route, setRoute] = useState<ChatRoute | null>(null);
  const [errors, setErrors] = useState<CoreError[]>([]);
  const inFlight = useRef(false);

  const onSend = async (message: string) => {
    if (inFlight.current) return;
    inFlight.current = true;
    setBusy(true);
    const result = await send(message);
    inFlight.current = false;
    setBusy(false);
    if ("error" in result) {
      setErrors([result.error]);
      return;
    }
    setErrors([]);
    setRoute(result.ok);
    // Clear only the text that was sent; anything typed while it was in flight stays.
    setDraft((current) => (current === message ? "" : current));
  };

  return (
    <div>
      <ErrorBanner errors={errors} />
      {handoff ? (
        <section aria-label="Chat">
          <ChatHandoff {...handoff} />
        </section>
      ) : (
        <Empty view="Chat" />
      )}
      <PromptComposer value={draft} onChange={setDraft} onSend={onSend} busy={busy} />
      {route && (
        <p>{`Picked by: ${route.step}${route.kind ? ` (${route.kind})` : ""}`}</p>
      )}
    </div>
  );
}
