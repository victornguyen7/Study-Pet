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
- **Revisit when:** `src-tauri/src/lib.rs` (one file, about 430 lines) gets hard to navigate.

### Python sidecar for logic
- **Why:** the spec wants extraction and graph jobs (PyMuPDF, a local model) in a process with no network access (PR-6). Timer and streak rules live there too, so all logic is in one language and easy to unit test.
- **Gave up:** every one-second tick is a Rust-to-Python round trip, and the app cannot run a timer without `backend/.venv`.
- **Revisit when:** the tick shows up in CPU use (target: under 5%), or packaging Python becomes the blocker.

## Phase 1 · Windows

### Two windows; closing the panel only hides it
- **Why:** the pet stays tiny and transparent, the panel is a normal window. Hiding instead of closing lets a click on the pet reopen it (P-3). See `lib.rs:358`.
- **Gave up:** nothing.

### Hide shortcut hardcoded to Alt+Shift+P
- **Why:** P-1 asks for one shortcut; one constant is the least code (`lib.rs:341`).
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
- **Why:** the simplest protocol that works: one request line, one reply line, no ports and no network. The process starts on first use and is respawned after an error (`lib.rs:163-215`).
- **Gave up:** calls are serialized, so a long job would stall the timer tick.
- **Revisit when:** Phase 5 adds graph building; give long jobs their own process.

### Sidecar is started from the source tree
- **Why:** fastest path for development: `backend/.venv/bin/python -m backend.main` (`lib.rs:172`).
- **Gave up:** a release build cannot find Python, so the app only runs through `npm run tauri dev`.
- **Revisit when:** building a distributable; freeze `backend/` with PyInstaller and ship it as `bundle.externalBin`.

### Python logic takes `now` as an argument
- **Why:** `Timer` and `streak` never read the clock themselves, so tests can fake time (`backend/tests/`).
- **Gave up:** nothing.

## Phase 1 · Timer

### Wall-clock time, and a tick that catches up
- **Why:** the laptop sleeps with the lid closed. `time.time()` plus plain `if`s in `Timer.tick` walk through every phase that was missed (`backend/main.py:28`, `backend/timer.py:48`).
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
- **Why:** `pet.db` is easy to inspect during development (`lib.rs:310`). It is in `.gitignore`.
- **Gave up:** dev and release builds do not share data.

### Schema changes through `PRAGMA user_version`
- **Why:** a migration is one `if version < N` block, with no migration library (`lib.rs:36`).
- **Gave up:** no down-migrations.

### Times stored as UTC text, days grouped by local time
- **Why:** UTC is unambiguous in storage; the streak is about the user's own day (`lib.rs:105-131`).
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
