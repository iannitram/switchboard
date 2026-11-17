import { defineConfig, loadEnv } from "vite";
import { fileURLToPath } from "node:url";
import react from "@vitejs/plugin-react";

// The React face. Tauri serves `dist/` (tauri.conf.json `frontendDist`); `npm run dev` serves
// on 5173 for `tauri dev`. Face tests run in jsdom with vitest.
export default defineConfig(({ mode }) => {
  // DEV only: VITE_FIXTURE=1 or VITE_SCENE in .env.development.local or ?scene= in URL.
  // Never active in tests or builds.
  const env = loadEnv(mode, process.cwd(), "VITE_");
  const fixture = mode === "development" && (!!env.VITE_FIXTURE || !!env.VITE_SCENE);
  return {
    plugins: [
      react(),
      {
        name: "scene-dev",
        apply: "serve",
        transformIndexHtml() {
          return [
            {
              tag: "script",
              attrs: { type: "module" },
              children: `import { seedScene, scenes } from "/src/scenes.ts";
const params = new URLSearchParams(window.location.search);
const name = params.get("scene") || (import.meta.env.VITE_SCENE) || (import.meta.env.VITE_FIXTURE ? "populated" : undefined);
if (name && name in scenes) {
  seedScene(name);
}
window.__seedScene = seedScene;
window.__scenes = scenes;
`,
              injectTo: "head-prepend",
            },
          ];
        },
      },
    ],
    clearScreen: false,
    server: { port: 5173, strictPort: true },
    resolve: fixture
      ? { alias: [{ find: /^@tauri-apps\/api\/core$/, replacement: fileURLToPath(new URL("./src/dev-core.ts", import.meta.url)) }] }
      : undefined,
    test: { environment: "jsdom", globals: true },
  };
});
