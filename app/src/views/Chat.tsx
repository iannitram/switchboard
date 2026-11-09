import { Empty } from "./Empty";

// The Chat view (feature:frontdoor, SPEC.md). FD-8: the "Open in Claude" button shows when the
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

export function Chat({ handoff }: { handoff?: ChatHandoffProps } = {}) {
  if (!handoff) return <Empty view="Chat" />;
  return (
    <section aria-label="Chat">
      <ChatHandoff {...handoff} />
    </section>
  );
}
