import { Sky } from "./Sky";
import { Waiting } from "./Waiting";
import { Reports } from "./Reports";
import { Budget } from "./Budget";
import { Chat } from "./Chat";
import { Loadout } from "./Loadout";
import { Settings } from "./Settings";

// The views, in tab order (SHELL-1): Loadout (IT-1) sixth, Settings (CORE-5) last.
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
