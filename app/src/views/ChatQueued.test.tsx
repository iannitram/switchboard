// FD-3 (#377): with nothing in budget the ask is queued in its thread. The chat says so, and the
// next message continues that thread.
import { fireEvent, render, screen } from "@testing-library/react";
import { vi } from "vitest";
import { Chat, type ChatRoute } from "./Chat";
import type { CoreResult } from "../core";

const QUEUED: CoreResult<ChatRoute> = {
  ok: {
    step: "labeller",
    kind: "quick-lookup",
    pick: { to: "wait", reason: "waiting for budget: quick-lookup" },
    handoff_id: "chat-1",
    queued: "waiting for budget: quick-lookup",
  },
};

function box() {
  return screen.getByRole("textbox", { name: "Message" }) as HTMLTextAreaElement;
}

test("a queued ask says so and the next message goes to the same thread", async () => {
  const send = vi.fn().mockResolvedValue(QUEUED);
  render(<Chat send={send} />);
  fireEvent.change(box(), { target: { value: "what is a monad" } });
  fireEvent.click(screen.getByRole("button", { name: "Send" }));
  expect(
    await screen.findByText("Queued: waiting for budget: quick-lookup. The ask is kept in this chat."),
  ).toBeTruthy();
  fireEvent.change(box(), { target: { value: "and a functor?" } });
  fireEvent.click(screen.getByRole("button", { name: "Send" }));
  expect(send).toHaveBeenLastCalledWith("and a functor?", "chat-1");
});
