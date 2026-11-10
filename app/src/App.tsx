import { useEffect, useState } from "react";
import { VIEWS, type ViewName } from "./views";
import { ErrorBanner, fromFleetError } from "./ErrorBanner";
import { fleetErrors, settingsMissing, type CoreError, type CoreResult } from "./core";

// SHELL-1: the status line for the settings the core could not find, or could not read.
function settingsLine(result: CoreResult<string[]>): string | null {
  if ("error" in result) {
    const [first] = result.error.lines;
    return `Settings could not be read: ${first ?? "no output"}`;
  }
  return result.ok.length > 0 ? `Settings needed: ${result.ok.join(", ")}` : null;
}

// DEV only: VITE_VIEW=<view name> in .env.development.local opens that view first. Dead in builds and tests.
function devView(): ViewName {
  const asked = import.meta.env.DEV && import.meta.env.MODE === "development" ? import.meta.env.VITE_VIEW : undefined;
  return asked && asked in VIEWS ? (asked as ViewName) : "Sky";
}

// SHELL-1: one window with the Sky, Waiting on you, Reports, Budget and Chat views. The face
// holds no rules: every view asks the core through `core.ts`.
export function App() {
  const [view, setView] = useState<ViewName>(devView);
  const [settingsStatus, setSettingsStatus] = useState<string | null>(null);
  const [errors, setErrors] = useState<CoreError[]>([]);
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

  // SHELL-3: the fleet error log is read when the window opens and again on every view change.
  useEffect(() => {
    let live = true;
    fleetErrors().then((result) => {
      if (!live) return;
      if ("error" in result) setErrors([result.error]);
      else setErrors(result.ok.map(fromFleetError));
    });
    return () => {
      live = false;
    };
  }, [view]);

  return (
    <div className="app">
      <header className="console">
        <div className="console-bar">
          <span className="console-brand" aria-hidden="true">
            <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round">
              <circle cx="12" cy="12" r="9" opacity="0.55" />
              <circle cx="12" cy="12" r="4.2" />
              <path d="M12 1.5v4M12 18.5v4M1.5 12h4M18.5 12h4" />
            </svg>
            Switchboard
          </span>
          <nav aria-label="Views">
            {(Object.keys(VIEWS) as ViewName[]).map((name) => (
              <button key={name} className="key" aria-pressed={name === view} onClick={() => setView(name)}>
                <span className="key-lamp" aria-hidden="true" />
                <span className="key-label">{name}</span>
              </button>
            ))}
          </nav>
        </div>
        <div className="console-readouts">
          <ErrorBanner errors={errors} />
          {settingsStatus && <p role="status" className="readout">{settingsStatus}</p>}
        </div>
      </header>
      <main>
        <div key={view} data-view={view} className={view === "Sky" ? "pane pane-sky" : view === "Chat" ? "pane pane-chat" : "pane"}>
          <View />
        </div>
      </main>
    </div>
  );
}
