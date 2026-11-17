/// <reference types="vite/client" />
import { render, screen } from "@testing-library/react";
import { expect, test } from "vitest";
import { VIEWS } from "./views";
import { call } from "./core";
import { seedScene, scenes } from "./scenes";

test("seedScene renders budget-braked view and finds brake notice", async () => {
  seedScene("budget-braked");
  const View = VIEWS[scenes["budget-braked"].view];
  render(<View />);
  expect(await screen.findByText("brake")).toBeTruthy();
  expect(screen.getByText("chatgpt")).toBeTruthy();
});

test("seedScene renders sky-with-drift view and finds drift notice", async () => {
  seedScene("sky-with-drift");
  const View = VIEWS[scenes["sky-with-drift"].view];
  render(<View />);
  expect(await screen.findByText("Drift (1)")).toBeTruthy();
  expect(screen.getByText(/#99 not on roadmap/)).toBeTruthy();
});

test("seedScene renders waiting-empty view and finds empty text", async () => {
  seedScene("waiting-empty");
  const View = VIEWS[scenes["waiting-empty"].view];
  render(<View />);
  expect(await screen.findByText("Nothing is waiting on you.")).toBeTruthy();
});

test("seedScene renders populated view and finds populated item", async () => {
  seedScene("populated");
  const View = VIEWS[scenes.populated.view];
  render(<View />);
  expect(await screen.findByText("Review PR #312: sky label culling")).toBeTruthy();
});

test("a command a scene does not answer rejects naming scene and command", async () => {
  seedScene("waiting-empty");
  const res = await call("unknown_command");
  expect("error" in res).toBe(true);
  if ("error" in res) {
    expect(res.error.command).toBe("unknown_command");
    expect(res.error.lines.join(" ")).toContain("Scene 'waiting-empty' does not answer command 'unknown_command'");
  }
});

test("seedScene with an unknown scene throws", () => {
  expect(() => seedScene("unknown-scene")).toThrow("Unknown scene: unknown-scene");
});
