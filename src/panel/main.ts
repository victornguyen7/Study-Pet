import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { onStreak, type StreakState } from "../streak";
import { onTimer, timer } from "../timer";

type Preset = { id: number; name: string; focus_min: number; break_min: number };
// Mirrors day_data() in src-tauri/src/db.rs
type Day = {
  sessions: { started: number; seconds: number; planned_min: number }[]; // today's focus blocks
  minutes: Record<string, number>; // "YYYY-MM-DD" -> focus minutes
  today: string;
  threshold: number;
};

// Mirrors Task in src-tauri/src/tasks.rs
type Task = {
  id: number;
  title: string;
  course: string | null;
  due: string | null; // "YYYY-MM-DD"
  estimate_min: number | null;
  done: boolean;
  source: Source | null; // where it was imported from
  removed: boolean; // imported, and no longer in its source's feed
  spent_min: number; // focus time in sessions tied to it
  same_as: { id: number; title: string; source: Source } | null; // a typed task's likely imported copy
  soon: boolean; // the time left until it is due is under twice the work left
  ask_submitted: boolean; // imported and past due: the feed can't say whether it was handed in
};
// Mirrors courses() in src-tauri/src/tasks.rs
type Course = { course: string; open: number; hidden: boolean };

type Source = "canvas" | "google";
const sourceNames = { canvas: "Canvas", google: "Google Calendar" };
// Mirrors feed_list() and store_feed() in src-tauri/src/feeds.rs
type Feed = {
  source: Source;
  set: boolean;
  file: boolean; // set from an uploaded .ics file rather than an address
  account: boolean; // set by signing in (Google only)
  // ok_at: the last good sync, kept while syncs fail
  status: { at: string; error?: string; ok_at?: string | null; tasks?: number; events?: number } | null;
};
// Mirrors events_today() in src-tauri/src/feeds.rs
type CalendarEvent = { title: string; start_at: string; end_at: string; all_day: boolean };

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

// S-2: a session can be for one task. `task` comes from a task's own start button, or from the "Working on" list.
const taskPick = $<HTMLSelectElement>("task-pick");
let openTasks: Task[] = [];

function startFocus(task?: Task) {
  if (!form.reportValidity()) return;
  const tied = task && { label: task.title, task_id: task.id };
  run(() => timer({ action: "start_focus", focus_min: focus.valueAsNumber, break_min: brk.valueAsNumber, ...tied }));
}

