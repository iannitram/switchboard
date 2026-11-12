import { isExternalLink } from "./external-links";

const APP = "http://localhost:5173/";

test("another site over http or https is external", () => {
  expect(isExternalLink("https://github.com/owner/repo/pull/1", APP)).toBe(true);
  expect(isExternalLink("http://example.com/", APP)).toBe(true);
});

test("the app's own origin, relative links and anchors are not", () => {
  expect(isExternalLink("http://localhost:5173/other", APP)).toBe(false);
  expect(isExternalLink("/path", APP)).toBe(false);
  expect(isExternalLink("#top", APP)).toBe(false);
});

test("a different port on the same host is another origin", () => {
  expect(isExternalLink("http://localhost:3000/", APP)).toBe(true);
});

test("other schemes, empty and broken links are left alone", () => {
  expect(isExternalLink("mailto:a@example.com", APP)).toBe(false);
  expect(isExternalLink("brocrew://agent/x", APP)).toBe(false);
  expect(isExternalLink("javascript:void(0)", APP)).toBe(false);
  expect(isExternalLink("", APP)).toBe(false);
  expect(isExternalLink(null, APP)).toBe(false);
  expect(isExternalLink("http://[bad", APP)).toBe(false);
});
