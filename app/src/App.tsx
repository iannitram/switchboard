import { useEffect, useState } from "react";
import { openExternalLinksInBrowser } from "./external-links";
import { reportBug } from "./report-bug";
import { VIEWS, type ViewName } from "./views";
import { ErrorBanner, fromFleetError } from "./ErrorBanner";
import { call, fleetErrors, settingsMissing, type CoreError, type CoreResult } from "./core";
import { Chat, type ChatRoute, type ChatSend, type Held } from "./views/Chat";

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

// Line glyphs for the nav tabs (16x16 grid), drawn as inline SVG so no asset or CSP change is needed.
const TAB_ICONS: Record<ViewName, string> = {
  Sky: "M8 1l1.6 5.4L15 8l-5.4 1.6L8 15l-1.6-5.4L1 8l5.4-1.6z",
  "Waiting on you": "M8 2a4 4 0 0 0-4 4v3l-1.5 2.5h11L12 9V6a4 4 0 0 0-4-4zM6.5 13.5a1.5 1.5 0 0 0 3 0",
  Reports: "M3 2h8l2 2v10H3zM5.5 6h5M5.5 8.5h5M5.5 11h3",
  Budget: "M8 2a6 6 0 1 0 0 12A6 6 0 0 0 8 2zM8 5v6M6 8h4",
  Chat: "M2 3h12v8H7l-3 3v-3H2z",
  Loadout: "M2 5l6-3 6 3v6l-6 3-6-3zM2 5l6 3 6-3M8 8v6",
  Settings: "M8 4a4 4 0 1 0 0 8 4 4 0 0 0 0-8zM8 1v14M1 8h14M3 3l10 10M13 3L3 13",
};

// SHELL-1: one window with the Sky, Waiting on you, Reports, Budget and Chat views. The face
// holds no rules: every view asks the core through `core.ts`.
// FD-12 (#429): the window keeps the request this machine holds for the last Chat message (the
// `chat_route` reply's `held`) and hands it to the Chat view, which offers to send it as a fleet
// job. A later reply with none keeps it (a send of it may still be out, and its result shows
// when filing ends); a new held request replaces it; leaving the Chat view clears it.
export function App() {
  const [view, setView] = useState<ViewName>(devView);
  const [held, setHeld] = useState<Held | null>(null);
  const [settingsStatus, setSettingsStatus] = useState<string | null>(null);
  const [errors, setErrors] = useState<CoreError[]>([]);
  const View = VIEWS[view];

  // Links to other sites open in the system browser, not in this window.
  useEffect(() => openExternalLinksInBrowser(), []);

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

  // FD-1 + FD-12: the Chat view's send: `chat_route`, keeping the reply's held request.
  const sendChat: ChatSend = async (message, handoffId) => {
    const result = await call<ChatRoute>("chat_route", handoffId === undefined ? { message } : { message, handoffId });
    if ("ok" in result && result.ok.held) setHeld(result.ok.held);
    return result;
  };
  const show = (name: ViewName) => {
    if (name !== view) setHeld(null);
    setView(name);
  };

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
              <button key={name} className="key" aria-pressed={name === view} onClick={() => show(name)}>
                <svg className="key-icon" viewBox="0 0 16 16" aria-hidden="true">
                  <path d={TAB_ICONS[name]} />
                </svg>
                <span className="key-label">{name}</span>
              </button>
            ))}
          </nav>
          <button type="button" className="btn console-report" onClick={() => reportBug(view)}>
            <svg className="btn-icon" viewBox="0 0 16 16" aria-hidden="true">
              <path d="M5.5 5.5a2.5 2.5 0 0 1 5 0v4a2.5 2.5 0 0 1-5 0zM5.5 7H2.5M13.5 7h-3M5.5 10H3M13 10h-2.5M6 3L4.5 1.5M10 3l1.5-1.5M8 6v6" />
            </svg>
            Report bug
          </button>
        </div>
        <div className="console-readouts">
          <ErrorBanner errors={errors} />
          {settingsStatus && <p role="status" className="readout">{settingsStatus}</p>}
        </div>
      </header>
      <main>
        <div key={view} data-view={view} className={view === "Sky" ? "pane pane-sky" : view === "Chat" ? "pane pane-chat" : "pane"}>
          {View === Chat ? <Chat held={held ?? undefined} send={sendChat} /> : <View />}
        </div>
      </main>
    </div>
  );
}