form.onsubmit = (e) => {
  e.preventDefault();
  startFocus(openTasks.find((t) => String(t.id) === taskPick.value));
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
  openTasks = all.filter((t) => !t.done);
  const picked = taskPick.value;
  taskPick.replaceChildren(
    new Option("No task", ""),
    ...openTasks.map((t) => new Option(t.course ? `${t.title} (${t.course})` : t.title, String(t.id))),
  );
  taskPick.value = openTasks.some((t) => String(t.id) === picked) ? picked : "";
  showFinished();
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
      const details = [
        t.course,
        due && `${late ? "was due" : "due"} ${due}`,
        t.estimate_min
          ? t.spent_min >= 1
            ? `${duration(t.spent_min)} of ${duration(t.estimate_min)}`
            : duration(t.estimate_min)
          : t.spent_min >= 1 && `${duration(t.spent_min)} spent`,
        t.removed && t.source && `removed from ${sourceNames[t.source]}`,
      ];
      const extras: HTMLElement[] = [];
      if (t.soon) extras.push(el("span", "start soon", "soon"));
      if (t.ask_submitted) {
        // stands in for Canvas submission status, which the feed does not carry
        const yes = el("button", "Did you submit it? Yes", "merge");
        yes.setAttribute("aria-label", `Yes, ${t.title} was submitted: check it off`);
        yes.onclick = () =>
          run(async () => {
            await invoke("set_task_done", { id: t.id, done: true });
            await loadTasks();
          });
        extras.push(yes);
      }
      const same = t.same_as;
      if (same) {
        // only a suggestion: the two become one task when the user says so
        const merge = el("button", `Same as “${same.title}” from ${sourceNames[same.source]}? Merge`, "merge");
        merge.onclick = () =>
          run(async () => {
            await invoke("merge_task", { typed: t.id, imported: same.id });
            await loadTasks();
          });
        extras.push(merge);
      }
      if (!t.done) {
        const start = el("button", "▶", "start");
        start.title = "Start a focus block for this task";
        start.setAttribute("aria-label", `Start a focus block for ${t.title}`);
        start.onclick = () => startFocus(t);
        extras.push(start);
      }
      const remove = el("button", "×");
      remove.setAttribute("aria-label", `Delete task ${t.title}`);
      remove.onclick = () =>
        run(async () => {
          await invoke("delete_task", { id: t.id });
          await loadTasks();
        });
      li.append(label, el("span", details.filter(Boolean).join(" · "), late ? "late" : "muted"), ...extras, remove);
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

// ---- imports: today's calendar events, and the feed addresses in Settings ----

const clock = (iso: string) => new Date(iso).toLocaleTimeString([], { timeStyle: "short" });

async function loadEvents() {
  const events = await invoke<CalendarEvent[]>("list_events");
  $("schedule").hidden = !events.length;
  $("events").replaceChildren(
    ...events.map((e) => {
      const li = el("li");
      const when = e.all_day ? "All day" : e.end_at === e.start_at ? clock(e.start_at) : `${clock(e.start_at)} – ${clock(e.end_at)}`;
      li.append(el("span", when, "when"), el("span", e.title));
      return li;
    }),
  );
}

function feedStatus(f: Feed) {
  if (!f.set) return "Not connected.";
  if (!f.status) return "Not synced yet.";
  if (f.status.error) return `Last sync failed at ${clock(f.status.at)}: ${f.status.error}.`;
  const counts = [f.status.tasks && plural(f.status.tasks, "assignment"), plural(f.status.events ?? 0, "event")];
  return `${f.account ? "Signed in. Synced" : f.file ? "Read from your file" : "Synced"} at ${clock(f.status.at)}: ${counts.filter(Boolean).join(", ")}.`;
}

/** Today says so when a connected feed has gone a day without a good sync; the reason is in Settings. */
async function loadStale() {
  const lines = (await invoke<Feed[]>("feeds")).flatMap((f) => {
    if (!f.set || !f.status) return [];
    const ok = f.status.error ? f.status.ok_at : f.status.at;
    if (!ok) return [`${sourceNames[f.source]} has never synced.`];
    if (Date.now() - Date.parse(ok) < 24 * 60 * 60 * 1000) return [];
    const when = new Date(ok).toLocaleString([], { weekday: "short", hour: "numeric", minute: "2-digit" });
    return [`${sourceNames[f.source]} has not synced since ${when}.`];
  });
  $("stale").hidden = !lines.length;
  $("stale").textContent = lines.length ? `${lines.join(" ")} This list may be out of date; see Settings.` : "";
}

async function loadFeeds() {
  // a sync can finish while an address is being typed; redrawing would wipe it
  if ($("feeds").querySelector<HTMLInputElement>("input[type=password]:focus, input[type=password]:not(:placeholder-shown)")) return;
  const feeds = await invoke<Feed[]>("feeds");
  $("feeds").replaceChildren(
    ...feeds.map((f) => {
      const name = sourceNames[f.source];
      const form = el("form", "", "feed") as HTMLFormElement;
      const input = el("input") as HTMLInputElement;
      input.type = "password"; // the address is a secret, so it is never shown back
      input.required = true;
      input.autocomplete = "off";
      input.placeholder = f.account
        ? "Signed in. Paste an address to replace it"
        : f.file
          ? "Using your file. Paste an address to replace it"
          : f.set
            ? "Saved. Paste a new address to replace it"
            : "Paste the feed address";
      input.setAttribute("aria-label", `${name} feed address`);
      const status = el("p", feedStatus(f), f.status?.error ? "late" : "muted");
      const save = (command: string, args: object) => {
        input.value = "";
        input.blur();
        status.textContent = "Syncing…";
        run(() => invoke(command, { source: f.source, ...args }).finally(loadFeeds));
      };
      const set = (url: string) => save("set_feed", { url });
      form.onsubmit = (e) => {
        e.preventDefault();
        set(input.value);
      };
      // for a calendar with no feed address: an exported .ics file, read here and stored as text
      const picker = el("input") as HTMLInputElement;
      picker.type = "file";
      picker.accept = ".ics,text/calendar";
      picker.hidden = true;
      picker.onchange = () => {
        const file = picker.files?.[0];
        if (file) run(async () => save("set_feed_file", { ics: await file.text() }));
      };
      const upload = el("button", "Upload .ics") as HTMLButtonElement;
      upload.type = "button";
      upload.setAttribute("aria-label", `Upload an .ics file for ${name}`);
      upload.onclick = () => picker.click();
      const row = el("div", "", "row");
      row.append(input, el("button", "Save"), upload, picker);
      if (f.source === "google") {
        // the fetcher opens the browser and waits for the sign-in to finish there
        const signIn = el("button", f.account ? "Sign in again" : "Sign in with Google") as HTMLButtonElement;
        signIn.type = "button";
        signIn.onclick = () => {
          status.textContent = "Waiting for the sign-in in your browser…";
          run(() => invoke("connect_google").finally(loadFeeds));
        };
        row.append(signIn);
      }
      if (f.set) {
        const remove = el("button", "Disconnect") as HTMLButtonElement;
        remove.type = "button";
        remove.setAttribute("aria-label", `Disconnect ${name}`);
        remove.onclick = () => set("");
        row.append(remove);
      }
      form.append(el("label", name), row, status);
      return form;
    }),
  );
}

// "Sync now" says how it went: the per-feed lines above have the detail
const syncButton = $<HTMLButtonElement>("sync");

function showSyncResult(feeds: Feed[]) {
  const connected = feeds.filter((f) => f.set);
  const failed = connected.filter((f) => f.status?.error).map((f) => sourceNames[f.source]);
  const latest = connected.map((f) => f.status?.at ?? "").sort().at(-1);
  const result = $("sync-result");
  result.className = failed.length ? "late" : connected.length ? "ok" : "muted";
  result.textContent = failed.length
    ? `${failed.join(" and ")} did not sync. See above.`
    : latest
      ? `✓ Synced at ${clock(latest)}.`
      : "Nothing to sync yet. Connect a feed above.";
}

// a later automatic sync redraws the line, so it never contradicts the feeds above it
async function refreshSyncResult() {
  if ($("sync-result").textContent && !syncButton.disabled) showSyncResult(await invoke<Feed[]>("feeds"));
}

syncButton.onclick = () =>
  run(async () => {
    const result = $("sync-result");
    result.className = "muted";
    result.textContent = "Syncing…";
    syncButton.disabled = true;
    try {
      await invoke("sync_now");
      showSyncResult(await invoke<Feed[]>("feeds"));
    } catch (e) {
      result.textContent = "";
      throw e;
    } finally {
      syncButton.disabled = false;
    }
  });

// ---- courses: which imported courses show on the task list ----

async function loadCourses() {
  const courses = await invoke<Course[]>("list_courses");
  $("courses").hidden = !courses.length;
  $("course-list").replaceChildren(
    ...courses.map((c) => {
      const li = el("li");
      const label = el("label");
      const box = el("input") as HTMLInputElement;
      box.type = "checkbox";
      box.checked = !c.hidden;
      box.onchange = () =>
        run(async () => {
          await invoke("set_course", { course: c.course, shown: box.checked });
          await Promise.all([loadTasks(), loadCourses()]);
        });
      label.append(box, el("span", c.course));
      li.append(label, el("span", plural(c.open, "open assignment"), "muted"));
      return li;
    }),
  );
}

// ---- "Finished it?": asked in the break after a focus block that was for a task ----

let lastTimer: { phase: string; task_id: number | null } = { phase: "idle", task_id: null };

function showFinished() {
  const resting = lastTimer.phase === "break" || lastTimer.phase === "callback";
  const task = resting ? openTasks.find((t) => t.id === lastTimer.task_id) : undefined;
  $("finished").hidden = !task;
  if (task) $("finished-text").textContent = `Finished “${task.title}”?`;
}

$("mark-done").onclick = () =>
  run(async () => {
    await invoke("set_task_done", { id: lastTimer.task_id, done: true });
    await loadTasks();
  });

threshold.onchange = () => {
  if (threshold.reportValidity())
    run(() => invoke("set_streak_threshold", { minutes: threshold.valueAsNumber }));
};

const sounds = $<HTMLInputElement>("sounds");
sounds.onchange = () => run(() => invoke("sounds", { on: sounds.checked }));
run(async () => {
  sounds.checked = await invoke<boolean>("sounds");
});

// the user's own word on the mode, which beats every rule in backend/modes.py
const mode = $<HTMLSelectElement>("mode");
mode.onchange = () => run(() => invoke("declared_mode", { set: mode.value }));
run(async () => {
  mode.value = (await invoke<string | null>("declared_mode")) ?? "";
});

// PR-2: apps and sites the pet could not place (label_day in backend/modes.py), and the answers it keeps
type Rule = { key: string; name: string; mode: string };
const MODES = ["study", "work", "relax", "private"];
function showLearned(rules: Rule[]) {
  $("learned").replaceChildren(
    ...rules.map((r) => {
      const li = el("li");
      const forget = el("button", "Forget");
      forget.onclick = () => run(async () => showLearned(await invoke<Rule[]>("learned", { key: r.key, mode: "" })));
      li.append(el("span", `${r.name}: ${r.mode}`), forget);
      return li;
    }),
  );
  if (!rules.length) $("learned").append(el("li", "Nothing yet.", "empty"));
}
async function loadAsks() {
  const { unknown } = await invoke<{ unknown: { key: string; name: string; seconds: number }[] }>("modes_today");
  $("ask").hidden = !unknown.length;
  $("ask-list").replaceChildren(
    ...unknown.map((u) => {
      const li = el("li");
      const buttons = el("span", "", "row");
      for (const mode of MODES) {
        const b = el("button", mode[0].toUpperCase() + mode.slice(1));
        b.onclick = () =>
          run(async () => {
            showLearned(await invoke<Rule[]>("learned", { key: u.key, name: u.name, mode }));
            await loadAsks();
          });
        buttons.append(b);
      }
      li.append(el("span", `${u.name} · ${duration(u.seconds / 60)} today`), buttons);
      return li;
    }),
  );
}
run(async () => showLearned(await invoke<Rule[]>("learned")));

// PR-10: macOS is only ever made to ask from this button, which sits under the explanation
async function loadAccessibility(ask = false) {
  const allowed = await invoke<boolean>("accessibility", { ask });
  $("ax-status").textContent = allowed ? "allowed" : "not allowed";
  $("ax-ask").hidden = allowed;
}
$("ax-ask").onclick = () => run(() => loadAccessibility(true));
// the answer is given in System Settings, so look again when the panel comes back to the front
window.addEventListener("focus", () => run(loadAccessibility));
run(loadAccessibility);

const titles = { idle: "", focus: "Focus", break: "Break", callback: "Break's over" };
const level = document.querySelector<HTMLElement>(".level")!;

// ---- the day: today's blocks, the wrap-up and the 7-day strip ----

function timeRows(list: string, rows: [string, number][]) {
  $(list).replaceChildren(
    ...rows.map(([name, seconds]) => {
      const li = el("li");
      li.append(el("span", name), el("span", duration(seconds / 60)));
      return li;
    }),
  );
  if (!rows.length) $(list).append(el("li", "Nothing noted yet today.", "empty"));
}

// Mirrors site_time_today() in src-tauri/src/browser.rs and app_time_today() in monitor.rs
async function loadSites() {
  const sites = await invoke<{ domain: string; seconds: number }[]>("site_time");
  timeRows("sites", sites.map((s) => [s.domain, s.seconds]));
  const apps = await invoke<{ app: string; seconds: number }[]>("app_time");
  timeRows("apps", apps.map((a) => [a.app, a.seconds]));
}
// the list also reloads by itself once a minute, with the rest of the day
$("sites-refresh").onclick = () => run(loadSites);

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
  // the fire is lit once today counts; stars match the ones the pet wears
  $("fire").classList.toggle("unlit", !streak.today_counted);
  $("streak-days").textContent = String(streak.days);
  $("streak-stars").textContent = "★".repeat([7, 30, 100].filter((m) => streak.days >= m).length);
  const progress = $<HTMLProgressElement>("streak-progress");
  progress.max = streak.threshold;
  progress.value = Math.min(day.minutes[day.today] ?? 0, streak.threshold);
  $("streak-line").textContent = `${goal} ${plural(streak.freezes_left, "freeze")} left this month.`;

  $("wrap-total").textContent = day.sessions.length
    ? `You focused for ${duration(day.minutes[day.today] ?? 0)} across ${plural(day.sessions.length, "block")} today.`
    : "Nothing logged today yet. A focus block of any length counts.";
  $("wrap-streak").textContent =
    `${goal} Your streak is ${streakText}, with ${plural(streak.freezes_left, "freeze")} left this month.`;

  await loadSites();
  await loadAsks();

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
// Rust sends "tasks" after every feed sync, whether it ran on the half hour or from Settings
const loadImports = () => Promise.all([loadTasks(), loadEvents(), loadFeeds(), loadStale(), loadCourses()]);
run(loadImports);
let shownDay = new Date().toDateString();
run(() =>
  listen("tasks", () => {
    run(loadImports);
    run(refreshSyncResult);
  }),
);
run(() =>
  onTimer((s) => {
    $("idle").hidden = s.phase !== "idle";
    $("running").hidden = s.phase === "idle";
    $("phase").textContent = titles[s.phase];
    $("working-on").hidden = !s.label;
    $("working-on").textContent = s.label ?? "";
    // a block just ended or began: the time spent on its task has changed
    const changed = s.phase !== lastTimer.phase;
    lastTimer = s;
    if (changed) run(loadTasks);
    else showFinished();
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
    // the streak arrives once a minute; the first one of a new day redraws what "today" means
    const day = new Date().toDateString();
    if (day !== shownDay) {
      shownDay = day;
      run(loadImports);
    }
  }),
);
