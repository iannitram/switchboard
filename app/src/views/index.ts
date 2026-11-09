import { Sky } from "./Sky";
import { Waiting } from "./Waiting";
import { Reports } from "./Reports";
import { Budget } from "./Budget";
import { Chat } from "./Chat";
import { Settings } from "./Settings";
import { Loadout } from "./Loadout";

// The views, in tab order (SHELL-1), Settings (CORE-5) last. Loadout (IT-1) sits before Settings.
export const VIEWS = {
  Sky,
  "Waiting on you": Waiting,
  Reports,
  Budget,
  Chat,
  Loadout,
  Settings,
} as const;

export type ViewName = keyof typeof VIEWS;
