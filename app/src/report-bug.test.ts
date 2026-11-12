import { buildBugReportUrl } from "./report-bug";

test("the URL targets the issue form with the bug label", () => {
  const url = new URL(buildBugReportUrl("1.2.3", "Budget"));
  expect(url.origin + url.pathname).toBe("https://github.com/iannitram/switchboard/issues/new");
  expect(url.searchParams.get("labels")).toBe("bug");
});

test("title and body carry only the version and the view", () => {
  const url = new URL(buildBugReportUrl("1.2.3", "Waiting on you"));
  expect(url.searchParams.get("title")).toBe("Bug in the Waiting on you view");
  const body = url.searchParams.get("body") ?? "";
  expect(body).toContain("App version: 1.2.3");
  expect(body).toContain("View: Waiting on you");
  expect(body).not.toMatch(/\/home|\/Users|[A-Z]:\\/);
});
