import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { currentMonitor, getCurrentWindow, LogicalPosition, LogicalSize } from "@tauri-apps/api/window";
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
  if (dragged) return;
  reminder = ""; // opening the panel is the answer to it
  invoke("toggle_panel");
});

// A task that has become due soon, or an imported one that has passed its due time and may have been
// handed in (take_reminder in src-tauri/src/tasks.rs). Rust never sends one during a
// focus block. It stays in the bubble until the pet is clicked or a focus block starts.
let reminder = "";
listen<string>("reminder", (e) => {
  reminder = e.payload;
  getCurrentWindow().show();
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

// ---- Drift ladder: Drift.update in backend/drift.py, sent with each sample of the front app ----
// `mode` is what is in front right now (not smoothed), `audio` whether a tab in Chrome is playing.
type DriftState = {
  step: 0 | 1 | 2 | 3 | 4; // 4: the site is blocked, if the user opted in (Settings)
  seconds: number;
  what: string | null;
  returned: boolean;
  mode: "study" | "work" | "relax" | "private" | null;
  audio: boolean;
};
let drift: DriftState = { step: 0, seconds: 0, what: null, returned: false, mode: null, audio: false };
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
  const before = drift.step;
  drift = e.payload;
  // step 1: a glance, which is the slime leaning over for a moment
  if (drift.step === 1 && before === 0 && !calm())
    pet.animate([{ rotate: "0deg" }, { rotate: "-8deg" }, { rotate: "8deg" }, { rotate: "0deg" }], { duration: 900, iterations: 2 });
  if (drift.step >= 2) getCurrentWindow().show();
  sit(drift.step >= 3);
  // coming back is always rewarded
  if (drift.returned) {
    welcome = true; // the Happy state, drawn with the rest in onTimer
    setTimeout(() => (welcome = false), 6000);
  }
});

// The pet's voice (Settings, "Focus keeper"). Every line is about the time, what it went to or the
// task; none says anything about the user.
type Voice = { bubble: (l: Line) => string; sitting: (l: Line) => string; welcome: string };
type Line = { min: number; what: string | null; task: string; left: number };
const VOICES: Record<string, Voice> = {
  gentle: {
    bubble: (l) => `${l.min} min on ${l.what}. ${l.task} has ${l.left} min left.`,
    sitting: (l) => `${l.min} min on ${l.what} now. I'll sit here until you're back on ${l.task}.`,
    welcome: "Welcome back!",
  },
  sassy: {
    bubble: (l) => `${l.what}, ${l.min} min and counting. ${l.task} is still waiting, ${l.left} min left.`,
    sitting: (l) => `This is my seat now. ${l.min} min on ${l.what}, and ${l.task} misses you.`,
    welcome: "Look who's back. Let's go.",
  },
  roast: {
    bubble: (l) => `${l.min} min of ${l.what}? ${l.task} won't finish itself. ${l.left} min left.`,
    sitting: (l) => `${l.what} has had ${l.min} min. I'm sitting here until ${l.task} gets some.`,
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
function nudge(s: TimerState): string {
  if (awayNote) return awayNote;
  if (welcome) return voice.welcome;
  if (s.phase !== "focus" || drift.step < 2 || away()) return "";
  const line = {
    min: Math.floor(drift.seconds / 60),
    what: drift.what,
    task: s.label ? `“${s.label}”` : "this block",
    left: Math.ceil(s.remaining / 60),
  };
  return drift.step === 2 ? voice.bubble(line) : voice.sitting(line);
}

let lastCalls = 0;
onTimer((s) => {
  // P-2: eyes open while a focus block runs, and shut again while something Private is in front
  pet.classList.toggle("watching", s.phase === "focus" && drift.mode !== "private" && !away());
  // the other states (pet.css): lying down, headphones on, rocking at the user, hopping for joy
  pet.classList.toggle("relaxing", s.phase !== "focus" && drift.mode === "relax");
  pet.classList.toggle("headphones", drift.audio);
  pet.classList.toggle("nudging", s.phase === "focus" && drift.step >= 2 && !welcome && !away());
  pet.classList.toggle("happy", welcome);
  if (s.phase === "focus") reminder = "";
  if (s.phase !== "focus" || away()) sit(false); // the block is over, or the pet was told to look away
  // the call back from a break comes first; otherwise a reminder is worth more than "stand up and stretch"
  const text = s.phase === "callback" ? line(s) : nudge(s) || reminder || line(s);
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
