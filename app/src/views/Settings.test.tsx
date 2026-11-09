// Locked test for the Settings tab (CORE-5). The builder never edits this file. The core is
// mocked: App.tsx reads `settingsMissing` and `fleetErrors` from ../core when the window opens,
// and the views ask through `call`, so the mock gives all three.
import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, expect, test, vi } from "vitest";
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

beforeEach(() => {
  mockCall.mockReset();
  mockCall.mockResolvedValue({ error: { command: "not in this test", lines: [] } });
});

test("Settings is the last entry of VIEWS", () => {
  const names = Object.keys(VIEWS);
  expect(names[names.length - 1]).toBe("Settings");
  expect(VIEWS.Settings).toBe(Settings);
});

test("the window has a Settings tab that opens the Settings view", async () => {
  render(<App />);
  fireEvent.click(screen.getByRole("button", { name: "Settings" }));
  expect(await screen.findByRole("region", { name: "Settings" })).toBeTruthy();
  expect(screen.getByRole("button", { name: "Settings" }).getAttribute("aria-pressed")).toBe("true");
});
