# Decisions

One entry per decision: what, why, what it costs, when to revisit. Newest phase last.
Requirement IDs (P-1, PR-4, ...) refer to `spec.md`.

## Phase 1 · Stack

### Tauri for the desktop shell
- **Why:** the pet needs a transparent, always-on-top window (P-1), under 300 MB of memory and a 3 s startup, and a Rust core that can call macOS APIs (NSWorkspace, ScreenCaptureKit) in Phases 3-4.
- **Rejected:** Electron (memory and bundle size).
- **Gave up:** the Mac App Store, since window transparency needs `macOSPrivateApi`.
- **Revisit when:** distributing publicly.

### macOS first
- **Why:** it is the laptop the first user studies on; the spec's Windows column is "later".
- **Gave up:** nothing yet. The code already assumes it (`macOSPrivateApi`, the Option key in the panel), although `spec.md` still lists it as an open decision.
- **Revisit when:** Phase 3, where the window monitor becomes platform-specific code.

### Vite with plain TypeScript, no UI framework
- **Why:** two small pages (`index.html` for the pet, `panel.html` for the panel) that only render state pushed from Rust. A framework would add more code than it removes.
- **Gave up:** components and reactive state; the DOM is updated by hand in `src/panel/main.ts`.
- **Revisit when:** the panel grows real views (Today list in Phase 2, graph in Phase 5).

### Rust core owns state, storage and windows
- **Why:** one place holds the database, the one-second tick loop, both windows, the global shortcut and the event broadcast. Python never touches the database, and the UI never holds state.
- **Gave up:** nothing so far.
- **Revisit when:** `src-tauri/src/lib.rs` (one file, about 650 lines) gets hard to navigate.

### Python sidecar for logic
- **Why:** the spec wants extraction and graph jobs (PyMuPDF, a local model) in a process with no network access (PR-6). Timer and streak rules live there too, so all logic is in one language and easy to unit test.
- **Gave up:** every one-second tick is a Rust-to-Python round trip, and the app cannot run a timer without `backend/.venv`.
- **Revisit when:** the tick shows up in CPU use (target: under 5%), or packaging Python becomes the blocker.

## Phase 1 · Windows

### Two windows; closing the panel only hides it
- **Why:** the pet stays tiny and transparent, the panel is a normal window. Hiding instead of closing lets a click on the pet reopen it (P-3). See `lib.rs:515`.
- **Gave up:** nothing.

### Hide shortcut hardcoded to Alt+Shift+P
- **Why:** P-1 asks for one shortcut; one constant is the least code (`lib.rs:490`).
- **Gave up:** it is not configurable and may clash with another app.
- **Revisit when:** it clashes, or PR-3's "look away" shortcut is added.

### UI is render-only; Rust pushes `timer` and `streak` events
- **Why:** both windows show the same state, so Rust broadcasts it (timer every second, streak every minute and after each block) and the windows just draw it. See `src/timer.ts`, `src/streak.ts`.
- **Gave up:** the panel re-renders every second even when nothing changed.

### No content security policy (`csp: null`)
- **Why:** the scaffold default; the app only loads its own bundled pages.
- **Gave up:** a layer of defense if a page ever renders untrusted content.
- **Revisit when:** anything remote is loaded or captured page text is shown (Phase 4).

## Phase 1 · Sidecar

### JSON lines over stdin/stdout, one process behind one lock
- **Why:** the simplest protocol that works: one request line, one reply line, no ports and no network. The process starts on first use and is respawned after an error (`lib.rs:202-254`).
- **Gave up:** calls are serialized, so a long job would stall the timer tick.
- **Revisit when:** Phase 5 adds graph building; give long jobs their own process.

### Sidecar is started from the source tree
- **Why:** fastest path for development: `backend/.venv/bin/python -m backend.main` (`lib.rs:216`).
- **Gave up:** a release build cannot find Python, so the app only runs through `npm run tauri dev`.
- **Revisit when:** building a distributable; freeze `backend/` with PyInstaller and ship it as `bundle.externalBin`.

### Python logic takes `now` as an argument
- **Why:** `Timer` and `streak` never read the clock themselves, so tests can fake time (`backend/tests/`).
- **Gave up:** nothing.

## Phase 1 · Timer

### Wall-clock time, and a tick that catches up
- **Why:** the laptop sleeps with the lid closed. `time.time()` plus plain `if`s in `Timer.tick` walk through every phase that was missed (`backend/main.py:29`, `backend/timer.py:48`).
- **Gave up:** time asleep during a focus block counts as focus, up to the planned length.
- **Revisit when:** Phase 3 can tell whether the user was actually there.

### Timer state lives in memory only
- **Why:** less code; only finished blocks are stored (`timer.py:11`).
- **Gave up:** quitting mid-session loses the block in progress. The spec allows losing at most the last minute.
- **Revisit when:** a block is lost in real use.

### A block of any length counts, including one stopped early
- **Why:** S-6, no per-session minimum (`timer.py:82`).
- **Gave up:** nothing.

## Phase 1 · Storage

### Plain SQLite, not encrypted
- **Why:** Phase 1 stores only session times and presets, nothing sensitive.
- **Gave up:** PR-4 (encryption at rest) is not met yet.
- **Revisit when:** before Phase 4 stores captured text; switch rusqlite to `bundled-sqlcipher` with a key in the keychain (`lib.rs:16`).

### Dev database in the project root, release in the OS app data dir
- **Why:** `pet.db` is easy to inspect during development (`lib.rs:457`). It is in `.gitignore`.
- **Gave up:** dev and release builds do not share data.

