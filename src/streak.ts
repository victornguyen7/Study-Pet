import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// Mirrors streak() in backend/streaks.py
export type StreakState = {
  days: number;
  today_minutes: number;
  today_counted: boolean;
  threshold: number; // study minutes a day needs to count
  freezes_left: number; // this month
  milestone: 0 | 7 | 30 | 100; // highest one reached
};

/** Calls `render` now and whenever the streak changes. */
export async function onStreak(render: (state: StreakState) => void) {
  await listen<StreakState>("streak", (e) => render(e.payload));
  await invoke("streak");
}
