import { render, screen } from "@testing-library/react";
import { App } from "./App";

// SHELL-1 (skeleton): the five views are tabs and each empty view says "no data yet".
test("the window lists the five views", () => {
  render(<App />);
  for (const name of ["Sky", "Waiting on you", "Reports", "Budget", "Chat"]) {
    expect(screen.getByRole("button", { name })).toBeTruthy();
  }
  expect(screen.getByText("no data yet")).toBeTruthy();
});
