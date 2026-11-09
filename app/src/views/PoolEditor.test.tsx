/// <reference types="vite/client" />
// BUD-2 (#26): edit a pool in place. Locked tests (R28): the builder never edits this file.
import { fireEvent, render, screen, within } from "@testing-library/react";
import { vi } from "vitest";
import codeMap from "../../../docs/code-map.md?raw";
import { call } from "../core";
import { listen } from "@tauri-apps/api/event";
import { Budget, BudgetPanel, PoolEditor, type PoolRow } from "./Budget";

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
  { pool: "chatgpt", pct: 98.6, pace: 81.4, delta: 17.2, verdict: "brake", reading_age_min: 138, stale: true, why: "stale" },
  { pool: "deepseek", pct: 40, pace: 50, delta: -10, verdict: "push", reading_age_min: 5, stale: false, why: "" },
]);

function editor() {
  const onSave = vi.fn();
  const onCancel = vi.fn();
  render(<PoolEditor pool="deepseek" onSave={onSave} onCancel={onCancel} />);
  const form = screen.getByRole("form", { name: "Edit deepseek" });
  const field = (label: string) => within(form).getByLabelText(label) as HTMLInputElement;
  const type = (label: string, value: string) => fireEvent.change(field(label), { target: { value } });
  return { onSave, onCancel, form, field, type };
}

beforeEach(() => {
  vi.mocked(call).mockReset();
  vi.mocked(listen).mockReset();
  vi.mocked(listen).mockResolvedValue(() => {});
});

test("the editor has plan, cap, reset and cost fields", () => {
  const { field } = editor();
  for (const label of ["Plan", "Cap ($)", "Reset", "Cost ($)"]) expect(field(label).tagName).toBe("INPUT");
  expect(field("Reset").placeholder).toBe("Sun 6:00am");
});

test("Enter saves only the fields filled in, numbers as numbers", () => {
  const { onSave, type, field } = editor();
  type("Cap ($)", "25");
  fireEvent.keyDown(field("Cap ($)"), { key: "Enter" });
  expect(onSave).toHaveBeenCalledTimes(1);
  expect(onSave.mock.calls[0][0]).toStrictEqual({ pool: "deepseek", cap: 25 });
});

test("the Save button saves plan, reset and cost, trimmed", () => {
  const { onSave, type, form } = editor();
  type("Plan", " Max 20x ");
  type("Reset", " Sun 6:00am ");
  type("Cost ($)", "12.5");
  fireEvent.click(within(form).getByRole("button", { name: "Save" }));
  expect(onSave).toHaveBeenCalledTimes(1);
  expect(onSave.mock.calls[0][0]).toStrictEqual({ pool: "deepseek", plan: "Max 20x", reset: "Sun 6:00am", cost_usd: 12.5 });
});

test("a cap or cost that is not a number 0 or more is refused in place", () => {
  const { onSave, type, field, form } = editor();
  for (const bad of ["abc", "-1"]) {
    type("Cap ($)", bad);
    fireEvent.keyDown(field("Cap ($)"), { key: "Enter" });
    expect(within(form).getByRole("alert").textContent).toBe("Cap must be a number 0 or more");
  }
  type("Cap ($)", "");
  type("Cost ($)", "x");
  fireEvent.keyDown(field("Cost ($)"), { key: "Enter" });
  expect(within(form).getByRole("alert").textContent).toBe("Cost must be a number 0 or more");
  expect(onSave).not.toHaveBeenCalled();
});

test("an empty edit says nothing to save", () => {
  const { onSave, field, form } = editor();
  fireEvent.keyDown(field("Plan"), { key: "Enter" });
  expect(within(form).getByRole("alert").textContent).toBe("Nothing to save");
  expect(onSave).not.toHaveBeenCalled();
});

