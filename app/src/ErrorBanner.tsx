// SHELL-3 (#13): the newest core or fleet error, shown in place above the view. The list is
// newest first; the banner shows the first one, and the rest behind a button.
import { useState } from "react";
import { type CoreError, type FleetErrorRow } from "./core";

export const BANNER_LINES = 5;

export function fromFleetError(row: FleetErrorRow): CoreError {
  const exitLine = row.exit_code === null ? "did not start" : `exit code ${row.exit_code}`;
  const stderr = row.stderr.replace(/\n+$/, "");
  return {
    command: row.command.join(" "),
    lines: [exitLine, ...(stderr === "" ? [] : stderr.split("\n"))],
  };
}

export function ErrorBanner(props: { errors: readonly CoreError[] }): JSX.Element | null {
  const { errors } = props;
  const [open, setOpen] = useState(false);
  if (errors.length === 0) return null;
  const [newest] = errors;
  return (
    <div role="alert" aria-label="Errors" className="error-banner">
      <strong>{newest.command}</strong>
      <pre>{newest.lines.length === 0 ? "(no output)" : newest.lines.slice(0, BANNER_LINES).join("\n")}</pre>
      {errors.length > 1 && (
        <>
          <button onClick={() => setOpen(!open)}>
            {open ? "Hide the list" : `Show all ${errors.length} errors`}
          </button>
          {open && (
            <ul aria-label="All errors">
              {errors.map((error, i) => (
                <li key={i}>{error.command}: {error.lines[0] ?? "(no output)"}</li>
              ))}
            </ul>
          )}
        </>
      )}
    </div>
  );
}
