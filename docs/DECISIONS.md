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
- **Revisit when:** `src-tauri/src/lib.rs` (one file, about 1,140 lines) gets hard to navigate.

### Python sidecar for logic
- **Why:** the spec wants extraction and graph jobs (PyMuPDF, a local model) in a process with no network access (PR-6). Timer and streak rules live there too, so all logic is in one language and easy to unit test.
- **Gave up:** every one-second tick is a Rust-to-Python round trip, and the app cannot run a timer without `backend/.venv`.
- **Revisit when:** the tick shows up in CPU use (target: under 5%), or packaging Python becomes the blocker.

## Phase 1 · Windows

### Two windows; closing the panel only hides it
- **Why:** the pet stays tiny and transparent, the panel is a normal window. Hiding instead of closing lets a click on the pet reopen it (P-3). See `lib.rs:852`.
- **Gave up:** nothing.

### Hide shortcut hardcoded to Alt+Shift+P
- **Why:** P-1 asks for one shortcut; one constant is the least code (`lib.rs:827`).
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
- **Why:** the simplest protocol that works: one request line, one reply line, no ports and no network. The process starts on first use and is respawned after an error (`lib.rs:204-258`).
- **Gave up:** calls are serialized, so a long job would stall the timer tick.
- **Revisit when:** Phase 5 adds graph building; give long jobs their own process.

### Sidecar is started from the source tree
- **Why:** fastest path for development: `backend/.venv/bin/python -m backend.main` (`lib.rs:220`).
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
- **Gave up:** PR-4 (encryption at rest) is not met yet. Since Phase 2 this is no longer harmless: the database also holds the feed addresses, imported assignments and events, and any uploaded calendar file in full (`KNOWN_ISSUES.md`, items 1 and 2).
- **Revisit when:** before any release build, and at the latest before Phase 4 stores captured text; switch rusqlite to `bundled-sqlcipher` with a key in the keychain (`lib.rs:17`).

### Dev database in the project root, release in the OS app data dir
- **Why:** `pet.db` is easy to inspect during development (`lib.rs:783`). It is in `.gitignore`.
- **Gave up:** dev and release builds do not share data.
- **Reviewed 2026-10-04:** sharing one file between the dev build and the installed everyday app was considered and turned down for now. The dev build would run unfinished migrations on real data with no way back, two running timers would count study time twice, and neither app sees the other's writes until it reloads.
- **Revisit when:** real data is needed while developing. The smallest change is an opt-in `PET_DB` environment variable that overrides the path for one run.

### Schema changes through `PRAGMA user_version`
- **Why:** a migration is one `if version < N` block, with no migration library (`lib.rs:70`).
- **Gave up:** no down-migrations.

### Times stored as UTC text, days grouped by local time
- **Why:** UTC is unambiguous in storage; the streak is about the user's own day (`lib.rs:159-190`).
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
- **Why:** nothing else is needed yet. "Removed at the source" is recorded on the source link (`removed_at`), and an imported task the user deleted is marked with `hidden_at`; neither is a status, so a hidden or removed task still knows whether it was done.
- **Revisit when:** the user wants to drop a typed task without deleting it.

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
- **Revisit when:** merging typed tasks with imported ones. Canvas is now imported and uses the registrar's names ("STATISTC 310", "COMPSCI 230"), which do not match what a student types ("STAT 310", "CS 230"), so a course-name match needs an alias list or a looser rule.

### Quick add gives a date, never a time
- **Why:** typed lines like "Fri" carry no time. Only `due_date` is filled; `due_at` waits for imports.
- **Revisit when:** typed times ("Fri 5pm") are wanted.

## Phase 2 · Task list

### Today and All are a filter in the panel over one list
- **Why:** `list_tasks` already returns every open task plus the ones checked off today, sorted by due date with undated ones last. Today is that list minus tasks due later, filtered in `src/panel/main.ts`, so there is no second query and no stored state.
- **Gave up:** an undated task counts as Today, since there is no other way to say "added for today"; `planned_for` is still never set, and nothing is built at 7 am. "Due soon compared with the estimate" is not part of Today yet.
- **Revisit when:** Today gets crowded. Imports have arrived: they always carry a due date, so they stay out of Today until their day, but All is now a flat list of every upcoming assignment (54 on the first real import) with no grouping by course or week.

### Delete is one click, with no confirmation or undo
- **Why:** the least code, and retyping a quick-add line is cheap.
- **Gave up:** an imported task deleted by mistake cannot be brought back from the panel (see "Deleting an imported task hides it").

