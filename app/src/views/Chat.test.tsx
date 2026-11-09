// FD-1 (#177): the Chat composer. A send calls the router once with the text and shows the step
// that picked the model; Enter sends, Shift+Enter does not; an empty draft cannot go; an error
// shows in the banner and keeps the text.
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { vi } from "vitest";
import { Chat, type ChatRoute } from "./Chat";
import type { CoreResult } from "../core";

const ROUTE: ChatRoute = { step: "rules", kind: "fits the budget", pick: { Service: { service: "gemini", model: "flash" } } };
const OK: CoreResult<ChatRoute> = { ok: ROUTE };

function box() {
  return screen.getByRole("textbox", { name: "Message" }) as HTMLTextAreaElement;
}

test("typing and pressing Send calls send once with the text and shows the picking step", async () => {
  const send = vi.fn().mockResolvedValue(OK);
  render(<Chat send={send} />);
  fireEvent.change(box(), { target: { value: "fix the labeller" } });
  fireEvent.click(screen.getByRole("button", { name: "Send" }));
  expect(send).toHaveBeenCalledTimes(1);
  expect(send).toHaveBeenCalledWith("fix the labeller");
  expect(await screen.findByText("Picked by: rules (fits the budget)")).toBeTruthy();
});

test("the pick line has no kind when the route has none", async () => {
  const send = vi.fn().mockResolvedValue({ ok: { ...ROUTE, kind: null } });
  render(<Chat send={send} />);
  fireEvent.change(box(), { target: { value: "hi" } });
  fireEvent.click(screen.getByRole("button", { name: "Send" }));
  expect(await screen.findByText("Picked by: rules")).toBeTruthy();
});

test("Enter sends and Shift+Enter adds a newline without sending", async () => {
  const send = vi.fn().mockResolvedValue(OK);
  render(<Chat send={send} />);
  fireEvent.change(box(), { target: { value: "one" } });
  const shifted = fireEvent.keyDown(box(), { key: "Enter", shiftKey: true });
  expect(shifted).toBe(true); // not prevented: the newline goes in
  expect(send).not.toHaveBeenCalled();
  const plain = fireEvent.keyDown(box(), { key: "Enter" });
  expect(plain).toBe(false); // prevented: the send happens instead
  expect(send).toHaveBeenCalledTimes(1);
  expect(send).toHaveBeenCalledWith("one");
  await screen.findByText("Picked by: rules (fits the budget)");
});

test("empty and whitespace-only text leaves Send disabled and sends nothing", () => {
  const send = vi.fn().mockResolvedValue(OK);
  render(<Chat send={send} />);
  const button = screen.getByRole("button", { name: "Send" }) as HTMLButtonElement;
  expect(button.disabled).toBe(true);
  fireEvent.change(box(), { target: { value: "   \n\t " } });
  expect(button.disabled).toBe(true);
  fireEvent.keyDown(box(), { key: "Enter" });
  expect(send).not.toHaveBeenCalled();
});

test("a CoreResult error shows in the banner and keeps the typed text", async () => {
  const error: CoreResult<ChatRoute> = { error: { command: "chat_route", lines: ["router is down"] } };
  const send = vi.fn().mockResolvedValue(error);
  render(<Chat send={send} />);
  fireEvent.change(box(), { target: { value: "keep me" } });
  fireEvent.click(screen.getByRole("button", { name: "Send" }));
  const alert = await screen.findByRole("alert", { name: "Errors" });
  expect(alert.textContent).toContain("router is down");
  expect(box().value).toBe("keep me");
  expect(screen.queryByText(/Picked by:/)).toBeNull();
});

test("a chat with no turns shows the empty state and the composer", () => {
  render(<Chat />);
  expect(screen.getByText("no data yet")).toBeTruthy();
  expect(box()).toBeTruthy();
});

test("a second send while one is in flight is ignored", async () => {
  let finish: (r: CoreResult<ChatRoute>) => void = () => undefined;
  const send = vi.fn(() => new Promise<CoreResult<ChatRoute>>((resolve) => (finish = resolve)));
  render(<Chat send={send} />);
  fireEvent.change(box(), { target: { value: "once" } });
  fireEvent.keyDown(box(), { key: "Enter" });
  fireEvent.keyDown(box(), { key: "Enter" });
  expect(send).toHaveBeenCalledTimes(1);
  finish(OK);
  await waitFor(() => expect(box().value).toBe(""));
});
