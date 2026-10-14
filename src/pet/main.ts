import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { currentMonitor, getCurrentWindow, LogicalPosition, LogicalSize } from "@tauri-apps/api/window";
import { onStreak } from "../streak";

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
  if (dragged) return;
  reminder = ""; // opening the panel is the answer to it
  invoke("toggle_panel");
});

// One line from Rust for the bubble: a task that has become due soon, an imported one that may have
// been handed in (take_reminder in src-tauri/src/tasks.rs), a question about an app, or "take a
// break". Rust sends none of the first three while the keeper is on. It stays until the pet is
// clicked or the keeper next turns on.
let reminder = "";
listen<string>("reminder", (e) => {
  reminder = e.payload;
  getCurrentWindow().show();
});

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

// ---- Drift ladder: Drift.update in backend/drift.py, sent with each sample of the front app ----
// `mode` is what is in front right now (not smoothed), `audio` whether any app is playing sound.
type DriftState = {
  step: 0 | 1 | 2 | 3 | 4; // 4: the site is blocked, if the user opted in (Settings)
  seconds: number;
  what: string | null;
  returned: boolean;
  on: boolean; // the keeper is on: study or work has added up (Keeper in backend/drift.py)
  mode: "study" | "work" | "relax" | "private" | null;
  audio: boolean;
};
let drift: DriftState = { step: 0, seconds: 0, what: null, returned: false, on: false, mode: null, audio: false };
let welcome = false; // "welcome back" is up
const calm = () => matchMedia("(prefers-reduced-motion: reduce)").matches;

// Step 3: the pet sits in the middle of the screen, in the way, and goes home when the user is back.
// ponytail: the middle of the screen, not the distracting window itself; finding that window's
// frame needs more of the Accessibility API. Dragging the pet while it sits moves it for good.
let home: { x: number; y: number } | null = null; // where the slime's feet were (bottom center)
async function sit(on: boolean) {
  if (on === !!home) return;
  const win = getCurrentWindow();
  const size = big ? BIG : SMALL;
  if (on) {
    const screen = await currentMonitor();
    if (!screen) return;
    const scale = await win.scaleFactor();
    const pos = (await win.outerPosition()).toLogical(scale);
    home = { x: pos.x + size.w / 2, y: pos.y + size.h };
    const origin = screen.position.toLogical(scale);
    const area = screen.size.toLogical(scale);
    await win.setPosition(new LogicalPosition(origin.x + (area.width - size.w) / 2, origin.y + (area.height - size.h) / 2));
  } else {
    const feet = home!;
    home = null;
    await win.setPosition(new LogicalPosition(feet.x - size.w / 2, feet.y - size.h));
  }
}

listen<DriftState>("drift", (e) => {
  const before = drift;
  drift = e.payload;
  if (drift.on && !before.on) reminder = ""; // back at work: an old line is out of date
  // step 1: a glance, which is the slime leaning over for a moment
  if (drift.step === 1 && before.step === 0 && !calm())
    pet.animate([{ rotate: "0deg" }, { rotate: "-8deg" }, { rotate: "8deg" }, { rotate: "0deg" }], { duration: 900, iterations: 2 });
  if (drift.step >= 2) getCurrentWindow().show();
  sit(drift.step >= 3);
  // coming back is always rewarded
  if (drift.returned) {
    welcome = true; // the Happy state, drawn with the rest in render
    setTimeout(() => (welcome = false), 6000);
  }
});

// The pet's voice (Settings, "Focus keeper"). Every line is about the time and what it went to;
// none says anything about the user.
type Voice = { bubble: (l: Line) => string; sitting: (l: Line) => string; welcome: string };
type Line = { min: number; what: string | null };
const VOICES: Record<string, Voice> = {
  gentle: {
    bubble: (l) => `${l.min} min on ${l.what}. Ready to get back to it?`,
    sitting: (l) => `${l.min} min on ${l.what} now. I'll sit here until you're back.`,
    welcome: "Welcome back!",
  },
  sassy: {
    bubble: (l) => `${l.what}, ${l.min} min and counting. The work is still waiting.`,
    sitting: (l) => `This is my seat now. ${l.min} min on ${l.what}, and the work misses you.`,
    welcome: "Look who's back. Let's go.",
  },
  roast: {
    bubble: (l) => `${l.min} min of ${l.what}? The work won't finish itself.`,
    sitting: (l) => `${l.what} has had ${l.min} min. I'm sitting here until the work gets some.`,
    welcome: "Finally. Back to it.",
  },
};
let voice = VOICES.gentle;
const useVoice = (k: { personality: string }) => (voice = VOICES[k.personality] ?? VOICES.gentle);
listen<{ personality: string }>("keeper", (e) => useVoice(e.payload));
invoke<{ personality: string }>("keeper").then(useVoice);

// PR-3: Option-Shift-L makes the pet look away for 15 minutes; `awayUntil` is epoch seconds, 0 when watching.
let awayUntil = 0;
let awayNote = ""; // said for a few seconds when it starts or ends
const away = () => Date.now() / 1000 < awayUntil;
listen<number>("away", (e) => {
  awayUntil = e.payload;
  const time = new Date(awayUntil * 1000).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
  awayNote = awayUntil ? `Looking away until ${time}. Same keys to bring me back.` : "I'm watching again.";
  getCurrentWindow().show();
  setTimeout(() => (awayNote = ""), 6000);
});

/** Steps 2 and up speak, in the chosen voice. */
function nudge(): string {
  if (awayNote) return awayNote;
  if (welcome) return voice.welcome;
  if (!drift.on || drift.step < 2 || away()) return "";
  const line = { min: Math.floor(drift.seconds / 60), what: drift.what };
  return drift.step === 2 ? voice.bubble(line) : voice.sitting(line);
}

function render() {
  const on = drift.on && !away();
  // P-2: eyes open while the keeper is on, and shut again while something Private is in front
  pet.classList.toggle("watching", on && drift.mode !== "private");
  // the other states (pet.css): lying down, headphones on, rocking at the user, hopping for joy
  pet.classList.toggle("relaxing", !drift.on && drift.mode === "relax");
  pet.classList.toggle("headphones", drift.audio);
  pet.classList.toggle("nudging", on && drift.step >= 2 && !welcome);
  pet.classList.toggle("happy", welcome);
  if (!on) sit(false); // the keeper let go, or the pet was told to look away
  const text = nudge() || reminder;
  bubble.textContent = text;
  if (!text) bubble.hidden = true; // hide before shrinking, show after growing
  resize(!!text).then(() => (bubble.hidden = !text));
}
// once a second: "welcome back", the away note and looking away all end by the clock, not by an event
setInterval(render, 1000);
render();

// The pet earns a star at each streak milestone: 7, 30 and 100 days.
onStreak((s) => {
  pet.dataset.stars = "★".repeat([7, 30, 100].filter((m) => s.days >= m).length);
});
