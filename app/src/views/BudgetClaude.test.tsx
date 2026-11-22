import { render, screen } from "@testing-library/react";
import { vi } from "vitest";
import { call } from "../core";
import { listen } from "@tauri-apps/api/event";
import { Budget, BudgetPanel, type ClaudePool, type PoolRow } from "./Budget";

vi.mock("../core", () => ({ call: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

const ROWS: PoolRow[] = [
  { pool: "chatgpt", pct: 50, pace: 40, delta: 10, verdict: "on-pace", reading_age_min: 10, stale: false, why: "" },
  { pool: "claude", pct: 2, pace: 3, delta: -1, verdict: "on-pace", reading_age_min: 65, stale: false, why: "" },
];

beforeEach(() => {
  vi.mocked(call).mockReset();
  vi.mocked(listen).mockReset();
  vi.mocked(listen).mockResolvedValue(() => {});
});

test("shows Claude's reading and labelled estimate in a separate line", () => {
  const claude: ClaudePool = {
    reading_pct: 2,
    reading_age_min: 65,
    estimate_tokens_since: 1200,
    estimate_messages: 4,
  };
  render(<BudgetPanel rows={ROWS} claude={claude} />);
  const row = screen.getByRole("group", { name: "claude" }).parentElement!;
  expect(row.querySelector('[data-part="pct"]')?.textContent).toBe("2.0%");
  expect(row.querySelector('[data-part="age"]')?.textContent).toBe("65 min old");
  expect(row.querySelector('[data-part="estimate"]')?.textContent).toBe(
    "estimate: 1200 tokens in 4 messages since the reading",
  );
});

test("loads BudgetSnapshot with zero-message estimate when no logs exist", async () => {
  const claude: ClaudePool = {
    reading_pct: 2,
    reading_age_min: 65,
    estimate_tokens_since: 0,
    estimate_messages: 0,
  };
  vi.mocked(call).mockResolvedValue({ ok: { rows: ROWS, claude } });
  render(<Budget />);
  expect(await screen.findByText("estimate: 0 tokens in 0 messages since the reading")).toBeTruthy();
  const row = screen.getByRole("group", { name: "claude" }).parentElement!;
  expect(row.querySelector('[data-part="pct"]')?.textContent).toBe("2.0%");
});
