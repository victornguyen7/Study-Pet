import { invoke } from "@tauri-apps/api/core";
import { onStreak, type StreakState } from "../streak";
import { onTimer, timer } from "../timer";

type Preset = { id: number; name: string; focus_min: number; break_min: number };
// Mirrors day_data() in src-tauri/src/lib.rs
type Day = {
  sessions: { started: number; seconds: number; planned_min: number }[]; // today's focus blocks
  minutes: Record<string, number>; // "YYYY-MM-DD" -> focus minutes
  today: string;
  threshold: number;
};

// Mirrors Task in src-tauri/src/lib.rs
type Task = {
  id: number;
  title: string;
  course: string | null;
  due: string | null; // "YYYY-MM-DD"
  estimate_min: number | null;
  done: boolean;
};

const $ = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;
const focus = $<HTMLInputElement>("focus");
const brk = $<HTMLInputElement>("break");
const form = $<HTMLFormElement>("start");
const threshold = $<HTMLInputElement>("threshold");

function el(tag: string, text = "", className = "") {
  const e = document.createElement(tag);
  e.textContent = text;
  e.className = className;
  return e;
}

/** Runs a backend call and shows its error, if any, in the panel. */
async function run(call: () => Promise<unknown>) {
  try {
    await call();
    $("error").textContent = "";
  } catch (e) {
    $("error").textContent = `That didn't work: ${e}`;
  }
}