### The panel reloads the list after its own changes, and on a `tasks` event after a sync
- **Why:** only the panel shows tasks, so after an add, check or delete it just reloads. A feed sync runs in the background, so Rust sends a `tasks` event when one finishes and the panel reloads tasks, today's events and the feed status.
- **Gave up:** nothing reloads at midnight on its own; the list and schedule can show yesterday's view until the next change or sync.
- **Revisit when:** the pet window shows tasks or reminders.

## Phase 2 · Imports

The costs of these choices are collected, in order of risk, in `KNOWN_ISSUES.md`.

### Both sources are private ICS addresses; Google OAuth is deferred
- **Why:** Canvas ("Calendar Feed") and Google Calendar ("Secret address in iCal format") both hand out a private HTTPS address that returns an ICS file. One fetcher and one parser cover both, with no Google Cloud project, consent screen or token refresh.
- **Gave up:** the spec lists Google as "OAuth, read-only". A school Google Workspace can turn the secret address off, and then this route does not work at all. The Google path has only been run against a made-up feed (`backend/tests/google.ics`), never a real calendar.
- **Revisit when:** a real account has no secret address, or the app is shared with other people. (It was revisited on 2026-10-05: see the next entry.)

### Google sign-in sits beside the address and the file; the token is in the Keychain through `security`
- **Why:** the school account has no secret address and an uploaded file goes stale. A throwaway script showed the school allows read-only access. An account is a third way to connect the Google source, so the fetcher, `sync_feed` and the schedule list are unchanged; events are keyed `iCalUID@start` on both routes, so switching updates them in place. The refresh token goes in the macOS Keychain (PR-4) by way of the `security` tool that ships with macOS, written on its standard input so it never shows in `ps`. Full design: `google_calendar_oauth.md`.
- **Rejected:** a Rust keychain library; the app is unsigned, so macOS would ask for the login password after every rebuild and reinstall. The `setting` table; plain text on disk. Reading from the macOS Calendar app; the user wanted Google directly. An OAuth library; the standard library covers the flow.
- **Gave up:** any program running as the user can ask `security` for the token. macOS only. The Cloud project and its client file are set up by hand, and the client file is copied in once per app. Primary calendar only.
- **Revisit when:** the app is signed (use a keychain library then), the app is shared with other people (the client must ship with it), or another calendar is wanted.

### A calendar with no address can be uploaded as an .ics file
- **Why:** the first real Google account had no secret address to copy. Google Calendar can still export an .ics file, so each feed in Settings has an "Upload .ics" button. The panel reads the file and Rust stores its text in the `setting` table in place of an address (`save_feed_file`, `lib.rs:571`). Every sync hands that text to the fetcher again, which parses it with no download, so repeating events keep rolling into the 30-day window.
- **Rejected:** parsing the file once at upload and keeping only the events; repeats past the first 30 days would never appear.
- **Gave up:** a file is a snapshot. New or changed events do not arrive until the user exports and uploads again, and nothing warns that a file is old. The whole file, with every event's description and guest list, sits in `pet.db` as plain text, up to 5 MB. Google's export is a .zip, which the user has to unzip first.
- **Revisit when:** uploading by hand gets tiresome; that is the point to build OAuth. (Built 2026-10-05.)

### Feeds are parsed with `icalendar` and `recurring-ical-events`
- **Why:** Canvas alone needed about 60 lines of standard library, but a Google feed has time zones, repeating events and left-out dates, which are not worth writing by hand. One library-based parser for both sources replaced the hand-written one (`backend/feeds.py`).
- **Gave up:** the backend is no longer free of `pip install` (`backend/requirements.txt`, three pinned packages plus what they pull in). Only the fetcher imports them; the offline sidecar is still standard library only.

### The fetcher is a one-shot process that Rust starts for each sync
- **Why:** PR-6 wants network access in its own process. `backend/fetcher.py` reads the addresses and any uploaded files on stdin, downloads and parses, prints one JSON reply and exits (`run_fetcher`, `lib.rs:618`). Nothing stays running, and the addresses never appear in `ps`.
- **Gave up:** each sync pays a Python start-up (about 2 s with the download). There is no overall time limit on the process, only a 20 s limit per network operation.
- **Revisit when:** a sync is seen to hang; kill the child after a fixed time.

### The fetcher carries its own root certificates (`certifi`)
- **Why:** python.org's macOS build has no root certificates until the user runs an extra installer, and every HTTPS download failed. Passing `certifi`'s list to `urlopen` works on any Python. Certificate checking is never turned off.
- **Gave up:** a third pinned package, and its list goes stale if it is never updated.

