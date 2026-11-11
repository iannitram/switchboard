// BAK-1 (#99): the Sky's back up buttons call `backup_repo` and `backup_all` (#206) and show
// each outcome, or the error in place (SHELL-3).
import { fireEvent, render, screen, within } from "@testing-library/react";
import type { Constellation, SkyModel } from "../sky/model";
import { Sky, backupLine, type BackupRow, type Manifest } from "./Sky";

const { call } = vi.hoisted(() => ({ call: vi.fn() }));
vi.mock("../core", () => ({ call }));

function constellation(repo: string, rank: number): Constellation {
  return {
    repo,
    url: `https://github.com/${repo}`,
    rank,
    radius: 0,
    size: 1,
    angle: 0,
    brightness: 0.5,
    features: [],
    edges: [],
    chain: [],
    drift: [],
    reuses: [],
  };
}

const SKY: SkyModel = {
  ageMinutes: 1,
  priorityAgeDays: 0,
  constellations: [constellation("owner/one", 0), constellation("owner/two", 1)],
};

function manifest(repo: string): Manifest {
  return {
    repo,
    taken_at: "2026-11-09T16:45:00Z",
    refs: 3,
    issues: 12,
    open_issues: 4,
    pulls: 5,
    open_pulls: 1,
    merged_pulls: 4,
    comments: 30,
    labels: 7,
    label_assignments: 20,
    milestones: 2,
    releases: 0,
    closed_without_pr: 1,
    wiki: true,
  };
}

const WRITTEN: BackupRow = {
  repo: "owner/one",
  outcome: "written",
  folder: "/backups/github/owner/one",
  manifest: manifest("owner/one"),
  reason: null,
};
const REFUSED: BackupRow = {
  repo: "owner/two",
  outcome: "refused",
  folder: "/backups/github/owner/two",
  manifest: null,
  reason: "the new backup has 12 open issues, the last had 40",
};

const FAILED: BackupRow = {
  repo: "owner/gone",
  outcome: "failed",
  folder: "/backups/github/owner/gone",
  manifest: null,
  reason: "owner/gone: git clone --mirror: not found",
};

function answer(byCommand: Record<string, unknown>) {
  call.mockImplementation(async (command: string) =>
    command in byCommand ? { ok: byCommand[command] } : { error: { command, lines: ["not mocked"] } },
  );
}

afterEach(() => call.mockReset());

test("back up all calls backup_all and shows one line per repo", async () => {
  answer({ sky: SKY, backup_all: [WRITTEN, REFUSED] });
  render(<Sky />);
  fireEvent.click(await screen.findByRole("button", { name: "Back up all" }));
  const outcome = await screen.findByRole("status", { name: "Backup outcome" });
  expect(call).toHaveBeenCalledWith("backup_all");
  const lines = within(outcome).getAllByRole("listitem").map((li) => li.textContent);
  expect(lines).toEqual([backupLine(WRITTEN), backupLine(REFUSED)]);
  expect(lines[0]).toContain("owner/one backed up at");
  expect(lines[0]).toContain("3 refs, 12 issues, 5 PRs, 30 comments, 7 labels, 2 milestones, the wiki");
  expect(lines[1]).toBe(
    "owner/two not backed up, the last backup was kept: the new backup has 12 open issues, the last had 40",
  );
});

// #369: a repo whose backup failed has a line of its own, beside the other repos' lines.
test("a failed repo in back up all is shown as failed, with its error", async () => {
  answer({ sky: SKY, backup_all: [WRITTEN, FAILED] });
  render(<Sky />);
  fireEvent.click(await screen.findByRole("button", { name: "Back up all" }));
  const outcome = await screen.findByRole("status", { name: "Backup outcome" });
  const lines = within(outcome).getAllByRole("listitem").map((li) => li.textContent);
  expect(lines[0]).toContain("owner/one backed up at");
  expect(lines[1]).toBe(
    "owner/gone backup failed, the last backup (if there was one) was kept: owner/gone: git clone --mirror: not found",
  );
});

test("back up on a constellation calls backup_repo with that repo", async () => {
  answer({ sky: SKY, backup_repo: WRITTEN });
  render(<Sky />);
  const stars = await screen.findAllByTestId("constellation");
  fireEvent.click(stars[0]);
  fireEvent.click(screen.getByRole("button", { name: "Back up this repo" }));
  const outcome = await screen.findByRole("status", { name: "Backup outcome" });
  expect(call).toHaveBeenCalledWith("backup_repo", { repo: "owner/one" });
  expect(outcome.textContent).toBe(backupLine(WRITTEN));
});

test("a failed backup shows the error in place", async () => {
  call.mockImplementation(async (command: string) =>
    command === "sky"
      ? { ok: SKY }
      : { error: { command, lines: ["setting not set: nextcloud_backups"] } },
  );
  render(<Sky />);
  fireEvent.click(await screen.findByRole("button", { name: "Back up all" }));
  const alert = await screen.findByRole("alert", { name: "Backup outcome" });
  expect(alert.textContent).toBe("backup_all failed: setting not set: nextcloud_backups");
});

test("the time a backup was taken reads as 12-hour local time", () => {
  const line = backupLine(WRITTEN);
  expect(line).toMatch(/at \d{4}-\d{2}-\d{2} \d{1,2}(:\d{2})?(am|pm):/);
});

test("a backup that followed a rename says which repo GitHub answered for", () => {
  const row: BackupRow = { ...WRITTEN, manifest: { ...manifest("owner/one"), resolved_repo: "new-owner/renamed" } };
  expect(backupLine(row)).toMatch(/^owner\/one \(GitHub answered for new-owner\/renamed: renamed or moved\) backed up at /);
  expect(backupLine(WRITTEN)).not.toContain("GitHub answered");
});
