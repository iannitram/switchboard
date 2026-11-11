// FD-9 (#372 review): the core refuses a rating whose turn index is not an answer it stored, and
// a message handed to Claude is shown but never stored. A thumbs press after a handoff sends the
// index the thread has, so the core accepts it.
import { fireEvent, render, screen } from "@testing-library/react";
import { vi } from "vitest";
import { Chat, type ChatRoute } from "./Chat";
import type { CoreResult } from "../core";

const HANDOFF: CoreResult<ChatRoute> = {
  ok: { step: "budget rule", kind: null, pick: { to: "claude-handoff", reason: "big with tools" } },
};

const ANSWER: CoreResult<ChatRoute> = {
  ok: {
    step: "rules",
    kind: "coding",
    pick: { to: "service", service: "qwen", model: "qwen-small" },
    handoff_id: "chat-1",
    text: "A monad is a wrapper.",
    service: "qwen",
    model: "qwen-small",
    handoff: null,
  },
};

const QUEUED: CoreResult<ChatRoute> = {
  ok: {
    step: "budget rule",
    kind: null,
    pick: { to: "wait", reason: "waiting for budget" },
    handoff_id: "chat-1",
    queued: "waiting for budget",
  },
};

function ask(text: string) {
  fireEvent.change(screen.getByRole("textbox", { name: "Message" }), { target: { value: text } });
  fireEvent.click(screen.getByRole("button", { name: "Send" }));
}

test("a message handed to Claude is not counted in the rated turn's index", async () => {
  const send = vi.fn().mockResolvedValueOnce(HANDOFF).mockResolvedValueOnce(ANSWER);
  const rate = vi.fn(async () => ({ ok: null }) as CoreResult<null>);
  render(<Chat send={send} rate={rate} />);
  ask("plan the database migration step by step");
  await screen.findByRole("button", { name: "Open in Claude" });
  ask("what is a monad");
  await screen.findByText("A monad is a wrapper.");
  fireEvent.click(screen.getByRole("button", { name: "Thumbs up" }));
  // Shown: [handed-off ask, ask, answer]; stored in chat-1: [ask, answer].
  expect(rate).toHaveBeenCalledWith(expect.objectContaining({ turnIndex: 1, handoffId: "chat-1" }));
});

test("a queued ask is stored, so it is counted", async () => {
  const send = vi.fn().mockResolvedValueOnce(QUEUED).mockResolvedValueOnce(ANSWER);
  const rate = vi.fn(async () => ({ ok: null }) as CoreResult<null>);
  render(<Chat send={send} rate={rate} />);
  ask("what is a monad");
  await screen.findByRole("status");
  ask("what is a monad, again");
  await screen.findByText("A monad is a wrapper.");
  fireEvent.click(screen.getByRole("button", { name: "Thumbs down" }));
  // Stored in chat-1: [queued ask, ask, answer].
  expect(rate).toHaveBeenCalledWith(expect.objectContaining({ turnIndex: 2, rating: "down" }));
});
