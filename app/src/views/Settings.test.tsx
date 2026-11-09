/// <reference types="vite/client" />
// Locked tests for the Settings view (CORE-5). The builder never edits this file. The core is
// mocked: App.tsx reads `settingsMissing` and `fleetErrors` from ../core when the window opens,
// and the views ask through `call`, which answers settings_get and settings_save here.
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, expect, test, vi } from "vitest";
import codeMap from "../../../docs/code-map.md?raw";
import { call } from "../core";
import { App } from "../App";
import { VIEWS } from "./index";
import { Settings } from "./Settings";

vi.mock("../core", () => ({
  call: vi.fn(),
  settingsMissing: vi.fn(async () => ({ ok: [] })),
  fleetErrors: vi.fn(async () => ({ ok: [] })),
}));
const mockCall = vi.mocked(call);

function deepFreeze<T>(value: T): T {
  if (value && typeof value === "object") {
    Object.values(value).forEach(deepFreeze);
    Object.freeze(value);
  }
  return value;
}

const EMPTY = deepFreeze({
  settings: { pipeline_repo: null, nextcloud_backups: null, vault_agents: null },
  missing: ["pipeline_repo", "nextcloud_backups", "vault_agents"],
});
const PIPELINE_SAVED = deepFreeze({
  settings: { pipeline_repo: "/srv/pipeline", nextcloud_backups: null, vault_agents: null },
  missing: ["nextcloud_backups", "vault_agents"],
});
const LABELS = ["Pipeline repo clone", "NextCloud backups folder", "Vault agents folder"];
const field = (label: string) => screen.getByLabelText(label) as HTMLInputElement;

beforeEach(() => {
  mockCall.mockReset();
  mockCall.mockResolvedValue({ error: { command: "not in this test", lines: [] } });
});

test("while the first answer is pending it says loading settings", () => {
  mockCall.mockReturnValueOnce(new Promise(() => {}));
  render(<Settings />);
  expect(screen.getByText("loading settings")).toBeTruthy();
  expect(mockCall).toHaveBeenCalledWith("settings_get");
});

test("an empty settings file names every missing path and shows empty fields", async () => {
  mockCall.mockResolvedValueOnce({ ok: EMPTY });
  render(<Settings />);
  expect(
    await screen.findByText("Not set: Pipeline repo clone, NextCloud backups folder, Vault agents folder"),
  ).toBeTruthy();
  for (const label of LABELS) expect(field(label).value).toBe("");
  expect(screen.queryByText("loading settings")).toBeNull();
});

test("the fields come in SETTING_NAMES order", async () => {
  mockCall.mockResolvedValueOnce({ ok: EMPTY });
  render(<Settings />);
  await screen.findByLabelText("Pipeline repo clone");
  const inputs = screen.getAllByRole("textbox");
  expect(inputs).toEqual(LABELS.map(field));
});

test("set paths show in their fields and all set says so", async () => {
  mockCall.mockResolvedValueOnce({
    ok: deepFreeze({
      settings: { pipeline_repo: "/srv/pipeline", nextcloud_backups: "/srv/backups", vault_agents: "/srv/agents" },
      missing: [],
    }),
  });
  render(<Settings />);
  expect(await screen.findByText("All paths set")).toBeTruthy();
  expect(field("Pipeline repo clone").value).toBe("/srv/pipeline");
  expect(field("NextCloud backups folder").value).toBe("/srv/backups");
  expect(field("Vault agents folder").value).toBe("/srv/agents");
});

test("Save sends trimmed paths, an empty field as null, and shows the saved answer", async () => {
  mockCall.mockResolvedValueOnce({ ok: EMPTY });
  mockCall.mockResolvedValueOnce({ ok: PIPELINE_SAVED });
  render(<Settings />);
  fireEvent.change(await screen.findByLabelText("Pipeline repo clone"), { target: { value: "  /srv/pipeline  " } });
  fireEvent.change(field("Vault agents folder"), { target: { value: "   " } });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  expect(await screen.findByText("Saved")).toBeTruthy();
  expect(mockCall).toHaveBeenLastCalledWith("settings_save", {
    settings: { pipeline_repo: "/srv/pipeline", nextcloud_backups: null, vault_agents: null },
  });
  expect(screen.getByText("Not set: NextCloud backups folder, Vault agents folder")).toBeTruthy();
  expect(field("Pipeline repo clone").value).toBe("/srv/pipeline");
  expect(field("Vault agents folder").value).toBe("");
});

test("editing a field after a save hides Saved", async () => {
  mockCall.mockResolvedValueOnce({ ok: EMPTY });
  mockCall.mockResolvedValueOnce({ ok: PIPELINE_SAVED });
  render(<Settings />);
  fireEvent.click(await screen.findByRole("button", { name: "Save" }));
  await screen.findByText("Saved");
  fireEvent.change(field("NextCloud backups folder"), { target: { value: "/srv/backups" } });
  expect(screen.queryByText("Saved")).toBeNull();
});

test("a core error on load is shown in place with its command and lines", async () => {
  mockCall.mockResolvedValueOnce({
    error: deepFreeze({ command: "settings_get", lines: ["file: permission denied", "second line"] }),
  });
  render(<Settings />);
  const alert = await screen.findByRole("alert");
  expect(alert.textContent).toContain("settings_get failed");
  expect(alert.textContent).toContain("file: permission denied");
  expect(alert.textContent).toContain("second line");
});

test("a core error on save is shown in place and nothing says Saved", async () => {
  mockCall.mockResolvedValueOnce({ ok: EMPTY });
  mockCall.mockResolvedValueOnce({ error: deepFreeze({ command: "settings_save", lines: ["file: read-only"] }) });
  render(<Settings />);
  fireEvent.click(await screen.findByRole("button", { name: "Save" }));
  const alert = await screen.findByRole("alert");
  expect(alert.textContent).toContain("settings_save failed");
  expect(alert.textContent).toContain("file: read-only");
  expect(screen.queryByText("Saved")).toBeNull();
});

test("Settings is the last entry of VIEWS", () => {
  const names = Object.keys(VIEWS);
  expect(names[names.length - 1]).toBe("Settings");
  expect(VIEWS.Settings).toBe(Settings);
});

test("the window has a Settings tab that opens the view and asks for the settings", async () => {
  mockCall.mockImplementation(async (command: string) =>
    command === "settings_get" ? { ok: EMPTY } : { error: { command, lines: [] } },
  );
  render(<App />);
  fireEvent.click(screen.getByRole("button", { name: "Settings" }));
  expect(await screen.findByRole("region", { name: "Settings" })).toBeTruthy();
  await waitFor(() => expect(mockCall).toHaveBeenCalledWith("settings_get"));
  expect(await screen.findByText("Not set: Pipeline repo clone, NextCloud backups folder, Vault agents folder")).toBeTruthy();
});

test("the code map shows core-settings present", () => {
  const lines = codeMap.split("\n");
  const at = lines.indexOf("### core-settings");
  expect(at).toBeGreaterThanOrEqual(0);
  expect(lines[at + 1]).toContain("· present.");
  expect(lines[at + 1]).toContain("(`app/src/views/Settings.tsx`, present)");
});
