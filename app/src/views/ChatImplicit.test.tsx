// FD-9 (#378): the Chat view's callers of `chat_implicit`. Pressing "Open in Claude", sending the
// same message again, copying an answer and closing the view on an unrated answer each log their
// signal, about the answer they concern, in the chat's stored thread; with no stored thread
// nothing is logged.
import { act, fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, expect, test, vi } from "vitest";
import { call, type CoreResult } from "../core";
import { Chat, type ChatImplicit, type ChatRoute } from "./Chat";

vi.mock("../core", () => ({ call: vi.fn() }));
const mockCall = vi.mocked(call);

const ANSWER: CoreResult<ChatRoute> = {
  ok: {
    step: "labeller",
    kind: "quick-lookup",
    pick: { to: "service", service: "qwen", model: "qwen-small" },
    handoff_id: "t",
    text: "A monad is a monoid.",
    service: "qwen",
    model: "qwen-small",
  },
};
const HANDOFF: CoreResult<ChatRoute> = {
  ok: {
    step: "explicit ask claude",
    kind: null,
    pick: { to: "claude-handoff", reason: "ask claude" },
    handoff_id: "t",
    moved_from: "qwen qwen-small",
    moved_why: 'I said "ask Claude"',
    packet: "Summary\nMonads.\n\nAsk\nask Claude to refactor it",
  },
};
const ABOUT = { handoffId: "t", service: "qwen", model: "qwen-small", kind: "quick-lookup" };

async function ask(text: string) {
  const box = screen.getByRole("textbox", { name: "Message" }) as HTMLTextAreaElement;
  fireEvent.change(box, { target: { value: text } });
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
  });
}

function logger() {
  return vi.fn<ChatImplicit>().mockResolvedValue({ ok: null });
}

beforeEach(() => {
  mockCall.mockReset();
  Object.defineProperty(navigator, "clipboard", { value: { writeText: vi.fn().mockResolvedValue(undefined) }, configurable: true });
});

test("pressing Open in Claude in a moved chat logs opened-in-claude about the answer it left", async () => {
  const implicit = logger();
  const send = vi.fn().mockResolvedValueOnce(ANSWER).mockResolvedValueOnce(HANDOFF);
  render(<Chat send={send} implicit={implicit} />);
  await ask("what is a monad");
  await ask("ask Claude to refactor it");
  expect(implicit).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Open in Claude" }));
  expect(implicit).toHaveBeenCalledTimes(1);
  expect(implicit).toHaveBeenCalledWith({ ...ABOUT, signal: "opened-in-claude" });
});

test("a new chat's handoff has no stored thread, so the press logs nothing", async () => {
  const implicit = logger();
  const bare: CoreResult<ChatRoute> = { ok: { step: "explicit ask claude", kind: null, pick: { to: "claude-handoff", reason: "ask claude" } } };
  const { unmount } = render(<Chat send={vi.fn().mockResolvedValue(bare)} implicit={implicit} />);
  await ask("ask Claude to refactor it");
  fireEvent.click(screen.getByRole("button", { name: "Open in Claude" }));
  unmount();
  expect(implicit).not.toHaveBeenCalled();
});

test("the same message sent again is a re-ask of the answer it got; another message is not", async () => {
  const implicit = logger();
  render(<Chat send={vi.fn().mockResolvedValue(ANSWER)} implicit={implicit} />);
  await ask("what is a monad");
  await ask("and a functor");
  expect(implicit).not.toHaveBeenCalled();
  await ask("  and a functor ");
  expect(implicit).toHaveBeenCalledTimes(1);
  expect(implicit).toHaveBeenCalledWith({ ...ABOUT, signal: "re-asked" });
});

test("a message that failed to send and is sent again is not a re-ask", async () => {
  const implicit = logger();
  const failed: CoreResult<ChatRoute> = { error: { command: "chat_route", lines: ["no key"] } };
  const send = vi.fn().mockResolvedValueOnce(failed).mockResolvedValueOnce(ANSWER);
  render(<Chat send={send} implicit={implicit} />);
  await ask("what is a monad");
  await ask("what is a monad");
  expect(implicit).not.toHaveBeenCalled();
});

test("copying from an answer logs copied-answer about that answer; copying my own message does not", async () => {
  const implicit = logger();
  render(<Chat send={vi.fn().mockResolvedValue(ANSWER)} implicit={implicit} />);
  await ask("what is a monad");
  fireEvent.copy(screen.getByText("what is a monad"));
  expect(implicit).not.toHaveBeenCalled();
  fireEvent.copy(screen.getByText("A monad is a monoid."));
  expect(implicit).toHaveBeenCalledTimes(1);
  expect(implicit).toHaveBeenCalledWith({ ...ABOUT, signal: "copied-answer" });
});

test("closing the view on an unrated answer logs abandoned; a rated one, or an empty chat, does not", async () => {
  const implicit = logger();
  const rate = vi.fn().mockResolvedValue({ ok: null });
  const first = render(<Chat send={vi.fn().mockResolvedValue(ANSWER)} implicit={implicit} rate={rate} />);
  await ask("what is a monad");
  first.unmount();
  expect(implicit).toHaveBeenCalledTimes(1);
  expect(implicit).toHaveBeenCalledWith({ ...ABOUT, signal: "abandoned" });

  implicit.mockClear();
  const second = render(<Chat send={vi.fn().mockResolvedValue(ANSWER)} implicit={implicit} rate={rate} />);
  await ask("what is a monad");
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Thumbs up" }));
  });
  second.unmount();
  render(<Chat implicit={implicit} />).unmount();
  expect(implicit).not.toHaveBeenCalled();
});

test("the default logger is the chat_implicit command, and a failed log shows no error", async () => {
  mockCall.mockResolvedValue({ error: { command: "chat_implicit", lines: ["no thread t"] } });
  render(<Chat send={vi.fn().mockResolvedValue(ANSWER)} />);
  await ask("what is a monad");
  await act(async () => {
    fireEvent.copy(screen.getByText("A monad is a monoid."));
  });
  expect(mockCall).toHaveBeenCalledWith("chat_implicit", { ...ABOUT, signal: "copied-answer" });
  expect(screen.queryByRole("alert")).toBeNull();
});