### Feed addresses are kept as plain text in the `setting` table (replaced 2026-10-05, see the next entry)
- **Why:** the plan was the OS keychain. In a dev build the binary's signature changes on every rebuild, so macOS would ask for the login password on each one, and a release build cannot run Python yet, so the keychain path could not be tested at all. The address sits next to data that is already unencrypted (`feed_urls`, `lib.rs:536`).
- **Gave up:** PR-4. Anyone who can read `pet.db` can read the calendars. The panel never shows an address back, and error messages are written so they cannot contain one.
- **Revisit when:** before any release build, together with SQLCipher: move the addresses to the keychain with the `keyring` crate.

### Feed addresses are in the macOS Keychain, through the `security` tool
- **Why:** PR-4. The Google sign-in had already shown that the `security` tool reaches the Keychain with no password prompt on rebuild, which was the reason the keychain was put off. The same helper now takes a name for each secret (`feed_url_canvas`, `feed_url_google`, `google_refresh_token`) under the app's identifier, so the dev and everyday apps keep their own. The `setting` table holds the marker `keychain` in place of the address, so "is a feed saved" and "a source holds one of address, file or account" still read from the database alone. The secret is written as hex on the tool's standard input: an address has `?` and `&` in it, and hex cannot break the command line or show in `ps`.
- **Moving old addresses:** at launch, an address still in the table is written to the Keychain, read back, and only then replaced by the marker. If the write fails it stays in the table and still works.
- **Rejected:** the `keyring` crate; the app is unsigned, so macOS would ask for the login password after every rebuild. Putting an uploaded .ics file there too; it is up to 5 MB and is data, not a credential, so it waits for database encryption.
- **Gave up:** any program running as the user can ask `security` for an address. macOS only. The Rust test for feed addresses now writes to the real login Keychain under a test name. A database copied to another Mac arrives without its addresses and asks for them to be pasted again.
- **Revisit when:** the app is signed, or the Windows build starts.

### A Canvas all-day item is a date, a timed one is a UTC time
- **Why:** Canvas sends most assignments as a bare date and a few with a time. They map straight onto `due_date` and `due_at`.
- **Gave up:** an assignment due at 11:59 pm shows only its day.

### Only Canvas assignments become tasks; everything else is a calendar event
- **Why:** a Canvas item whose id starts with `event-assignment-` has a due date and can be finished. Other Canvas items and everything from Google are things that happen at a time, which is what `calendar_event` is for.
- **Gave up:** a Google event such as "Essay due" is never a task. A Canvas address pasted into the Google box imports its assignments as events.

### A re-sync updates the task in place and never touches its status
- **Why:** the spec's sync rules. `sync_feed` (`lib.rs:462`) finds the task through `(source, source_uid)`, overwrites the title, course and due that the source owns, and leaves `status` and `done_at` alone. It runs in one transaction and takes `now` as an argument so the test can fake time.
- **Gave up:** an edit the user makes to an imported task's title or due date would be overwritten by the next sync (there is no edit yet).

### A task already past due when first seen is not imported
- **Why:** the feed does not say whether an assignment was handed in. The first real import had 102 assignments, 48 of them already past; importing those would have filled Today with "was due" rows for finished work. A task imported while still upcoming stays on the list after its date passes.
- **Gave up:** an assignment that was genuinely missed before the feed was connected never appears.
- **Revisit when:** a missed assignment from before the connection goes unnoticed. (A Canvas API token would have settled this; that route is closed, see the next entry.)

### No Canvas API token; the pet asks whether work was submitted
- **Why:** decided 2026-10-05. The school does not let students create a Canvas API token, and the calendar feed carries no submission status, so the pet cannot know an assignment was handed in. Checking off by hand already works and a sync keeps the done status. To make that nearly free, the pet asks once at the first break after a Canvas task's due time ("Did you submit it?") and when a focus block tied to a task ends ("Finished it?"). Both ride on roadmap tasks that were already planned (reminders at breaks, tying a session to a task), so nothing new is built for this alone.
- **Rejected:** scraping Canvas with the browser's login; fragile, and it means holding a session that can do anything the student can. Hiding a task a few days after its due date; it could hide work that was never done.
- **Gave up:** real submission status. A task the user never answers about stays overdue until checked off or deleted.
- **Revisit when:** the school allows tokens, or overdue Canvas tasks pile up in Today.

