import { useState } from "react";
import { VIEWS, type ViewName } from "./views";

// SHELL-1: one window with the Sky, Waiting on you, Reports, Budget and Chat views. The face
// holds no rules: every view asks the core through `core.ts`.
export function App() {
  const [view, setView] = useState<ViewName>("Sky");
  const View = VIEWS[view];
  return (
    <div className="app">
      <nav aria-label="Views">
        {(Object.keys(VIEWS) as ViewName[]).map((name) => (
          <button key={name} aria-pressed={name === view} onClick={() => setView(name)}>
            {name}
          </button>
        ))}
      </nav>
      <main>
        <View />
      </main>
    </div>
  );
}
