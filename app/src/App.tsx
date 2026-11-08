import { useEffect, useState } from "react";
import { VIEWS, type ViewName } from "./views";
import { settingsMissing, type CoreResult } from "./core";

// The status line for the settings asked for when the window opens (CORE-5), or none.
function settingsLineFor(result: CoreResult<string[]>): string | null {
  if ("error" in result) {
    const { lines } = result.error;
    return `Settings could not be read: ${lines.length > 0 ? lines[0] : "no output"}`;
  }
  return result.ok.length > 0 ? `Settings needed: ${result.ok.join(", ")}` : null;
}

// SHELL-1: one window with the Sky, Waiting on you, Reports, Budget and Chat views. The face
// holds no rules: every view asks the core through `core.ts`.
export function App() {
  const [view, setView] = useState<ViewName>("Sky");
  const [settingsLine, setSettingsLine] = useState<string | null>(null);
  const View = VIEWS[view];

  useEffect(() => {
    let live = true;
    settingsMissing().then((result) => {
      if (live) setSettingsLine(settingsLineFor(result));
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
      {settingsLine && <p role="status">{settingsLine}</p>}
      <main>
        <View />
      </main>
    </div>
  );
}
