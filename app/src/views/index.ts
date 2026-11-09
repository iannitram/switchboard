import { Sky } from "./Sky";
import { Waiting } from "./Waiting";
import { Reports } from "./Reports";
import { Budget } from "./Budget";
import { Chat } from "./Chat";
import { Settings } from "./Settings";

// The views, in tab order (SHELL-1), Settings (CORE-5) last. Loadout (IT-1) is added by its
// issue.
export const VIEWS = {
  Sky,
  "Waiting on you": Waiting,
  Reports,
  Budget,
  Chat,
  Settings,
} as const;

export type ViewName = keyof typeof VIEWS;
