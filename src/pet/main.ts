import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow, LogicalPosition, LogicalSize } from "@tauri-apps/api/window";
import { onStreak } from "../streak";
import { onTimer, type TimerState } from "../timer";

const pet = document.getElementById("pet")!;
const bubble = document.getElementById("bubble")!;
let down: { x: number; y: number } | null = null;
let dragged = false;

// Drag moves the window (P-1), a plain click opens the panel (P-3).
pet.addEventListener("mousedown", (e) => {
  down = { x: e.screenX, y: e.screenY };
  dragged = false;
});
pet.addEventListener("mousemove", (e) => {
  if (!down || e.buttons !== 1) return;
  if (Math.hypot(e.screenX - down.x, e.screenY - down.y) > 4) {
    down = null;
    dragged = true;
    getCurrentWindow().startDragging();
  }
});
pet.addEventListener("click", () => {
  if (!dragged) invoke("toggle_panel");
});

function line(s: TimerState): string {
  if (s.phase === "callback")
    return s.calls > 1 ? "Still there? Let's get back to it." : "Break's over. Ready to go?";
  if (s.phase === "break")
    return s.suggest_real_break
      ? "That's 3 breaks skipped in a row. Take this one for real?"
      : "Break time. Stand up and stretch.";
  return "";
}

// The window is exactly the slime (120 x 80, as in pet.css and tauri.conf.json). It grows up and
// outward only while the bubble shows, and the slime stays where it is on screen.
const SMALL = { w: 120, h: 80 };
const BIG = { w: 220, h: 210 };
let big = false;
async function resize(grow: boolean) {
  if (grow === big) return;
  big = grow;
  const win = getCurrentWindow();
  const [from, to] = grow ? [SMALL, BIG] : [BIG, SMALL];
  const pos = (await win.outerPosition()).toLogical(await win.scaleFactor());
  await win.setPosition(new LogicalPosition(pos.x - (to.w - from.w) / 2, pos.y - (to.h - from.h)));
  await win.setSize(new LogicalSize(to.w, to.h));
}

let lastCalls = 0;
onTimer((s) => {
  pet.classList.toggle("watching", s.phase === "focus"); // P-2
  const text = line(s);
  bubble.textContent = text;
  if (!text) bubble.hidden = true; // hide before shrinking, show after growing
  resize(!!text).then(() => (bubble.hidden = !text));

  // S-4: each call back brings the pet out of hiding and makes it hop
  if (s.calls > lastCalls) {
    getCurrentWindow().show();
    if (!matchMedia("(prefers-reduced-motion: reduce)").matches)
      pet.animate(
        [{ translate: "0 0" }, { translate: "0 -10px" }, { translate: "0 0" }],
        { duration: 400, iterations: 6 },
      );
  }
  lastCalls = s.calls;
});

// The pet earns a star at each streak milestone: 7, 30 and 100 days.
onStreak((s) => {
  pet.dataset.stars = "★".repeat([7, 30, 100].filter((m) => s.days >= m).length);
});
