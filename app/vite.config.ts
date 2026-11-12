import { defineConfig, loadEnv } from "vite";
import { fileURLToPath } from "node:url";
import react from "@vitejs/plugin-react";

// The React face. Tauri serves `dist/` (tauri.conf.json `frontendDist`); `npm run dev` serves
// on 5173 for `tauri dev`. Face tests run in jsdom with vitest.
export default defineConfig(({ mode }) => {
  // DEV only: VITE_FIXTURE=1 in .env.development.local swaps the `invoke` the views use for sample
  // answers (src/dev-fixtures.ts). Never active in tests or builds.
  const fixture = mode === "development" && !!loadEnv(mode, process.cwd(), "VITE_").VITE_FIXTURE;
  return {
    plugins: [react()],
    clearScreen: false,
    server: { port: 5173, strictPort: true },
    resolve: fixture
      ? { alias: [{ find: /^@tauri-apps\/api\/core$/, replacement: fileURLToPath(new URL("./src/dev-core.ts", import.meta.url)) }] }
      : undefined,
    test: { environment: "jsdom", globals: true },
  };
});
