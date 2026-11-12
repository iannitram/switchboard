// The "Report bug" button: opens a new issue in the system browser, prefilled with facts that
// are safe to publish (app version and the open view). No paths, hostnames or settings values.
import { openUrl } from "@tauri-apps/plugin-opener";
import pkg from "../package.json";

export const ISSUES_URL = "https://github.com/iannitram/switchboard/issues/new";

export function buildBugReportUrl(version: string, view: string): string {
  const params = new URLSearchParams({
    labels: "bug",
    title: `Bug in the ${view} view`,
    body: `App version: ${version}\nView: ${view}\n\nWhat happened:\n\nWhat I expected:\n`,
  });
  return `${ISSUES_URL}?${params.toString()}`;
}

/** Opens the prefilled issue; outside Tauri it falls back to a normal new tab. */
export function reportBug(view: string): void {
  const url = buildBugReportUrl(pkg.version, view);
  if (typeof window !== "undefined" && "__TAURI_INTERNALS__" in window) {
    void openUrl(url).catch(() => undefined);
  } else {
    window.open(url, "_blank", "noopener");
  }
}
