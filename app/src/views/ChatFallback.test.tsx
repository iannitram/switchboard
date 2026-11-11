// FD-3 (#377): a reply the local Qwen lane gave because the picked pool was braked says so.
// FD-5 (#378): a turn that moved the chat shows the move's line, as the work tree writes it.
// FD-6 (#378): a held request sent as a fleet job from an open chat names the chat's thread.
import { act, fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, expect, test, vi } from "vitest";
import { call, type CoreResult } from "../core";
import { Chat, moveLine, sendAsJob, type ChatRoute, type Held } from "./Chat";

vi.mock("../core", () => ({ call: vi.fn() }));
const mockCall = vi.mocked(call);

const REASON = "nothing else is in budget (waiting for budget: general-question), so the local Qwen lane answers";
const BASE: ChatRoute = {
  step: "labeller",
  kind: "general-question",
  pick: { to: "service", service: "qwen", model: "qwen-small" },
  handoff_id: "t",
  text: "Tabs.",
  service: "qwen",
  model: "qwen-small",
};
const HELD: Held = { kind: "web-research", request: "find the docs", unmet: ["web"] };

async function ask(text: string) {
  const box = screen.getByRole("textbox", { name: "Message" }) as HTMLTextAreaElement;
  fireEvent.change(box, { target: { value: text } });
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
  });
}

beforeEach(() => {
  mockCall.mockReset();
});

test("a fallback answer says Qwen answered and why; a plain answer says nothing", async () => {
  const send = vi
    .fn()
    .mockResolvedValueOnce({ ok: { ...BASE, fallback: REASON } })
    .mockResolvedValueOnce({ ok: BASE });
  const { container } = render(<Chat send={send} implicit={vi.fn().mockResolvedValue({ ok: null })} />);
  await ask("tabs or spaces");
  expect(container.querySelector('[data-part="fallback"]')?.textContent).toBe(`Qwen answered: ${REASON}.`);
  expect(container.querySelector('[data-part="move"]')).toBeNull();
  await ask("and for yaml");
  expect(container.querySelector('[data-part="fallback"]')).toBeNull();
});

test("a turn that moved the chat shows the move's line", async () => {
  const moved: ChatRoute = {
    ...BASE,
    pick: { to: "service", service: "deepseek", model: "deepseek-chat" },
    moved_from: "qwen qwen-small",
    moved_why: "thumbs down",
  };
  const { container } = render(
    <Chat send={vi.fn().mockResolvedValue({ ok: moved })} implicit={vi.fn().mockResolvedValue({ ok: null })} />,
  );
  await ask("what is a functor");
  expect(container.querySelector('[data-part="move"]')?.textContent).toBe(
    "Moved: qwen/qwen-small → deepseek/deepseek-chat: thumbs down",
  );
});

test("moveLine names where the chat went, or nothing when it did not move", () => {
  expect(moveLine(BASE)).toBeNull();
  const handoff: ChatRoute = {
    step: "explicit ask claude",
    kind: null,
    pick: { to: "claude-handoff", reason: "ask claude" },
    moved_from: "qwen qwen-small",
    moved_why: 'I said "ask Claude"',
  };
  // #433 (was `→ claude`): a handoff Switchboard chose no model for says so, as the work tree does.
  expect(moveLine(handoff)).toBe('qwen/qwen-small → claude/model not chosen by Switchboard: I said "ask Claude"');
  expect(moveLine({ ...handoff, moved_why: undefined })).toBe(
    "qwen/qwen-small → claude/model not chosen by Switchboard: not recorded",
  );
  // #428: the move names the Claude model it went to.
  const withModel: ChatRoute = { ...handoff, pick: { to: "claude-handoff", reason: "ask claude", model: "sonnet" } };
  expect(moveLine(withModel)).toBe('qwen/qwen-small → claude/sonnet: I said "ask Claude"');
});

test("sendAsJob names the chat's thread when there is one", async () => {
  mockCall.mockResolvedValue({ ok: {} });
  await sendAsJob(HELD);
  expect(mockCall).toHaveBeenLastCalledWith("chat_send_as_job", { kind: "web-research", request: "find the docs", unmet: ["web"] });
  await sendAsJob(HELD, "t");
  expect(mockCall).toHaveBeenLastCalledWith("chat_send_as_job", {
    kind: "web-research",
    request: "find the docs",
    unmet: ["web"],
    handoffId: "t",
  });
});

test("the held request's button sends the open chat's thread", async () => {
  mockCall.mockImplementation((command: string) =>
    Promise.resolve((command === "chat_route" ? { ok: BASE } : { ok: {} }) as CoreResult<unknown>) as never,
  );
  const view = render(<Chat held={HELD} />);
  await ask("tabs or spaces");
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Send as a fleet job" }));
  });
  expect(mockCall).toHaveBeenCalledWith("chat_send_as_job", {
    kind: "web-research",
    request: "find the docs",
    unmet: ["web"],
    handoffId: "t",
  });
  view.unmount();
});

// #433: a Claude handoff shows its model as the core labels it, or that Switchboard chose none.
test("a Claude handoff shows the core's model label", async () => {
  const handoff = (claude_label: string): CoreResult<ChatRoute> => ({
    ok: {
      step: "explicit ask claude",
      kind: null,
      pick: { to: "claude-handoff", reason: "ask claude" },
      claude_label,
    },
  });
  const send = vi
    .fn()
    .mockResolvedValueOnce(handoff("Claude (model not chosen by Switchboard)"))
    .mockResolvedValueOnce(handoff("Claude opus"));
  const { container } = render(<Chat send={send} />);
  await ask("ask Claude about it");
  expect(container.querySelector('[data-part="claude-model"]')?.textContent).toBe(
    "Claude (model not chosen by Switchboard)",
  );
  await ask("ask Claude Opus about it");
  expect(container.querySelector('[data-part="claude-model"]')?.textContent).toBe("Claude opus");
});