test("Escape and the Cancel button cancel without saving", () => {
  const { onSave, onCancel, type, field, form } = editor();
  type("Cap ($)", "25");
  fireEvent.keyDown(field("Cap ($)"), { key: "Escape" });
  expect(onCancel).toHaveBeenCalledTimes(1);
  fireEvent.click(within(form).getByRole("button", { name: "Cancel" }));
  expect(onCancel).toHaveBeenCalledTimes(2);
  expect(onSave).not.toHaveBeenCalled();
});

test("the panel has no Edit buttons without onSave", () => {
  render(<BudgetPanel rows={ROWS} />);
  expect(screen.queryByRole("button", { name: "Edit chatgpt" })).toBeNull();
});

test("Edit opens the editor in that pool's row, one at a time", () => {
  render(<BudgetPanel rows={ROWS} onSave={vi.fn()} />);
  fireEvent.click(screen.getByRole("button", { name: "Edit chatgpt" }));
  const form = screen.getByRole("form", { name: "Edit chatgpt" });
  expect(form.closest("li")?.querySelector("details")?.getAttribute("aria-label")).toBe("chatgpt");
  fireEvent.click(screen.getByRole("button", { name: "Edit deepseek" }));
  expect(screen.queryByRole("form", { name: "Edit chatgpt" })).toBeNull();
  expect(screen.getByRole("form", { name: "Edit deepseek" })).toBeTruthy();
});

test("saving from a row calls onSave and closes the editor; Escape just closes it", () => {
  const onSave = vi.fn();
  render(<BudgetPanel rows={ROWS} onSave={onSave} />);
  fireEvent.click(screen.getByRole("button", { name: "Edit deepseek" }));
  const cap = within(screen.getByRole("form", { name: "Edit deepseek" })).getByLabelText("Cap ($)");
  fireEvent.change(cap, { target: { value: "25" } });
  fireEvent.keyDown(cap, { key: "Enter" });
  expect(onSave.mock.calls[0][0]).toStrictEqual({ pool: "deepseek", cap: 25 });
  expect(screen.queryByRole("form", { name: "Edit deepseek" })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Edit chatgpt" }));
  fireEvent.keyDown(within(screen.getByRole("form", { name: "Edit chatgpt" })).getByLabelText("Plan"), { key: "Escape" });
  expect(screen.queryByRole("form", { name: "Edit chatgpt" })).toBeNull();
  expect(onSave).toHaveBeenCalledTimes(1);
});

async function saveCapFromTheView() {
  render(<Budget />);
  fireEvent.click(await screen.findByRole("button", { name: "Edit deepseek" }));
  const cap = within(screen.getByRole("form", { name: "Edit deepseek" })).getByLabelText("Cap ($)");
  fireEvent.change(cap, { target: { value: "25" } });
  fireEvent.keyDown(cap, { key: "Enter" });
}

test("the view saves through budget_save_edit and shows the rows it returns", async () => {
  const after = deepFreeze([{ ...ROWS[1], pct: 20 }]);
  vi.mocked(call).mockImplementation(async (command) => ({ ok: command === "budget_rows" ? ROWS : after }));
  await saveCapFromTheView();
  expect(call).toHaveBeenCalledWith("budget_save_edit", { edit: { pool: "deepseek", cap: 25 } });
  await screen.findByText("20.0%");
  expect(screen.queryByRole("group", { name: "chatgpt" })).toBeNull();
});

test("a failed save shows the error in place and keeps the rows", async () => {
  vi.mocked(call).mockImplementation(async (command) =>
    command === "budget_rows" ? { ok: ROWS } : { error: { command: "budget_save_edit", lines: ["no pool deepseek in config.json"] } },
  );
  await saveCapFromTheView();
  const alert = await screen.findByRole("alert");
  expect(within(alert).getByText("no pool deepseek in config.json")).toBeTruthy();
  expect(screen.getByRole("group", { name: "chatgpt" })).toBeTruthy();
});

test("the code map marks the editor present", () => {
  expect(codeMap).toContain("- BUD-2 editor (`PoolEditor`): present.");
});
