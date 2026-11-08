/// <reference types="vite/client" />
// Locked tests for issue 8 (CORE-5: the Settings view). Qwen never edits this file. The core
// is mocked: `call` from ../core answers settings_get and settings_save.
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import codeMap from "../../../docs/code-map.md?raw";
import { call } from "../core";
import { App } from "../App";
import { Settings } from "./Settings";

vi.mock("../core", () => ({ call: vi.fn() }));
const mockCall = vi.mocked(call);

const EMPTY = Object.freeze({
  settings: Object.freeze({ pipeline_repo: null, nextcloud_backups: null, vault_agents: null }),
  missing: Object.freeze(["pipeline_repo", "nextcloud_backups", "vault_agents"]),
});

beforeEach(() => {
  mockCall.mockReset();
});

test("an empty settings file names every missing path and shows empty fields", async () => {
  mockCall.mockResolvedValueOnce({ ok: EMPTY });
  render(<Settings />);
  expect(
    await screen.findByText("Not set: Pipeline repo clone, NextCloud backups folder, Vault agents folder"),
  ).toBeTruthy();
  expect(mockCall).toHaveBeenCalledWith("settings_get");
  for (const label of ["Pipeline repo clone", "NextCloud backups folder", "Vault agents folder"]) {
    expect((screen.getByLabelText(label) as HTMLInputElement).value).toBe("");
  }
});

test("set paths show in their fields and all set says so", async () => {
  mockCall.mockResolvedValueOnce({
    ok: {
      settings: { pipeline_repo: "/srv/pipeline", nextcloud_backups: "/srv/backups", vault_agents: "/srv/agents" },
      missing: [],
    },
  });
  render(<Settings />);
  expect(await screen.findByText("All paths set")).toBeTruthy();
  expect((screen.getByLabelText("Pipeline repo clone") as HTMLInputElement).value).toBe("/srv/pipeline");
  expect((screen.getByLabelText("NextCloud backups folder") as HTMLInputElement).value).toBe("/srv/backups");
  expect((screen.getByLabelText("Vault agents folder") as HTMLInputElement).value).toBe("/srv/agents");
});

test("Save sends trimmed paths, an empty field as null, and shows the saved answer", async () => {
  mockCall.mockResolvedValueOnce({ ok: EMPTY });
  mockCall.mockResolvedValueOnce({
    ok: {
      settings: { pipeline_repo: "/srv/pipeline", nextcloud_backups: null, vault_agents: null },
      missing: ["nextcloud_backups", "vault_agents"],
    },
  });
  render(<Settings />);
  const field = (await screen.findByLabelText("Pipeline repo clone")) as HTMLInputElement;
  fireEvent.change(field, { target: { value: "  /srv/pipeline  " } });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  expect(await screen.findByText("Saved")).toBeTruthy();
  expect(mockCall).toHaveBeenLastCalledWith("settings_save", {
    settings: { pipeline_repo: "/srv/pipeline", nextcloud_backups: null, vault_agents: null },
  });
  expect(screen.getByText("Not set: NextCloud backups folder, Vault agents folder")).toBeTruthy();
  expect((screen.getByLabelText("Pipeline repo clone") as HTMLInputElement).value).toBe("/srv/pipeline");
});

test("a core error on load is shown in place with its command and lines", async () => {
  mockCall.mockResolvedValueOnce({
    error: { command: "settings_get", lines: ["file: permission denied", "second line"] },
  });
  render(<Settings />);
  const alert = await screen.findByRole("alert");
  expect(alert.textContent).toContain("settings_get failed");
  expect(alert.textContent).toContain("file: permission denied");
  expect(alert.textContent).toContain("second line");
});

test("a core error on save is shown in place and nothing says Saved", async () => {
  mockCall.mockResolvedValueOnce({ ok: EMPTY });
  mockCall.mockResolvedValueOnce({ error: { command: "settings_save", lines: ["file: read-only"] } });
  render(<Settings />);
  fireEvent.click(await screen.findByRole("button", { name: "Save" }));
  const alert = await screen.findByRole("alert");
  expect(alert.textContent).toContain("settings_save failed");
  expect(alert.textContent).toContain("file: read-only");
  expect(screen.queryByText("Saved")).toBeNull();
});

test("the window has a Settings tab that opens the view", async () => {
  mockCall.mockResolvedValue({ ok: EMPTY });
  render(<App />);
  fireEvent.click(screen.getByRole("button", { name: "Settings" }));
  await waitFor(() => expect(screen.getByRole("region", { name: "Settings" })).toBeTruthy());
  expect(mockCall).toHaveBeenCalledWith("settings_get");
});

test("the code map shows core-settings present", () => {
  const lines = codeMap.split("\n");
  const at = lines.indexOf("### core-settings");
  expect(at).toBeGreaterThanOrEqual(0);
  expect(lines[at + 1]).toContain("· present.");
});
