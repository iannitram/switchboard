/// <reference types="vite/client" />
// BUD-1 (#24): the Budget panel. Locked tests (R28): the builder never edits this file.
import { act, render, screen, within } from "@testing-library/react";
import { vi } from "vitest";
import codeMap from "../../../docs/code-map.md?raw";
import { call } from "../core";
import { listen } from "@tauri-apps/api/event";
import { Budget, BudgetPanel, type PoolRow } from "./Budget";

vi.mock("../core", () => ({ call: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

function deepFreeze<T>(v: T): T {
  if (v && typeof v === "object") {
    Object.values(v as object).forEach(deepFreeze);
    Object.freeze(v);
  }
  return v;
}

const ROWS: PoolRow[] = deepFreeze([
  { pool: "chatgpt", pct: 98.6, pace: 81.4, delta: 17.2, verdict: "brake", reading_age_min: 138, stale: true, why: "reading is 138 min old (limit 120)" },
  { pool: "claude", pct: 2, pace: 3, delta: -1, verdict: "on-pace", reading_age_min: 65, stale: false, why: "claude 2.0% vs pace 3.0%" },
  { pool: "qwen", pct: null, pace: null, delta: null, verdict: "not recorded", reading_age_min: null, stale: false, why: "" },
]);

function part(row: HTMLElement, name: string): HTMLElement {
  const el = row.querySelector<HTMLElement>(`[data-part="${name}"]`);
  if (!el) throw new Error(`no ${name} in ${row.getAttribute("aria-label")}`);
  return el;
}

beforeEach(() => {
  vi.mocked(call).mockReset();
  vi.mocked(listen).mockReset();
  vi.mocked(listen).mockResolvedValue(() => {});
});

test("no rows shows the empty state", () => {
  render(<BudgetPanel rows={[]} />);
  expect(screen.getByText("no data yet")).toBeTruthy();
});

test("one collapsible row per pool, in the order given", () => {
  render(<BudgetPanel rows={ROWS} />);
  const rows = screen.getAllByRole("group");
  expect(rows.map((r) => r.getAttribute("aria-label"))).toEqual(["chatgpt", "claude", "qwen"]);
  expect(rows.every((r) => r.tagName === "DETAILS")).toBe(true);
});

test("a row shows used, pace, points over or under, verdict and reading age", () => {
  render(<BudgetPanel rows={ROWS} />);
  const chatgpt = screen.getByRole("group", { name: "chatgpt" });
  expect(part(chatgpt, "pct").textContent).toBe("98.6%");
  expect(part(chatgpt, "pace").textContent).toBe("pace 81.4%");
  expect(part(chatgpt, "delta").textContent).toBe("+17.2 pts");
  expect(part(chatgpt, "verdict").textContent).toBe("brake");
  expect(part(chatgpt, "age").textContent).toBe("138 min old");
  const claude = screen.getByRole("group", { name: "claude" });
  expect(part(claude, "pct").textContent).toBe("2.0%");
  expect(part(claude, "delta").textContent).toBe("-1.0 pts");
});

test("the bar is as wide as the percent used and the pace line sits at the pace", () => {
  render(<BudgetPanel rows={ROWS} />);
  const chatgpt = screen.getByRole("group", { name: "chatgpt" });
  expect(part(chatgpt, "bar").style.width).toBe("98.6%");
  expect(part(chatgpt, "paceline").style.left).toBe("81.4%");
});

test("a bar over 100 percent is drawn at 100", () => {
  const over = deepFreeze([{ ...ROWS[1], pool: "gemini", pct: 120, pace: 130 }]);
  render(<BudgetPanel rows={over} />);
  const gemini = screen.getByRole("group", { name: "gemini" });
  expect(part(gemini, "bar").style.width).toBe("100%");
  expect(part(gemini, "paceline").style.left).toBe("100%");
});

test("a missing number reads not recorded and draws no bar", () => {
  render(<BudgetPanel rows={ROWS} />);
  const qwen = screen.getByRole("group", { name: "qwen" });
  expect(part(qwen, "pct").textContent).toBe("not recorded");
  expect(part(qwen, "pace").textContent).toBe("pace not recorded");
  expect(part(qwen, "delta").textContent).toBe("not recorded");
  expect(part(qwen, "age").textContent).toBe("not recorded");
  expect(part(qwen, "bar").style.width).toBe("0%");
  expect(qwen.querySelector('[data-part="paceline"]')).toBeNull();
});

test("a stale row is greyed, says stale and is open on its reason", () => {
  render(<BudgetPanel rows={ROWS} />);
  const chatgpt = screen.getByRole("group", { name: "chatgpt" });
  expect(chatgpt.classList.contains("stale")).toBe(true);
  expect(part(chatgpt, "stale").textContent).toBe("stale");
  expect(chatgpt.hasAttribute("open")).toBe(true);
  expect(part(chatgpt, "why").textContent).toBe("reading is 138 min old (limit 120)");
  const claude = screen.getByRole("group", { name: "claude" });
  expect(claude.classList.contains("stale")).toBe(false);
  expect(claude.querySelector('[data-part="stale"]')).toBeNull();
  expect(claude.hasAttribute("open")).toBe(false);
  expect(part(claude, "why").textContent).toBe("claude 2.0% vs pace 3.0%");
});

test("the view starts empty, then loads budget_rows from the core", async () => {
  vi.mocked(call).mockResolvedValue({ ok: ROWS });
  render(<Budget />);
  expect(screen.getByText("no data yet")).toBeTruthy();
  expect(await screen.findByRole("group", { name: "chatgpt" })).toBeTruthy();
  expect(call).toHaveBeenCalledWith("budget_rows");
});

test("a core error is shown in place with its command and lines", async () => {
  vi.mocked(call).mockResolvedValue({ error: { command: "fleet budget --json", lines: ["exit 2", "no answer"] } });
  render(<Budget />);
  const alert = await screen.findByRole("alert");
  expect(within(alert).getByText("fleet budget --json failed")).toBeTruthy();
  expect(within(alert).getByText("exit 2")).toBeTruthy();
  expect(within(alert).getByText("no answer")).toBeTruthy();
});

test("rows pushed on budget-rows replace the list", async () => {
  vi.mocked(call).mockResolvedValue({ ok: ROWS });
  render(<Budget />);
  await screen.findByRole("group", { name: "chatgpt" });
  expect(vi.mocked(listen).mock.calls[0][0]).toBe("budget-rows");
  const handler = vi.mocked(listen).mock.calls[0][1];
  act(() => {
    handler({ event: "budget-rows", id: 1, payload: deepFreeze([{ ...ROWS[1], pct: 4 }]) });
  });
  expect(screen.queryByRole("group", { name: "chatgpt" })).toBeNull();
  expect(part(screen.getByRole("group", { name: "claude" }), "pct").textContent).toBe("4.0%");
});

test("leaving the view stops listening", async () => {
  const unlisten = vi.fn();
  vi.mocked(listen).mockResolvedValue(unlisten);
  vi.mocked(call).mockResolvedValue({ ok: ROWS });
  const { unmount } = render(<Budget />);
  await screen.findByRole("group", { name: "chatgpt" });
  unmount();
  await act(async () => {});
  expect(unlisten).toHaveBeenCalledTimes(1);
});

test("the code map marks the panel present", () => {
  expect(codeMap).toContain("- BUD-1 panel (`BudgetPanel`, `Budget`): present.");
});
