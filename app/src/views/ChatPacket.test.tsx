// FD-8 (#378): a Claude handoff that moves an open chat carries the packet; "Open in Claude"
// copies the packet, not the bare message.
import { fireEvent, render, screen } from "@testing-library/react";
import { vi } from "vitest";
import { Chat, type ChatRoute } from "./Chat";
import type { CoreResult } from "../core";

const PACKET = "Summary\nWe talked about monads.\n\nLast turns\nuser: what is a monad\n\nLinks\n\n\nAsk\nask Claude to refactor it";

function reply(packet?: string): CoreResult<ChatRoute> {
  return {
    ok: {
      step: "explicit ask claude",
      kind: null,
      pick: { to: "claude-handoff", reason: "ask claude" },
      ...(packet === undefined ? {} : { handoff_id: "t", moved_from: "qwen qwen-small", packet }),
    },
  };
}

function ask(text: string) {
  const box = screen.getByRole("textbox", { name: "Message" }) as HTMLTextAreaElement;
  fireEvent.change(box, { target: { value: text } });
  fireEvent.click(screen.getByRole("button", { name: "Send" }));
}

test.each([
  ["a moved chat copies the packet", PACKET, PACKET],
  ["a new chat copies the message", undefined, "ask Claude to refactor it"],
])("%s", async (_name, packet, copied) => {
  const writeText = vi.fn().mockResolvedValue(undefined);
  Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
  render(<Chat send={vi.fn().mockResolvedValue(reply(packet))} />);
  ask("ask Claude to refactor it");
  fireEvent.click(await screen.findByRole("button", { name: "Open in Claude" }));
  expect(writeText).toHaveBeenCalledWith(copied);
});
