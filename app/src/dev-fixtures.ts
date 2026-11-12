// DEV only (never active in tests or production builds): VITE_FIXTURE=1 in the git-ignored
// .env.development.local answers the core commands a view asks for with sample data, so the
// populated states can be looked at without a pipeline repo. VITE_VIEW=<view name> picks the
// first view. The call sites are guarded by import.meta.env.DEV, so a build drops them.

const NOW = Math.floor(Date.now() / 1000);
const hoursAgo = (h: number) => NOW - h * 3600;

const WAITING = [
  { what: "Review PR #312: sky label culling", why: "Checks passed; waiting for a human merge decision.", source: "pr", link: "https://example.com/pr/312", action: "Open PR", since: hoursAgo(2) },
  { what: "Approve budget raise for the codex pool", why: "The pool is at 92% with three days left in the window.", source: "budget", link: "https://example.com/budget", action: "Review", since: hoursAgo(9) },
  { what: "Sign in to the gemini CLI", why: "The last two jobs stopped at the login prompt.", source: "loadout", link: "https://example.com/loadout", action: "Sign in", since: hoursAgo(30) },
];

const REPORTS = [
  {
    id: "r-104", title: "Sky: semantic zoom", project: "owner/switchboard", at: hoursAgo(3), outcome: "passed", failure_mode: null,
    changed: { pr: "https://example.com/pr/310", files: 7, lines: 412 },
    checked: ["unit tests", "build", "screenshot of the zoomed sky"], not_checked: ["a real GPU run"],
    cost: { pool: "claude", tokens: 184230, usd: 1.42, seconds: 931 },
    moves: [{ from_service: "gemini", from_model: "flash", to_service: "claude", to_model: "sonnet", why: "the first pass left a failing test" }],
  },
  {
    id: "r-103", title: "Budget: pace line", project: "owner/switchboard", at: hoursAgo(26), outcome: "failed", failure_mode: "tests red",
    changed: { pr: null, files: 3, lines: 58 },
    checked: ["unit tests"], not_checked: ["build", "the pace line at 100%"],
    cost: { pool: "qwen", tokens: 51200, usd: null, seconds: 402 },
    moves: [],
  },
  {
    id: "r-101", title: "Loadout: toolchain rows", project: "owner/core", at: hoursAgo(50), outcome: "passed", failure_mode: null,
    changed: { pr: "https://example.com/pr/301", files: 2, lines: 31 },
    checked: ["unit tests", "build"], not_checked: [],
    cost: { pool: "chatgpt", tokens: 22110, usd: 0.31, seconds: 95 },
    moves: [],
  },
];

const BUDGET = [
  { pool: "chatgpt", pct: 98.6, pace: 81.4, delta: 17.2, verdict: "brake", reading_age_min: 138, stale: true, why: "reading is 138 min old (limit 120)" },
  { pool: "claude", pct: 42, pace: 55.5, delta: -13.5, verdict: "on-pace", reading_age_min: 12, stale: false, why: "claude 42.0% vs pace 55.5%" },
  { pool: "gemini", pct: 71.3, pace: 60, delta: 11.3, verdict: "watch", reading_age_min: 5, stale: false, why: "gemini 71.3% vs pace 60.0%" },
  { pool: "qwen", pct: null, pace: null, delta: null, verdict: "not recorded", reading_age_min: null, stale: false, why: "" },
];

const LOADOUT = {
  read_at: NOW - 130,
  clis: [["claude", true], ["codex", true], ["gemini", false]],
  models_loaded: ["qwen3-coder-30b"],
  toolchains: ["cargo", "node"],
  busy: false,
  recent_errors: ["job 88 exited 1: cargo test", "job 87 did not start"],
};

const SETTINGS = {
  settings: { pipeline_repo: "/srv/pipeline", nextcloud_backups: null, vault_agents: null },
  missing: ["nextcloud_backups", "vault_agents"],
};

const ANSWERS: Record<string, unknown> = {
  waiting: WAITING,
  reports: REPORTS,
  budget_rows: BUDGET,
  loadout: LOADOUT,
  settings_get: SETTINGS,
  settings_missing: ["nextcloud_backups", "vault_agents"],
  fleet_errors: [],
};

/** The sample answer for a command, or undefined to go to the real core. */
export const fixtureAnswer = (cmd: string): { answer: unknown } | undefined => (cmd in ANSWERS ? { answer: ANSWERS[cmd] } : undefined);

export const DEV_CHAT_TURNS = [
  { role: "user" as const, text: "Summarise what is waiting on me and what the budget says." },
  {
    role: "assistant" as const,
    text: "Three items wait on you: a PR to merge, a budget raise to approve, and a CLI sign-in.\n\nThe gemini pool is at 71.3%, ahead of pace by 11 points; the others are on pace.",
    service: "claude", model: "sonnet", step: "rules", handoff: null,
  },
  { role: "user" as const, text: "Sign in to gemini for me, then open the codex budget." },
  {
    role: "assistant" as const,
    text: "I cannot sign in for you. Run the gemini login in a terminal, then press Open PR on the first item to see the checks.",
    service: "gemini", model: "flash", step: "default", handoff: "the task is short",
  },
];
