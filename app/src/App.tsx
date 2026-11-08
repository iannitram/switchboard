import { useEffect, useState } from "react";
import { VIEWS, type ViewName } from "./views";
import { settingsMissing, type CoreResult } from "./core";

// SHELL-1: the status line for the settings the core could not find, or could not read.
function settingsLine(result: CoreResult<string[]>): string | null {
  if ("error" in result) {
    const [first] = result.error.lines;
    return `Settings could not be read: ${first ?? "no output"}`;
  }
  return result.ok.length > 0 ? `Settings needed: ${result.ok.join(", ")}` : null;
}

// SHELL-1: one window with the Sky, Waiting on you, Reports, Budget and Chat views. The face
// holds no rules: every view asks the core through `core.ts`.
export function App() {
  const [view, setView] = useState<ViewName>("Sky");
  const [settingsStatus, setSettingsStatus] = useState<string | null>(null);
  const View = VIEWS[view];

  useEffect(() => {
    let live = true;
    settingsMissing().then((result) => {
      if (live) setSettingsStatus(settingsLine(result));
    });
    return () => {
      live = false;
    };
  }, []);

  return (
    <div className="app">
      <nav aria-label="Views">
        {(Object.keys(VIEWS) as ViewName[]).map((name) => (
          <button key={name} aria-pressed={name === view} onClick={() => setView(name)}>
            {name}
          </button>
        ))}
      </nav>
      {settingsStatus && <p role="status">{settingsStatus}</p>}
      <main>
        <View />
      </main>
    </div>
  );
}
