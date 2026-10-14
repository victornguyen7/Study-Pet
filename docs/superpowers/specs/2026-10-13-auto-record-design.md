# Automatic recording replaces the focus block

Date: 2026-10-13. Status: design, approved in chat, not built.

## Why

A day counts today only if the user presses Start. On a day they do not, nothing counts: no streak, no heatmap square, no focus keeper. The pet already labels every stretch of the day as Study, Work, Relax or Private (`backend/modes.py`), so it can count the time itself.

## What changes, in one paragraph

The focus block, its countdown and its Start button are removed. A day's recorded time is its Study plus Work time, taken from the labels the pet already makes. The streak and the heatmap count that time. The focus keeper turns itself on after steady work, and the pet suggests a break after a long stretch. Days before the switch keep the minutes they had.

## Decisions made with the user

| Question | Answer |
| --- | --- |
| When does the focus keeper nudge? | By itself, after about 5 minutes of Study or Work |
| What happens to breaks? | The pet suggests one after a long stretch. No countdown |
| What happens to time per task? | Dropped. The pet cannot tell which task an app belongs to |
| What happens to past days? | They keep their focus-block minutes and show as Study |

## 1. Recorded time

- **What counts.** A day's recorded minutes are the Study and Work seconds in `label_day`'s `totals`. Relax, Private and unknown time do not count.
- **Unknown apps.** An app or site nothing recognizes counts as nothing until the user says what it is for. The answer then counts its past time too, because labels are worked out when asked for.
- **The context guess goes.** `classify`'s `in_session` rule ("an unknown app inside a focus block is Study") stays in the code only so an old day's recap reads as it did. No new day has a block, so it never fires again.
- **Midnight.** A span belongs to the local day it started on, as `ON_DAY` in `modes.rs` already says.

### Storage

- **New table** `day_total (day TEXT PRIMARY KEY, study_secs INTEGER, work_secs INTEGER)`. It holds finished days only.
- **Filling it.** When minutes per day are asked for, each day from `auto_since` to yesterday that has no row is labelled once and saved. Today is always labelled live.
- **Clearing it.** `set_rule` and forgetting a rule (`learned`) delete every row, so the next ask rebuilds them with the new answer.
- **New setting** `auto_since`: the local date of the first launch of this version. It is written once and never changed.

### Minutes per day

`streak_input` (`db.rs`) returns the same shape as today, built from two sources:

| Day | Minutes | Mode |
| --- | --- | --- |
| Before `auto_since` | Sum of that day's `session` rows, as now | Study |
| `auto_since` to yesterday | `day_total` | The larger of Study and Work; a tie is Study |
| Today | `label_day`, live | The same rule |

It also returns `modes: { "YYYY-MM-DD": "study" | "work" }` for the heatmap. Because labelling needs the sidecar, the function moves out of plain SQL: Rust reads the rows, the sidecar labels the missing days, Rust saves them.

## 2. Streak and heatmap

- **Streak.** `backend/streaks.py` does not change. It still takes minutes per day, today and the threshold.
- **Heatmap.** Each cell takes its hue from the day's mode (blue for Study, orange for Work) and its shade from the total. The steps stay at 1, 2 and 4 hours. The blue ramp is the one in `panel.css` now (`--heat-1` to `--heat-4`, built around `--study`). Work needs a second ramp of four oranges around `--work`, for light and dark, checked against `--surface` the same way. The key under the heatmap shows both.
- **Goal.** Recorded time adds up faster than block time. The default threshold stays at 25 minutes and the setting is unchanged; the user can raise it.
- **Wrap up text.** "You focused for 2 h across 3 blocks today" becomes "You studied or worked for 2 h today". The list of today's blocks is removed; the recap's mode bar already shows the day.

## 3. Focus keeper, automatic

All of this is in `backend/drift.py`, which already gets a sample of the front app every 5 seconds. `Drift` gains two numbers: productive seconds so far, and when the keeper turned on.

| Event | Rule |
| --- | --- |
| **On** | 5 minutes of Study or Work samples have added up (`ON_AFTER = 300`). Unknown, Private and music samples neither add nor reset. |
| **Drift** | While on, a Relax sample starts the ladder, exactly as now (30 s, 2 min, 5 min, 10 min). |
| **Off: a break** | 15 minutes of Relax in a row (`OFF_AFTER = 900`). The ladder stops and the 5 minutes must be earned again. |
| **Off: away** | No sample for 5 minutes (`AWAY_SECS = 300`): the lid was closed or nobody was there. |
| **Off: declared** | The user has declared Relax. As now. |
| **Off: told to look away** | `AWAY_UNTIL` in `browser.rs`. As now. |

