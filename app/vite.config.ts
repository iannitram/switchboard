import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

// The React face. Tauri serves `dist/` (tauri.conf.json `frontendDist`); `npm run dev` serves
// on 5173 for `tauri dev`. Face tests run in jsdom with vitest.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { port: 5173, strictPort: true },
  test: { environment: "jsdom", globals: true },
});