### A task missing from the feed is flagged only if it is not yet past
- **Why:** the spec wants "removed from Canvas" instead of vanishing, so `removed_at` is set on the source link, the panel shows "removed from Canvas", and the flag clears if the item returns. A feed may also drop old items on its own, and those must not look removed.
- **Gave up:** an assignment deleted at the source after its due date is never flagged. A valid but empty feed (the wrong account's address, a new term) flags every upcoming task until a good sync.

### A failed download stores nothing
- **Why:** treating a failure as an empty feed would flag every upcoming task as removed. The fetcher reports an error per source, a page that is not a calendar is an error rather than an empty list, and `store_feed` (`lib.rs:642`) only calls `sync_feed` on a good reply.
- **Gave up:** nothing.

### Deleting an imported task hides it
- **Why:** deleting the row drops its source link, and the next sync would import it again as new. `remove_task` (`lib.rs:431`) sets `hidden_at` on an imported task and deletes a typed one (schema version 3).
- **Gave up:** there is no way to bring a hidden task back from the panel, and hidden rows are never cleaned up.
- **Revisit when:** someone hides a task by mistake.

### Calendar events are replaced on every sync, and kept for a window of days
- **Why:** events have no state of their own, so deleting a source's events and inserting the feed's copy handles edits and removals with no extra column. Repeating events have no end, so only yesterday to 30 days ahead is expanded and stored.
- **Gave up:** an event's row id changes on every sync and its `uid` is the feed's id plus its start time, so nothing can point at an event row. The laptop must sync at least once a month to have events at all.
- **Revisit when:** reminders or sessions need to refer to an event.

### Sync runs at launch, every half hour, when an address is saved, and on "Sync now"
- **Why:** one background thread and one lock (`Syncing`) so two syncs never overlap (`sync_feeds`, `lib.rs:662`). The database is not locked while the download runs.
- **Gave up:** the half hour does not count time asleep, so the first sync after opening the lid can be up to 30 minutes away. A failure is shown only in Settings.
- **Revisit when:** stale tasks are noticed after waking; sync on wake.

### Today's schedule is a plain list above the tasks
- **Why:** the spec asks for events "as a timeline without checkboxes". A list of times and titles, all-day first, is that with no layout code; the section is hidden when there are no events (`events_today`, `lib.rs:727`).
- **Gave up:** no hour grid, no "now" marker, and a timed event that began yesterday and runs past midnight is not shown.

## Phase 2 · Look

### The pet is two static SVG pictures, swapped by a CSS class
- **Why:** the design is a 32 x 32 pixel grid worked out by a React component. The app has no React (see "Vite with plain TypeScript"), so the grid was rendered once into `src/pet/closed.svg` and `src/pet/open.svg`. `#pet.watching` picks the open-eyed one, so `src/pet/main.ts` did not change and P-2 works as before.
- **Rejected:** adding React to draw one picture; drawing the grid in TypeScript at startup (more code that runs on every launch for a picture that never changes).
- **Gave up:** the pictures cannot be edited as shapes and colors. A new look means generating both files again from the design source, which is not in this repository (`KNOWN_ISSUES.md`, item 30).
- **Detail:** the open-eyed picture is also loaded as a second background layer under the closed one, so the first swap does not flash. This only works while both pictures have the same outline.

### One picture for the pet, the panel and the app icon
- **Why:** the panel's view switcher shows `src/pet/closed.svg` itself, so there is no second copy to keep in step. The app icon needs a background tile to sit in the Dock, so it has its own source, `src-tauri/app-icon.svg`, and `npm run tauri icon src-tauri/app-icon.svg` makes the icon files from it.
- **Gave up:** only the five icon files named in the Tauri configs are kept. The command also writes Windows Store, iOS and Android icons; those were deleted.
- **Revisit when:** the Windows build starts (run the command again and keep what that platform needs).

## Phase 2 · Windows

### The pet shows on every Space
- **Why:** a pet that stays behind on one desktop stops being a companion as soon as the user swipes to another. `visibleOnAllWorkspaces` on the pet window in both `tauri.conf.json` and `tauri.daily.conf.json` is one line of config.
- **Gave up:** the pet cannot be parked on one desktop; the hide shortcut is the only way to get it out of view.

### The panel moves to the active Space, through AppKit directly
- **Why:** the panel is hidden, not closed, so macOS remembered the Space it was last on and a click on the pet threw the user back there. Setting `NSWindowCollectionBehavior::MoveToActiveSpace` once at startup makes it appear where the user is (`move_to_active_space`, `lib.rs:756`). Tauri has no config key for this behavior, so it is set on the `NSWindow` itself.
- **Rejected:** `visibleOnAllWorkspaces` on the panel too; a normal window that follows across every desktop is in the way.
- **Gave up:** a macOS-only dependency (`objc2-app-kit`) and one `unsafe` pointer cast, both behind `cfg(target_os = "macos")`. Other platforms get no equivalent.
- **Revisit when:** Tauri exposes the collection behavior, or the Windows build starts.
