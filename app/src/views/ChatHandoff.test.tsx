// FD-8 (#176): the Open in Claude button. Shown for a Claude handoff pick only; a press calls the
// opener once with the prompt and logs one `opened-in-claude` row; rendering alone does neither.
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { vi } from "vitest";
import { Chat, ChatHandoff, type Pick } from "./Chat";

const PROMPT = "Summary\nfix the labeller\n\nLast turns\nuser: why\n\nLinks\ndocs/a.md";
const HANDOFF: Pick = { ClaudeHandoff: "big ask with tools" };
const SERVICE: Pick = { Service: { service: "gemini", model: "gemini-flash" } };

test("a Claude handoff pick shows the button, with its reason", () => {
  render(<ChatHandoff pick={HANDOFF} prompt={PROMPT} />);
  expect(screen.getByRole("button", { name: "Open in Claude" })).toBeTruthy();
  expect(screen.getByText("big ask with tools")).toBeTruthy();
});

test("a service pick has no button", () => {
  render(<ChatHandoff pick={SERVICE} prompt={PROMPT} />);
  expect(screen.queryByRole("button", { name: "Open in Claude" })).toBeNull();
});

test("a wait pick has no button", () => {
  render(<ChatHandoff pick={{ Wait: "queued for budget" }} prompt={PROMPT} />);
  expect(screen.queryByRole("button")).toBeNull();
});

test("rendering alone calls neither the opener nor the logger", () => {
  const openInClaude = vi.fn();
  const logImplicit = vi.fn();
  render(
    <ChatHandoff pick={HANDOFF} prompt={PROMPT} openInClaude={openInClaude} logImplicit={logImplicit} />,
  );
  expect(openInClaude).not.toHaveBeenCalled();
  expect(logImplicit).not.toHaveBeenCalled();
});

test("a press calls the opener once with the prompt and logs one opened-in-claude row", () => {
  const openInClaude = vi.fn();
  const logImplicit = vi.fn();
  render(
    <ChatHandoff pick={HANDOFF} prompt={PROMPT} openInClaude={openInClaude} logImplicit={logImplicit} />,
  );
  fireEvent.click(screen.getByRole("button", { name: "Open in Claude" }));
  expect(openInClaude).toHaveBeenCalledTimes(1);
  expect(openInClaude).toHaveBeenCalledWith(PROMPT);
  expect(logImplicit).toHaveBeenCalledTimes(1);
  expect(logImplicit).toHaveBeenCalledWith("opened-in-claude");
});

test("with no opener, a press copies the prompt to the clipboard", () => {
  const writeText = vi.fn().mockResolvedValue(undefined);
  Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
  render(<ChatHandoff pick={HANDOFF} prompt={PROMPT} />);
  fireEvent.click(screen.getByRole("button", { name: "Open in Claude" }));
  expect(writeText).toHaveBeenCalledWith(PROMPT);
});

test("the chat without a handoff shows the empty state", () => {
  render(<Chat />);
  expect(screen.getByText("no data yet")).toBeTruthy();
});

test("the chat with a handoff shows the button", () => {
  render(<Chat handoff={{ pick: HANDOFF, prompt: PROMPT }} />);
  expect(screen.getByRole("button", { name: "Open in Claude" })).toBeTruthy();
});

// #294: the chat builds the handoff itself from the route it just got.
const REASON = "big with tools: the Claude subscription";
const handoffRoute = { ok: { step: "budget rule", kind: null, pick: { to: "claude-handoff", reason: REASON } } } as const;
const serviceRoute = { ok: { step: "rules", kind: null, pick: { to: "service", service: "gemini", model: "flash" } } } as const;

async function sendMessage(text: string) {
  fireEvent.change(screen.getByRole("textbox"), { target: { value: text } });
  fireEvent.click(screen.getByRole("button", { name: "Send" }));
  await screen.findByText(/Picked by:/);
}

test("a sent message routed to a Claude handoff shows the button and the reason", async () => {
  render(<Chat send={vi.fn().mockResolvedValue(handoffRoute)} />);
  await sendMessage("plan the labeller rewrite");
  expect(screen.getByRole("button", { name: "Open in Claude" })).toBeTruthy();
  expect(screen.getByText(REASON)).toBeTruthy();
  expect(screen.getByText("plan the labeller rewrite")).toBeTruthy();
});

test("a sent message routed to a service has no button", async () => {
  render(<Chat send={vi.fn().mockResolvedValue(serviceRoute)} />);
  await sendMessage("hello");
  expect(screen.queryByRole("button", { name: "Open in Claude" })).toBeNull();
});

test("a later service pick removes the button", async () => {
  const send = vi.fn().mockResolvedValueOnce(handoffRoute).mockResolvedValueOnce(serviceRoute);
  render(<Chat send={send} />);
  await sendMessage("first");
  expect(screen.getByRole("button", { name: "Open in Claude" })).toBeTruthy();
  fireEvent.change(screen.getByRole("textbox"), { target: { value: "second" } });
  fireEvent.click(screen.getByRole("button", { name: "Send" }));
  await waitFor(() => expect(screen.queryByRole("button", { name: "Open in Claude" })).toBeNull());
});

test("pressing the routed button copies the sent message", async () => {
  const writeText = vi.fn().mockResolvedValue(undefined);
  Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
  render(<Chat send={vi.fn().mockResolvedValue(handoffRoute)} />);
  await sendMessage("plan the labeller rewrite");
  fireEvent.click(screen.getByRole("button", { name: "Open in Claude" }));
  expect(writeText).toHaveBeenCalledWith("plan the labeller rewrite");
});
