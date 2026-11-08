// The face's only door into the core: Tauri commands (app/src-tauri/src/commands.rs). A core
// error comes back as { error: { command, lines } } and is shown in place (SHELL-3).
import { invoke } from "@tauri-apps/api/core";

export type CoreError = { command: string; lines: string[] };
export type CoreResult<T> = { ok: T } | { error: CoreError };

export async function call<T>(command: string, args?: Record<string, unknown>): Promise<CoreResult<T>> {
  try {
    return { ok: await invoke<T>(command, args) };
  } catch (e) {
    const err = e as Partial<CoreError> | string;
    if (typeof err === "string") return { error: { command, lines: err.split("\n").slice(0, 5) } };
    return { error: { command: err.command ?? command, lines: err.lines ?? [] } };
  }
}
