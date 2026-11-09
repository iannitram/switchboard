/// <reference types="vite/client" />
// REP-2 (#47): the work tree on a report. Locked tests (R28): the builder never edits this file.
import { render, screen, within } from "@testing-library/react";
import codeMap from "../../../docs/code-map.md?raw";
import { ReportList, type Move, type Report } from "./Reports";
import { WorkTree } from "./WorkTree";

function deepFreeze<T>(v: T): T {
  if (v && typeof v === "object") {
    Object.values(v as object).forEach(deepFreeze);
    Object.freeze(v);
  }
  return v;
}

const MOVES: Move[] = deepFreeze([
  { from_service: "qwen", from_model: "qwen3-coder", to_service: "gemini", to_model: "gemini-flash", why: "down" },
  { from_service: "gemini", from_model: "gemini-flash", to_service: "gemini", to_model: "gemini-pro", why: "failed (tests)" },
  { from_service: "gemini", from_model: "gemini-pro", to_service: "claudeapi", to_model: "sonnet", why: "not recorded" },
]);

const report = (id: string, moves: Move[]): Report => ({
  id,
  title: null,
  project: null,
  at: null,
  outcome: "passed",
  failure_mode: null,
  changed: { pr: null, files: null, lines: null },
  checked: [],
  not_checked: [],
  cost: { pool: null, tokens: null, usd: null, seconds: null },
  moves,
});

const REPORTS: Report[] = deepFreeze([report("thread-3", MOVES), report("thread-0", [])]);

test("one line per move, oldest first: from, to and why", () => {
  render(<WorkTree moves={MOVES} />);
  const tree = screen.getByRole("list", { name: "Work tree" });
  expect(tree.tagName).toBe("OL");
  expect(tree.getAttribute("data-part")).toBe("work-tree");
  expect(within(tree).getAllByRole("listitem").map((li) => li.textContent)).toEqual([
    "qwen/qwen3-coder → gemini/gemini-flash: down",
    "gemini/gemini-flash → gemini/gemini-pro: failed (tests)",
    "gemini/gemini-pro → claudeapi/sonnet: not recorded",
  ]);
});

test("no moves says so", () => {
  render(<WorkTree moves={[]} />);
  expect(screen.queryByRole("list", { name: "Work tree" })).toBeNull();
  const none = screen.getByText("no moves");
  expect(none.getAttribute("data-part")).toBe("work-tree");
});

test("every report in the list ends with its work tree", () => {
  render(<ReportList reports={REPORTS} />);
  const three = screen.getByRole("group", { name: "thread-3" });
  expect(three.lastElementChild?.getAttribute("aria-label")).toBe("Work tree");
  expect(within(three).getAllByRole("listitem").slice(-3).map((li) => li.textContent)).toEqual([
    "qwen/qwen3-coder → gemini/gemini-flash: down",
    "gemini/gemini-flash → gemini/gemini-pro: failed (tests)",
    "gemini/gemini-pro → claudeapi/sonnet: not recorded",
  ]);
  const zero = screen.getByRole("group", { name: "thread-0" });
  expect(zero.lastElementChild?.textContent).toBe("no moves");
});

test("the code map marks the work tree present", () => {
  expect(codeMap).toContain("- REP-2 work tree (`WorkTree`): present.");
});