const mmss = (s: number) => `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
function duration(minutes: number) {
  if (minutes < 1) return "under 1 min";
  const m = Math.floor(minutes);
  return m < 60 ? `${m} min` : `${Math.floor(m / 60)} h ${m % 60} min`;
}
const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;

// ---- view switcher ----

const views = [...document.querySelectorAll<HTMLButtonElement>("nav button")];
for (const button of views)
  button.onclick = () => {
    for (const b of views) {
      if (b === button) b.setAttribute("aria-current", "page");
      else b.removeAttribute("aria-current");
      $(b.dataset.view!).hidden = b !== button;
    }
  };

// ---- presets (S-1): a preset fills in the lengths; the user can still change them ----

async function loadPresets() {
  const presets = await invoke<Preset[]>("list_presets");
  $("presets").replaceChildren(
    ...presets.map((p) => {
      const b = el("button", p.name);
      b.onclick = () => {
        focus.value = String(p.focus_min);
        brk.value = String(p.break_min);
      };
      return b;
    }),
  );
  $("preset-list").replaceChildren(
    ...presets.map((p) => {
      const li = el("li");
      const remove = el("button", "Remove");
      remove.setAttribute("aria-label", `Remove preset ${p.name}`);
      remove.onclick = () =>
        run(async () => {
          await invoke("delete_preset", { id: p.id });
          await loadPresets();
        });
      li.append(el("span", `${p.focus_min} min focus, ${p.break_min} min break`), remove);
      return li;
    }),
  );
  if (!presets.length)
    $("preset-list").append(el("li", "No presets yet.", "empty"));
}

// ---- session controls ----

form.onsubmit = (e) => {
  e.preventDefault();
  run(() =>
    timer({ action: "start_focus", focus_min: focus.valueAsNumber, break_min: brk.valueAsNumber }),
  );
};
$("save").onclick = () => {
  if (!form.reportValidity()) return;
  run(async () => {
    await invoke("add_preset", { focusMin: focus.valueAsNumber, breakMin: brk.valueAsNumber });
    await loadPresets();
  });
};
$("resume").onclick = () => run(() => timer({ action: "resume" }));
$("skip").onclick = () => run(() => timer({ action: "skip_break" }));
$("stop").onclick = () => run(() => timer({ action: "stop" }));

// ---- tasks: quick add, then a checkbox and a delete button per task ----

const taskText = $<HTMLInputElement>("task-text");
const filters = [...document.querySelectorAll<HTMLButtonElement>("#task-filter button")];
let filter = "today";

for (const button of filters)
  button.onclick = () => {
    filter = button.dataset.filter!;
    for (const b of filters) b.setAttribute("aria-pressed", String(b === button));
    run(loadTasks);
  };

async function loadTasks() {
  const all = await invoke<Task[]>("list_tasks");
  const today = new Date().toLocaleDateString("en-CA"); // "YYYY-MM-DD"
  // Today: due today or overdue, undated, and the ones checked off today (list_tasks returns no older done ones)
  const tasks = filter === "all" ? all : all.filter((t) => t.done || t.due === null || t.due <= today);
  $("tasks").replaceChildren(
    ...tasks.map((t) => {
      const li = el("li", "", t.done ? "done" : "");
      const label = el("label");
      const box = el("input") as HTMLInputElement;
      box.type = "checkbox";
      box.checked = t.done;
      box.onchange = () =>
        run(async () => {
          await invoke("set_task_done", { id: t.id, done: box.checked });
          await loadTasks();
        });
      label.append(box, el("span", t.title));

      const late = !t.done && t.due !== null && t.due < today;
      const due =
        t.due &&
        (t.due === today
          ? "today"
          : // noon avoids daylight-saving edges
            new Date(`${t.due}T12:00:00`).toLocaleDateString([], { weekday: "short", month: "short", day: "numeric" }));
      const details = [t.course, due && `${late ? "was due" : "due"} ${due}`, t.estimate_min && duration(t.estimate_min)];
      const remove = el("button", "×");
      remove.setAttribute("aria-label", `Delete task ${t.title}`);
      remove.onclick = () =>
        run(async () => {
          await invoke("delete_task", { id: t.id });
          await loadTasks();
        });
      li.append(label, el("span", details.filter(Boolean).join(" · "), late ? "late" : "muted"), remove);
      return li;
    }),
  );
  if (!tasks.length)
    $("tasks").append(el("li", all.length ? "Nothing for today. The rest is under All." : "No tasks yet. Type one above.", "empty"));
}

$<HTMLFormElement>("add-task").onsubmit = (e) => {
  e.preventDefault();
  run(async () => {
    await invoke("add_task", { text: taskText.value });
    taskText.value = "";
    await loadTasks();
  });
};

threshold.onchange = () => {
  if (threshold.reportValidity())
    run(() => invoke("set_streak_threshold", { minutes: threshold.valueAsNumber }));
};

const titles = { idle: "", focus: "Focus", break: "Break", callback: "Break's over" };
const level = document.querySelector<HTMLElement>(".level")!;

// ---- the day: today's blocks, the wrap-up and the 7-day strip ----

async function loadDay(streak: StreakState) {
  const day = await invoke<Day>("day");

  $("blocks").replaceChildren(
    ...day.sessions.map((s) => {
      const li = el("li");
      const time = new Date(s.started * 1000).toLocaleTimeString([], { timeStyle: "short" });
      li.append(el("span", time), el("span", duration(s.seconds / 60)));
      return li;
    }),
  );
  if (!day.sessions.length)
    $("blocks").append(el("li", "No focus blocks yet today. Start one above.", "empty"));

  const left = Math.ceil(streak.threshold - (day.minutes[day.today] ?? 0));
  const goal = streak.today_counted
    ? "Today counts toward your streak."
    : `${plural(left, "more minute")} and today counts.`;
  const streakText = `${plural(streak.days, "day")} in a row`;
  $("streak-line").textContent = `${streakText}. ${goal}`;

  $("wrap-total").textContent = day.sessions.length
    ? `You focused for ${duration(day.minutes[day.today] ?? 0)} across ${plural(day.sessions.length, "block")} today.`
    : "Nothing logged today yet. A focus block of any length counts.";
  $("wrap-streak").textContent =
    `${goal} Your streak is ${streakText}, with ${plural(streak.freezes_left, "freeze")} left this month.`;

  // 7 days ending today; noon avoids daylight-saving edges when stepping by day
  const days = Array.from({ length: 7 }, (_, i) => {
    const d = new Date(`${day.today}T12:00:00`);
    d.setDate(d.getDate() - (6 - i));
    return { date: d, minutes: day.minutes[d.toLocaleDateString("en-CA")] ?? 0 };
  });
  const tallest = Math.max(day.threshold, ...days.map((d) => d.minutes));
  $("week").replaceChildren(
    ...days.map((d, i) => {
      const li = el("li", "", d.minutes >= day.threshold ? "counted" : "");
      const bar = el("div", "", "bar");
      bar.style.height = `${(d.minutes / tallest) * 100}%`;
      const name = d.date.toLocaleDateString([], { weekday: "short" });
      li.append(
        el("span", String(Math.floor(d.minutes))),
        bar,
        el("span", name, i === 6 ? "today" : ""),
      );
      li.setAttribute("aria-label", `${name}: ${duration(d.minutes)}`);
      return li;
    }),
  );
}

run(loadPresets);
run(loadTasks);
run(() =>
  onTimer((s) => {
    $("idle").hidden = s.phase !== "idle";
    $("running").hidden = s.phase === "idle";
    $("phase").textContent = titles[s.phase];
    $("clock").textContent = s.phase === "callback" ? "" : mmss(s.remaining);
    $("resume").hidden = s.phase !== "callback";
    $("skip").hidden = s.phase !== "break";
    // water rises through a focus block, drains through a break
    const filled =
      s.phase === "focus"
        ? 1 - s.remaining / (s.focus_min * 60)
        : s.phase === "break"
          ? s.remaining / (s.break_min * 60)
          : 0;
    level.style.setProperty("--level", `${filled * 100}%`);
    level.hidden = filled === 0;
  }),
);
run(() =>
  onStreak((s) => {
    if (document.activeElement !== threshold) threshold.value = String(s.threshold);
    run(() => loadDay(s));
  }),
);
