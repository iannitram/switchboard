// Upstream: inspired by storytold/craft-anything at 2e7f276 (MIT OR Apache-2.0)
// - The scenes and the headless flag: crates/ui-egui/src/demo.rs
// - About 40 scenes rendered to PNG: crates/ui-egui/examples/snapshot.rs

import { mockIPC } from "@tauri-apps/api/mocks";
import type { ViewName } from "./views";
import type { SkyModel } from "./sky/model";
import type { PoolRow } from "./views/Budget";
import type { WaitingItemData } from "./views/Waiting";

export type Scene = {
  view: ViewName;
  answers: Record<string, unknown>;
};

const NOW = Math.floor(Date.now() / 1000);
const hoursAgo = (h: number) => NOW - h * 3600;

const WAITING: WaitingItemData[] = [
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

const BUDGET_BRAKED: PoolRow[] = [
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

const SKY_WITH_DRIFT: SkyModel = {
  ageMinutes: 12,
  priorityAgeDays: null,
  constellations: [
    {
      repo: "owner/switchboard",
      url: "https://github.com/owner/switchboard",
      rank: 0,
      radius: 0.5,
      size: 0.8,
      angle: 0,
      brightness: 0.6,
      features: [
        {
          id: "sky",
          title: "Sky view",
          release: "1.0",
          stars: [
            {
              number: 492,
              title: "Named fixture states",
              state: "open",
              url: "https://github.com/owner/switchboard/issues/492",
              labels: ["size:medium"],
              size: "medium",
              milestone: null,
              inFlight: false,
              pr: null,
              job: null,
            },
          ],
        },
      ],
      stages: [],
      edges: [],
      chain: [],
      drift: [
        {
          kind: "orphan",
          text: "#99 not on roadmap",
          link: "https://github.com/owner/switchboard/issues/99",
        },
      ],
      reuses: [],
    },
  ],
};

export const budgetBraked: Scene = {
  view: "Budget",
  answers: {
    settings_missing: [],
    fleet_errors: [],
    budget_rows: BUDGET_BRAKED,
  },
};

export const skyWithDrift: Scene = {
  view: "Sky",
  answers: {
    settings_missing: [],
    fleet_errors: [],
    sky: SKY_WITH_DRIFT,
  },
};

export const waitingEmpty: Scene = {
  view: "Waiting on you",
  answers: {
    settings_missing: [],
    fleet_errors: [],
    waiting: [],
  },
};

export const populated: Scene = {
  view: "Waiting on you",
  answers: {
    settings_missing: ["nextcloud_backups", "vault_agents"],
    fleet_errors: [],
    waiting: WAITING,
    reports: REPORTS,
    budget_rows: BUDGET_BRAKED,
    loadout: LOADOUT,
    settings_get: SETTINGS,
    sky: SKY_WITH_DRIFT,
  },
};

export {
  budgetBraked as "budget-braked",
  skyWithDrift as "sky-with-drift",
  waitingEmpty as "waiting-empty",
};

export const scenes: Record<string, Scene> = {
  "budget-braked": budgetBraked,
  "sky-with-drift": skyWithDrift,
  "waiting-empty": waitingEmpty,
  populated,
};

export type SceneName = keyof typeof scenes;

export function seedScene(name: string): Scene {
  const scene = scenes[name];
  if (!scene) {
    throw new Error(`Unknown scene: ${name}`);
  }
  if (typeof window !== "undefined") {
    (window as unknown as { __ACTIVE_SCENE__?: Scene }).__ACTIVE_SCENE__ = scene;
  }
  mockIPC((cmd, _payload) => {
    if (cmd in scene.answers) {
      return Promise.resolve(scene.answers[cmd]);
    }
    const err = new Error(`Scene '${name}' does not answer command '${cmd}'`);
    (err as unknown as { command: string; lines: string[] }).command = cmd;
    (err as unknown as { command: string; lines: string[] }).lines = [err.message];
    return Promise.reject(err);
  });
  return scene;
}

export const fixtureAnswer = (cmd: string): { answer: unknown } | undefined =>
  cmd in scenes.populated.answers ? { answer: scenes.populated.answers[cmd] } : undefined;

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
