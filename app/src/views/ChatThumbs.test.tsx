// FD-9 (#179): a thumbs up and a thumbs down on each answer in the Chat view. A press calls `rate` once and
// marks the button; the other button replaces the mark; a failed call shows the banner and clears the mark;
// user turns have no buttons.
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { vi } from "vitest";
import { Chat, type ChatRoute, type ChatSend } from "./Chat";
import type { CoreResult } from "../core";

function typeAndSend(text: string) {
  fireEvent.change(screen.getByRole("textbox", { name: "Message" }), { target: { value: text } });
  fireEvent.click(screen.getByRole("button", { name: "Send" }));
}

const ANSWER: ChatRoute = {
  step: "rules",
  kind: "coding",
  pick: { to: "service", service: "gemini", model: "flash" },
  handoff_id: "h-1",
  text: "Use the labeller's first list.",
  service: "gemini",
  model: "flash",
  handoff: null,
};

const okSend: ChatSend = vi.fn(async () => ({ ok: ANSWER }));

function up() {
  return screen.getByRole("button", { name: "Thumbs up" });
}

function down() {
  return screen.getByRole("button", { name: "Thumbs down" });
}

test("each answer has a thumbs up and a thumbs down; user turns have none", async () => {
  const { container } = render(<Chat send={okSend} rate={vi.fn(async () => ({ ok: null }))} />);
  typeAndSend("fix the labeller");
  await screen.findByText("Use the labeller's first list.");
  typeAndSend("and the packet");
  await waitFor(() => expect(screen.getAllByText("Use the labeller's first list.")).toHaveLength(2));

  expect(screen.getAllByRole("button", { name: "Thumbs up" })).toHaveLength(2);
  expect(screen.getAllByRole("button", { name: "Thumbs down" })).toHaveLength(2);
  const userTurns = container.querySelectorAll('[data-role="user"]');
  expect(userTurns.length).toBe(2);
  userTurns.forEach((turn) => expect(turn.querySelector("button")).toBeNull());
});

test("pressing thumbs down calls rate once with the turn index and marks it pressed", async () => {
  const rate = vi.fn(async () => ({ ok: null }) as CoreResult<null>);
  render(<Chat send={okSend} rate={rate} />);
  typeAndSend("hi");
  await screen.findByText("Use the labeller's first list.");

  fireEvent.click(down());
  expect(rate).toHaveBeenCalledTimes(1);
  expect(rate).toHaveBeenCalledWith({
    turnIndex: 1,
    rating: "down",
    handoffId: "h-1",
    service: "gemini",
    model: "flash",
    kind: "coding",
  });
  expect(down().getAttribute("aria-pressed")).toBe("true");
  expect(up().getAttribute("aria-pressed")).toBe("false");
});

test("pressing thumbs up after thumbs down calls rate with up and moves the mark", async () => {
  const rate = vi.fn(async () => ({ ok: null }) as CoreResult<null>);
  render(<Chat send={okSend} rate={rate} />);
  typeAndSend("hi");
  await screen.findByText("Use the labeller's first list.");

  fireEvent.click(down());
  fireEvent.click(up());
  expect(rate).toHaveBeenNthCalledWith(2, {
    turnIndex: 1,
    rating: "up",
    handoffId: "h-1",
    service: "gemini",
    model: "flash",
    kind: "coding",
  });
  expect(up().getAttribute("aria-pressed")).toBe("true");
  expect(down().getAttribute("aria-pressed")).toBe("false");
});

test("a failing rate shows the error banner and clears the mark", async () => {
  const rate = vi.fn(async () => ({ error: { command: "chat_rate", lines: ["no thread open"] } }) as CoreResult<null>);
  render(<Chat send={okSend} rate={rate} />);
  typeAndSend("hi");
  await screen.findByText("Use the labeller's first list.");

  fireEvent.click(up());
  expect(await screen.findByRole("alert")).toBeTruthy();
  expect(screen.getByText("chat_rate")).toBeTruthy();
  await waitFor(() => expect(up().getAttribute("aria-pressed")).toBe("false"));
  expect(down().getAttribute("aria-pressed")).toBe("false");
});

test("an answer without a thread id, service or model shows no thumbs", async () => {
  const bare: ChatSend = vi.fn(async () => ({ ok: { ...ANSWER, handoff_id: undefined } }));
  render(<Chat send={bare} rate={vi.fn(async () => ({ ok: null }))} />);
  typeAndSend("hi");
  await screen.findByText("Use the labeller's first list.");
  expect(screen.queryByRole("button", { name: "Thumbs up" })).toBeNull();
  expect(screen.queryByRole("button", { name: "Thumbs down" })).toBeNull();
});

test("the second send carries the thread id from the first reply", async () => {
  const send = vi.fn(async () => ({ ok: ANSWER }) as CoreResult<ChatRoute>);
  render(<Chat send={send} rate={vi.fn(async () => ({ ok: null }))} />);
  typeAndSend("one");
  await screen.findByText("Use the labeller's first list.");
  typeAndSend("two");
  await waitFor(() => expect(send).toHaveBeenCalledTimes(2));
  expect(send).toHaveBeenNthCalledWith(1, "one", undefined);
  expect(send).toHaveBeenNthCalledWith(2, "two", "h-1");
});
