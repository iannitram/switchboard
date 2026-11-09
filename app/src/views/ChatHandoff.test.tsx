// FD-8 (#176): the Open in Claude button. Shown for a Claude handoff pick only; a press calls the
// opener once with the prompt and logs one `opened-in-claude` row; rendering alone does neither.
import { fireEvent, render, screen } from "@testing-library/react";
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
