import { useEffect, useState } from "react";
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

// SHELL-1: one window with the Sky, Waiting on you, Reports, Budget and Chat views. The face
// holds no rules: every view asks the core through `core.ts`.
// FD-12 (#429): the window keeps the request this machine holds for the last Chat message (the
// `chat_route` reply's `held`) and hands it to the Chat view, which offers to send it as a fleet
// job. A later reply with none keeps it (a send of it may still be out, and its result shows
// when filing ends); a new held request replaces it; leaving the Chat view clears it.
export function App() {
  const [view, setView] = useState<ViewName>("Sky");
  const [held, setHeld] = useState<Held | null>(null);
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
      <nav aria-label="Views" style={{ position: "sticky", top: 0, zIndex: 1, background: "Canvas" }}>
        {(Object.keys(VIEWS) as ViewName[]).map((name) => (
          <button key={name} aria-pressed={name === view} onClick={() => show(name)}>
            {name}
          </button>
        ))}
      </nav>
      <ErrorBanner errors={errors} />
      {settingsStatus && <p role="status">{settingsStatus}</p>}
      <main>
        {View === Chat ? <Chat held={held ?? undefined} send={sendChat} /> : <View />}
      </main>
    </div>
  );
}
