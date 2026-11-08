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

// CORE-2 as the `fleet_errors` command sends it (commands.rs `FleetErrorRow`), newest first.
export type FleetErrorRow = { command: string[]; exit_code: number | null; stderr: string };

// CORE-5: the names of the settings not set (`settings_missing`).
export const settingsMissing = () => call<string[]>("settings_missing");

// CORE-2: the fleet errors the core recorded this session (`fleet_errors`).
export const fleetErrors = () => call<FleetErrorRow[]>("fleet_errors");
