import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// Mirrors Timer.state() in backend/timer.py
export type TimerState = {
  phase: "idle" | "focus" | "break" | "callback";
  remaining: number; // seconds left in the focus block or break
  focus_min: number;
  break_min: number;
  label: string | null;
  calls: number; // times the pet has called the user back from this break
  suggest_real_break: boolean;
};

type Action =
  | { action: "tick" | "skip_break" | "resume" | "stop" }
  | { action: "start_focus"; focus_min: number; break_min: number; label?: string };

export const timer = (params: Action) => invoke<TimerState>("timer", { params });

/** Calls `render` now and on every change (Rust re-broadcasts the state each second). */
export async function onTimer(render: (state: TimerState) => void) {
  await listen<TimerState>("timer", (e) => render(e.payload));
  await timer({ action: "tick" });
}
