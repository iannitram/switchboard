// FD-12 (#429): the window hands the Chat view the request this machine holds. The real App is
// rendered over a mocked Tauri `invoke`: `chat_route` answers with a `held` request, the button
// shows in the Chat view, and a press goes through to `chat_send_as_job` once. The send does not
// block (owner ruling 2026-11-11): while it is out the button says "sending…" and the composer
// still sends; the result shows in the chat when filing ends.
import { act, fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, expect, test, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { App } from "./App";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const mockInvoke = vi.mocked(invoke);

const HELD = { kind: "web-research", request: "find the llama-swap docs", unmet: ["web"] };
const REPLY = {
  step: "labeller",
  kind: "web-research",
  pick: { to: "service", service: "chatgpt", model: "gpt-5" },
  handoff_id: "t-1",
  text: "Here is what I found.",
  service: "chatgpt",
  model: "gpt-5",
};

/** `chat_send_as_job` answers only when the test says so; the route answers with `held` for HELD's text. */
function world() {
  let file: (value: unknown) => void = () => undefined;
  let fail: (error: unknown) => void = () => undefined;
  mockInvoke.mockImplementation(async (cmd: string, args?: unknown) => {
    if (cmd === "chat_route") {
      const message = (args as { message: string }).message;
      return message === HELD.request ? { ...REPLY, held: HELD } : { ...REPLY, text: "Sure." };
    }
    if (cmd === "chat_send_as_job") {
      return new Promise((resolve, reject) => {
        file = resolve;
        fail = reject;
      });
    }
    return [];
  });
  return { file: (v: unknown) => file(v), fail: (e: unknown) => fail(e) };
}

const sends = () => mockInvoke.mock.calls.filter(([cmd]) => cmd === "chat_send_as_job");

async function ask(text: string) {
  const box = screen.getByRole("textbox", { name: "Message" }) as HTMLTextAreaElement;
  fireEvent.change(box, { target: { value: text } });
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
  });
}

async function openChat() {
  render(<App />);
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Chat" }));
  });
}

beforeEach(() => {
  mockInvoke.mockReset();
});

test("a held request from chat_route shows its offer in the Chat view, and a press files it once", async () => {
  const fleet = world();
  await openChat();
  expect(screen.queryByRole("button", { name: "Send as a fleet job" })).toBeNull();
  await ask(HELD.request);
  expect(screen.getByText("Held: this machine lacks web.")).toBeTruthy();

  fireEvent.click(screen.getByRole("button", { name: "Send as a fleet job" }));
  const sending = screen.getByRole("button", { name: "sending…" }) as HTMLButtonElement;
  expect(sending.disabled).toBe(true);
  fireEvent.click(sending);
  expect(sends()).toEqual([
    ["chat_send_as_job", { kind: "web-research", request: HELD.request, unmet: ["web"], handoffId: "t-1" }],
  ]);

  // The window stays usable while it is filed: the composer still sends.
  await ask("and one more thing");
  expect(screen.getByText("Sure.")).toBeTruthy();
  expect(sends()).toHaveLength(1);

  await act(async () => fleet.file({ kind: "web-research" }));
  expect(screen.getByRole("status").textContent).toBe("Sent as a fleet job. Another machine will pull it.");
  expect(sends()).toHaveLength(1);
});

test("a failed filing shows its error in the chat when it ends", async () => {
  const fleet = world();
  await openChat();
  await ask(HELD.request);
  fireEvent.click(screen.getByRole("button", { name: "Send as a fleet job" }));
  await act(async () => fleet.fail({ command: "chat_send_as_job", lines: ["fd-12: fleet did not queue the job (exit 75)"] }));
  expect(screen.getByRole("alert").textContent).toBe(
    "chat_send_as_job failed: fd-12: fleet did not queue the job (exit 75)",
  );
  expect((screen.getByRole("button", { name: "Send as a fleet job" }) as HTMLButtonElement).disabled).toBe(false);
});

test("a later reply with no held request keeps the offer; leaving the Chat view clears it", async () => {
  world();
  await openChat();
  await ask(HELD.request);
  expect(screen.getByRole("button", { name: "Send as a fleet job" })).toBeTruthy();
  await ask("something this machine can do");
  expect(screen.getByRole("button", { name: "Send as a fleet job" })).toBeTruthy();

  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Sky" }));
  });
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Chat" }));
  });
  expect(screen.queryByRole("button", { name: "Send as a fleet job" })).toBeNull();
});
