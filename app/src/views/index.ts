import { Sky } from "./Sky";
import { Waiting } from "./Waiting";
import { Reports } from "./Reports";
import { Budget } from "./Budget";
import { Chat } from "./Chat";

// The five views, in tab order (SHELL-1). Settings (CORE-5) and Loadout (IT-1) are added by
// their issues.
export const VIEWS = {
  Sky,
  "Waiting on you": Waiting,
  Reports,
  Budget,
  Chat,
} as const;

export type ViewName = keyof typeof VIEWS;
