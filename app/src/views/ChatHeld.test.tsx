// FD-12 (#246): a held request in the Chat view says which needs this machine lacks and offers
// "Send as a fleet job". Locked test (R28); the builder never edits this file. The core is
// stubbed: the default send is `call("chat_send_as_job", { kind, request, unmet })`.
import { act, fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, expect, test, vi } from "vitest";
import { call, type CoreResult } from "../core";
import { Chat, ChatHeld, sendAsJob, type Held } from "./Chat";

vi.mock("../core", () => ({ call: vi.fn() }));
const mockCall = vi.mocked(call);

function deepFreeze<T>(value: T): T {
  if (value && typeof value === "object") {
    Object.values(value).forEach(deepFreeze);
    Object.freeze(value);
  }
  return value;
}

const HELD: Held = deepFreeze({ kind: "web-research", request: "find the llama-swap docs", unmet: ["cuda", "web"] });
const BUTTON = { name: "Send as a fleet job" };
const OK: CoreResult<unknown> = { ok: { kind: "web-research" } };
const FAIL: CoreResult<unknown> = {
  error: { command: "chat_send_as_job", lines: ["fleet error: python3 fleet.py attach", "exit 3"] },
};

/** A send that resolves only when the test says so. */
function pending() {
  let resolve: (r: CoreResult<unknown>) => void = () => undefined;
  const send = vi.fn(() => new Promise<CoreResult<unknown>>((r) => (resolve = r)));
  return { send, resolve: (r: CoreResult<unknown>) => resolve(r) };
}

beforeEach(() => {
  mockCall.mockReset();
});

test("a held request names the missing needs, in order, and offers the button", () => {
  render(<ChatHeld held={HELD} send={vi.fn()} />);
  expect(screen.getByText("Held: this machine lacks cuda, web.")).toBeTruthy();
  expect(screen.getByRole("button", BUTTON)).toBeTruthy();
});

test("one missing need is named alone", () => {
  render(<ChatHeld held={{ ...HELD, unmet: ["gh"] }} send={vi.fn()} />);
  expect(screen.getByText("Held: this machine lacks gh.")).toBeTruthy();
});

test("nothing unmet shows nothing", () => {
  const { container } = render(<ChatHeld held={{ ...HELD, unmet: [] }} send={vi.fn()} />);
  expect(container.innerHTML).toBe("");
});

test("rendering alone sends nothing", () => {
  const send = vi.fn();
  render(<ChatHeld held={HELD} send={send} />);
  expect(send).not.toHaveBeenCalled();
  expect(mockCall).not.toHaveBeenCalled();
});

test("a press sends the held request once", async () => {
  const send = vi.fn().mockResolvedValue(OK);
  render(<ChatHeld held={HELD} send={send} />);
  await act(async () => {
    fireEvent.click(screen.getByRole("button", BUTTON));
  });
  expect(send).toHaveBeenCalledTimes(1);
  expect(send).toHaveBeenCalledWith(HELD);
});

// #429 (owner ruling 2026-11-11): the send does not block; while it is out the button says
// "sending…" (this test found it by its old name) and a second press sends nothing.
test("while the send is out the button says sending…, is disabled and a second press sends nothing", async () => {
  const { send, resolve } = pending();
  render(<ChatHeld held={HELD} send={send} />);
  fireEvent.click(screen.getByRole("button", BUTTON));
  const button = screen.getByRole("button", { name: "sending…" }) as HTMLButtonElement;
  expect(button.disabled).toBe(true);
  fireEvent.click(button);
  expect(send).toHaveBeenCalledTimes(1);
  await act(async () => resolve(OK));
});

test("once sent it says so and the button is gone", async () => {
  const send = vi.fn().mockResolvedValue(OK);
  render(<ChatHeld held={HELD} send={send} />);
  await act(async () => {
    fireEvent.click(screen.getByRole("button", BUTTON));
  });
  expect(screen.getByRole("status").textContent).toBe("Sent as a fleet job. Another machine will pull it.");
  expect(screen.queryByRole("button", BUTTON)).toBeNull();
  expect(screen.queryByRole("alert")).toBeNull();
});

test("a failed send shows the core's error in place and the button can be pressed again", async () => {
  const send = vi.fn().mockResolvedValueOnce(FAIL).mockResolvedValueOnce(OK);
  render(<ChatHeld held={HELD} send={send} />);
  await act(async () => {
    fireEvent.click(screen.getByRole("button", BUTTON));
  });
  expect(screen.getByRole("alert").textContent).toBe(
    "chat_send_as_job failed: fleet error: python3 fleet.py attach / exit 3",
  );
  expect(screen.queryByRole("status")).toBeNull();
  const button = screen.getByRole("button", BUTTON) as HTMLButtonElement;
  expect(button.disabled).toBe(false);
  await act(async () => {
    fireEvent.click(button);
  });
  expect(send).toHaveBeenCalledTimes(2);
  expect(screen.queryByRole("alert")).toBeNull();
  expect(screen.getByRole("status")).toBeTruthy();
});

test("sendAsJob calls chat_send_as_job once with the kind, request and unmet needs", async () => {
  mockCall.mockResolvedValue(OK);
  await expect(sendAsJob(HELD)).resolves.toEqual(OK);
  expect(mockCall).toHaveBeenCalledTimes(1);
  expect(mockCall).toHaveBeenCalledWith("chat_send_as_job", {
    kind: "web-research",
    request: "find the llama-swap docs",
    unmet: ["cuda", "web"],
  });
});

test("with no send given, a press goes to chat_send_as_job once", async () => {
  mockCall.mockResolvedValue(OK);
  render(<ChatHeld held={HELD} />);
  await act(async () => {
    fireEvent.click(screen.getByRole("button", BUTTON));
  });
  expect(mockCall).toHaveBeenCalledTimes(1);
  expect(mockCall.mock.calls[0][0]).toBe("chat_send_as_job");
});

test("the Chat view with a held request shows the offer", () => {
  render(<Chat held={HELD} />);
  expect(screen.getByRole("region", { name: "Chat" })).toBeTruthy();
  expect(screen.getByText("Held: this machine lacks cuda, web.")).toBeTruthy();
  expect(screen.getByRole("button", BUTTON)).toBeTruthy();
});

test("the Chat view with neither a handoff nor a held request is still empty", () => {
  render(<Chat />);
  expect(screen.getByText("no data yet")).toBeTruthy();
  expect(screen.queryByRole("button", BUTTON)).toBeNull();
});
