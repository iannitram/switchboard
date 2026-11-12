// DEV only: stands in for "@tauri-apps/api/core" when VITE_FIXTURE is set (see vite.config.ts).
export * from "../node_modules/@tauri-apps/api/core.js";
import { invoke as realInvoke } from "../node_modules/@tauri-apps/api/core.js";
import { fixtureAnswer } from "./dev-fixtures";

export async function invoke<T>(cmd: string, args?: Record<string, unknown>, options?: unknown): Promise<T> {
  const hit = fixtureAnswer(cmd);
  if (hit) return hit.answer as T;
  return realInvoke<T>(cmd, args, options as never);
}
