// FD-1 (#178): the Chat message list. A sent message shows as a user turn before its answer; an answer
// shows its text, a "service · model · picked by step" line, and the handoff reason when the chat moved;
// turns stay in the order sent, a long thread keeps every turn, and a brocrew:// link goes through the resolver.
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { vi } from "vitest";
import { Chat, type ChatRoute, type ChatSend } from "./Chat";
import type { CoreResult } from "../core";
import type { LinkResolution } from "../brocrew/ipc-types";

function box() {
  return screen.getByRole("textbox", { name: "Message" }) as HTMLTextAreaElement;
}

function sendButton() {
  return screen.getByRole("button", { name: "Send" });
}

function typeAndSend(text: string) {
  fireEvent.change(box(), { target: { value: text } });
  fireEvent.click(sendButton());
}

/** The turns in the message list, in DOM order: each is "user" or "assistant" with its text. */
function turns(container: HTMLElement): string[] {
  return Array.from(container.querySelectorAll("[data-role]")).map((el) => `${el.getAttribute("data-role")}:${el.textContent}`);
}

const ANSWER: ChatRoute = {
  step: "rules",
  kind: null,
  pick: { Service: { service: "gemini", model: "flash" } },
  text: "Use the labeller's first list.",
  service: "gemini",
  model: "flash",
  handoff: null,
};

test("a sent message shows as a user turn before the answer arrives", async () => {
  let finish: (r: CoreResult<ChatRoute>) => void = () => undefined;
  const send: ChatSend = vi.fn(() => new Promise<CoreResult<ChatRoute>>((resolve) => (finish = resolve)));
  const { container } = render(<Chat send={send} />);
  typeAndSend("fix the labeller");
  expect(turns(container)).toEqual(["user:Youfix the labeller"]);
  finish({ ok: ANSWER });
  expect(await screen.findByText("Use the labeller's first list.")).toBeTruthy();
  expect(turns(container)).toEqual(["user:Youfix the labeller", "assistant:Use the labeller's first list.gemini · flash · picked by rules"]);
});

test("an answer shows its service, model and picking step", async () => {
  const send = vi.fn().mockResolvedValue({ ok: ANSWER });
  render(<Chat send={send} />);
  typeAndSend("hi");
  expect(await screen.findByText("gemini · flash · picked by rules")).toBeTruthy();
  expect(screen.queryByText(/^moved:/)).toBeNull();
});

test("an answer with a handoff shows the reason it moved", async () => {
  const send = vi.fn().mockResolvedValue({ ok: { ...ANSWER, handoff: "big ask with tools" } });
  render(<Chat send={send} />);
  typeAndSend("refactor the packet");
  expect(await screen.findByText("moved: big ask with tools")).toBeTruthy();
});

test("a line with an unknown service and model names only what it knows", async () => {
  const send = vi.fn().mockResolvedValue({ ok: { step: "labeller", kind: null, pick: null, text: "ok" } });
  render(<Chat send={send} />);
  typeAndSend("hi");
  expect(await screen.findByText("picked by labeller")).toBeTruthy();
});

test("turns stay in the order sent and a long thread keeps every turn", async () => {
  const send: ChatSend = vi.fn(async (message) => ({ ok: { ...ANSWER, text: `answer to ${message}` } }));
  const { container } = render(<Chat send={send} />);
  const count = 30;
  for (let i = 1; i <= count; i++) {
    typeAndSend(`message ${i}`);
    await screen.findByText(`answer to message ${i}`);
  }
  const all = turns(container);
  expect(all).toHaveLength(count * 2);
  for (let i = 1; i <= count; i++) {
    expect(all[(i - 1) * 2]).toBe(`user:Youmessage ${i}`);
    expect(all[(i - 1) * 2 + 1]).toContain(`answer to message ${i}`);
  }
});

test("a failed send takes its user turn out and keeps the text in the composer", async () => {
  const error: CoreResult<ChatRoute> = { error: { command: "chat_route", lines: ["router is down"] } };
  const send = vi.fn().mockResolvedValue(error);
  const { container } = render(<Chat send={send} />);
  typeAndSend("keep me");
  await screen.findByRole("alert", { name: "Errors" });
  expect(turns(container)).toEqual([]);
  expect(box().value).toBe("keep me");
});

test("a brocrew:// link in a turn goes through the resolver and says why it cannot open", async () => {
  const deleted: LinkResolution = { state: "deleted", kind: "conversation", title: null, status: null, opens: null };
  const resolveLink = vi.fn().mockResolvedValue(deleted);
  const send = vi.fn().mockResolvedValue({ ok: { ...ANSWER, text: "see brocrew://conversation/abc" } });
  render(<Chat send={send} resolveLink={resolveLink} />);
  typeAndSend("where was that");
  const link = await screen.findByRole("link", { name: "Conversation link" });
  expect(link.getAttribute("data-brocrew-target")).toBe("brocrew://conversation/abc");
  fireEvent.click(link);
  expect(await screen.findByText("This conversation was deleted.")).toBeTruthy();
  expect(resolveLink).toHaveBeenCalledWith("brocrew://conversation/abc");
});

test("without a resolver a link in a turn stays plain text", async () => {
  const send = vi.fn().mockResolvedValue({ ok: { ...ANSWER, text: "see brocrew://agent/x" } });
  render(<Chat send={send} />);
  typeAndSend("who");
  expect(await screen.findByText(/see brocrew:\/\/agent\/x/)).toBeTruthy();
  expect(screen.queryByRole("link")).toBeNull();
});

test("a rejected resolver shows the plain failure sentence", async () => {
  const resolveLink = vi.fn().mockRejectedValue(new Error("gone"));
  const send = vi.fn().mockResolvedValue({ ok: { ...ANSWER, text: "brocrew://piece/9" } });
  render(<Chat send={send} resolveLink={resolveLink} />);
  typeAndSend("what");
  fireEvent.click(await screen.findByRole("link", { name: "Conversation link" }));
  await waitFor(() => expect(screen.getByText("Couldn't open this link.")).toBeTruthy());
});