The rule lives in a new class, `Keeper`, which wraps `Drift`, and is reached through a new sidecar method, `watch`. Its reply is `Drift`'s plus `on: bool` and `break: bool`. The pet's face and nudges use `on` where they used `phase === "focus"`. The pet "watches" whenever the keeper is on.

The pet's lines can no longer name a task or the minutes left in a block, so each voice gets new lines that name only the time and what it went to.

The task reminder and the "what is this app for?" question wait while the keeper is on, as they waited during a focus block.

## 4. Break suggestion

- `Drift` also counts the current run: time since the keeper turned on, with no pause of 5 minutes.
- When the run reaches the break length, the reply carries `break: true` on that one update. Rust plays Glass and sends the pet's line as a `reminder` event: "You've been at it for 50 minutes. Take a break?"
- It is said once per run. A break (either "off" rule above) ends the run, and the next run can suggest again.
- **New setting** `break_after_min`, default 50, shown in Settings under "Focus keeper". 0 turns the suggestion off.
- There is no countdown, no "come back" call, and no Ping.

## 5. What is removed

| Removed | Where |
| --- | --- |
| The timer state machine and its tests | `backend/timer.py`, `backend/tests/test_timer.py` |
| The `timer` method and the running-block patch in `modes` | `backend/main.py` |
| The `timer` command, `timer_action`, `save_session`, `Session` | `src-tauri/src/session.rs`, `db.rs` |
| Presets: `list_presets`, `add_preset`, `delete_preset` | `db.rs`, the panel |
| The 1-second timer tick. The once-a-minute work stays, on a 60-second loop | `lib.rs` |
| The pause in recording during a break (`PAUSED`). `AWAY_UNTIL` stays | `browser.rs`, `lib.rs` |
| The "start a focus block?" offer (`studying`, `offer`) | `modes.rs` |
| The tank at the top of Today: session controls, the clock, the water level | `src/panel/main.ts`, `panel.html`, `panel.css`, `src/timer.ts` |
| Start on a task, and the time spent shown on a task | `src/panel/main.ts` |
| The pet's break and callback lines | `src/pet/main.ts` |
| Quitting stops the timer first | `lib.rs` |

The `session` and `preset` tables stay in `pet.db`. `session` is read for days before `auto_since`; `preset` is not read at all. Nothing is dropped, so the old everyday app's data survives a reinstall either way.

Two things stay on purpose:

- **`tasks.rs` still subtracts old blocks from a task's work left.** A task that had focus blocks before the switch keeps that credit in its reminder. No new time is ever added, and the panel no longer shows it.
- **The sidecar keeps `timer` and `drift` until the everyday app is reinstalled.** The app in `/Applications` starts Python from this source tree (`KNOWN_ISSUES.md`, item 33), so removing either method would break it at once. Everything before that step only adds to `backend/`. `backend/timer.py`, its test, and the two methods are deleted in the last step, together with a reinstall the user asks for.

## 6. Tests

- **`test_drift.py`**: the keeper turns on after 5 minutes of Study; unknown samples do not turn it on; 15 minutes of Relax turns it off; a 5-minute gap turns it off; the break suggestion comes once per run and again after a break; a break length of 0 never suggests.
- **Rust, one test**: with a `session` row before `auto_since` and a `day_total` row after, minutes per day and modes come out as the table in section 1 says, and `set_rule` empties `day_total`.
- **Removed**: `test_timer.py` and the Rust tests that start a focus block.
- **By hand**: run the dev app, study for 5 minutes, open a Relax site, and see the pet nudge.

## 7. Docs to update when built

`docs/spec.md` (S-1 to S-7 and C-4 are replaced), `ARCHITECTURE.md` (the focus-block example, the tables, the command list), `DECISIONS.md` (one entry for this change, replacing "The focus keeper is on during a focus block only"), `CHANGELOG.md`, `ROADMAP.md`.

## Out of scope

- Time per task, by any means.
- Re-counting days before `auto_since`.
- Telling Study from Work inside one app.
- Any change to the privacy gate, the extension or the task fetcher.

## Known limits

- **A wrong label counts wrong.** The streak is now only as good as the mode rules. An unanswered app is uncounted time until the user answers.
- **"I'm relaxing" lasts until midnight.** Starting a focus block used to take it back. Now only the user does, so the keeper stays off for the rest of that day.
- **A quiet break is missed.** Reading a paper book beside the laptop is 5 minutes "away", so the keeper turns off. That is the intended reading.
- **Changing a rule re-labels every finished day since `auto_since`.** That is one `label_day` per day, done once. If it gets slow after a year of history, re-label only days that had that app.