### Schema changes through `PRAGMA user_version`
- **Why:** a migration is one `if version < N` block, with no migration library (`lib.rs:70`).
- **Gave up:** no down-migrations.

### Times stored as UTC text, days grouped by local time
- **Why:** UTC is unambiguous in storage; the streak is about the user's own day (`lib.rs:153-182`).
- **Gave up:** a block that crosses midnight counts wholly toward the day it started.
- **Revisit when:** late-night sessions make a day's total look wrong.

## Phase 1 · Streaks

### Streak is recomputed from the full session history
- **Why:** no stored streak to drift out of sync; the rules are one pure function (`backend/streaks.py`).
- **Gave up:** changing the threshold re-judges past days.
- **Revisit when:** past days must stay as they were judged; store a per-day "counted" flag (the spec's Day summary).

### Freeze rules
- **Why:** the spec says two freezes a month. The code spends one on the day of the miss, only when a streak exists; a frozen day keeps the streak but adds nothing; today never breaks a streak because it is not over.
- **Gave up:** nothing. These details are not in the spec, so this entry is their only record.

## Phase 2 · Task storage

### Two due columns on `task`: `due_at` and `due_date`
- **Why:** an assignment due at 11:59 pm needs a UTC time; an all-day item has only a date, and forcing a time on it would shift it across time zones. A check stops both being set at once.
- **Gave up:** every query that sorts by due has to combine the two (`coalesce(due_date, date(due_at, 'localtime'))`).

### Calendar events get their own table
- **Why:** events are not tasks. They have no done state and never merge with anything, so putting them in `task` would mean columns that are always empty and filters on every query.
- **Gave up:** the Today view has to read two tables.

### `(source, source_uid)` is UNIQUE in the database
- **Why:** the spec says a re-sync must never bring back a checked task. With the constraint, a duplicate import fails in SQLite even if the import code has a bug.
- **Gave up:** nothing.

### Deleting a task keeps its focus blocks
- **Why:** `session.task_id` is `ON DELETE SET NULL`, so study time and the streak survive; the task's source links are deleted with it.
- **Gave up:** a deleted task's blocks no longer say what they were for.

### Status is `todo` or `done` only
- **Why:** nothing else is needed yet. "Removed at the source" is recorded on the source link, not as a status.
- **Revisit when:** the user wants to drop a task without deleting it.

## Phase 2 · Quick add

### Due date and estimate are read off the end of the line only
- **Why:** the same words inside a title stay untouched ("read Friday Night Lights sat"). The two can come in either order.
- **Gave up:** "2h HW3 fri" leaves "2h" in the title.

### A weekday name means the coming one, and "next" is ignored
- **Why:** a weekday that is today means today, so a task due in hours does not slip a week. Words that only lead into the date ("due", "on", "by", "next", "this", ...) are dropped with it.
- **Gave up:** "next Friday" cannot mean the Friday after this one, and a title that really ends in one of those words loses it ("check in fri" becomes "check").
- **Revisit when:** a task lands on the wrong week in real use.

### The course is guessed from its shape
- **Why:** letters then a number at the start of the line ("STAT 310", "cs61a") needs no list of courses.
- **Gave up:** "read 100 pages" becomes course READ 100, and "Stat HW 5" gets no course.
- **Revisit when:** the Canvas import supplies the real course names; match against those.

### Quick add gives a date, never a time
- **Why:** typed lines like "Fri" carry no time. Only `due_date` is filled; `due_at` waits for imports.
- **Revisit when:** typed times ("Fri 5pm") are wanted.

## Phase 2 · Task list

### One list for now, not Today and All
- **Why:** it was the shortest path to a usable list. Open tasks sort by due date with undated ones last; tasks checked off today stay, struck through, until the day ends.
- **Gave up:** every open task shows whatever its date, and `planned_for` is never set.
- **Revisit when:** building the Today view.

### Delete is one click, with no confirmation or undo
- **Why:** the least code, and retyping a quick-add line is cheap.
- **Revisit when:** imported tasks appear; deleting one of those should probably hide it rather than remove it, or the next sync brings it back.

### The panel reloads the list instead of listening for an event
- **Why:** only the panel shows tasks, so there is nothing to keep in sync.
- **Revisit when:** the pet window shows tasks or reminders.

## Phase 2 · Windows

### The pet shows on every Space
- **Why:** a pet that stays behind on one desktop stops being a companion as soon as the user swipes to another. `visibleOnAllWorkspaces` on the pet window in both `tauri.conf.json` and `tauri.daily.conf.json` is one line of config.
- **Gave up:** the pet cannot be parked on one desktop; the hide shortcut is the only way to get it out of view.

### The panel moves to the active Space, through AppKit directly
- **Why:** the panel is hidden, not closed, so macOS remembered the Space it was last on and a click on the pet threw the user back there. Setting `NSWindowCollectionBehavior::MoveToActiveSpace` once at startup makes it appear where the user is (`move_to_active_space`, `lib.rs:431`). Tauri has no config key for this behavior, so it is set on the `NSWindow` itself.
- **Rejected:** `visibleOnAllWorkspaces` on the panel too; a normal window that follows across every desktop is in the way.
- **Gave up:** a macOS-only dependency (`objc2-app-kit`) and one `unsafe` pointer cast, both behind `cfg(target_os = "macos")`. Other platforms get no equivalent.
- **Revisit when:** Tauri exposes the collection behavior, or the Windows build starts.
