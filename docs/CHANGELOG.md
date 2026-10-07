# Changelog

## Unreleased · Phase 3

**Added**
- Chrome extension in `extension/` (load it unpacked): on every page opened it asks "can I look at this page?", and for an allowed page in front sends its address and title to the app every 5 seconds. No answer is remembered: a yes covers that page in that tab until the tab goes somewhere else. Its popup says whether the app is connected.
- The app receives pages by native messaging (`browser.rs`): a Unix socket `ext.sock` and a host script `chrome-host.sh` beside `pet.db`, and a host manifest written into Chrome's folder at launch. Nothing is recorded while the app is closed or during a break.
- New table `activity_span` (start, end, site, address, title) and command `site_time`. "Sites today" in the Wrap up view lists time per site.

## 0.2.0 · Phase 2, in progress

**Added**
- Tables for the daily list: `task`, `task_source_link` and `calendar_event`, plus a nullable `task_id` on `session` (schema version 2).
- Quick add: a typed line such as "STAT 310 HW3 Fri 2h" becomes a task with a course, due date and estimate (`backend/tasks.py`, with tests).
- Tasks section in the panel's Today view: add a task, check it off, delete it. Overdue tasks are marked.
- Commands `add_task`, `list_tasks`, `set_task_done` and `delete_task`.
- `.env` files are ignored by git.
- Import from Canvas: assignments in the Canvas calendar feed become tasks with a course and a due date or time. A re-sync updates a task in place, keeps its done status, and marks one that left the feed as "removed from Canvas".
- Import from Google Calendar by its secret iCal address: events only, with repeating events and time zones handled. Not yet tried against a real calendar.
- Upload an .ics file in place of a feed address, for a calendar that has none. The file is read again on every sync so repeating events keep appearing, but it only changes when a new one is uploaded.
- Today's schedule in the panel: today's calendar events listed above the tasks, all-day ones first.
- Imports section in Settings: paste a feed address for Canvas or Google Calendar, upload a file, disconnect it, or sync now. Each feed shows when it last synced or why it failed.
- Sync at launch and every half hour, by a separate fetcher process that is the only one on the network (`backend/fetcher.py`).
- Commands `feeds`, `set_feed`, `set_feed_file`, `sync_now` and `list_events`, and a `tasks` event after each sync.
- `backend/requirements.txt`: the fetcher needs `icalendar`, `recurring-ical-events` and `certifi`.
- `docs/KNOWN_ISSUES.md`: what is unfinished or fragile, and what it costs.
- Today and All filter on the panel's task list. Today shows what is due today or overdue, undated tasks and the ones checked off today; All shows every open task.
- The panel is larger: 420 × 620, up from 360 × 520.
- The pet shows on every macOS Space (`visibleOnAllWorkspaces`), so it follows you between desktops.
- The panel opens on the Space you are on instead of pulling you back to the one it was last on (`move_to_active_space`, macOS only). New macOS-only dependency: `objc2-app-kit`.
- The pet is a pixel slime: two pictures, eyes closed and eyes open (`src/pet/closed.svg`, `src/pet/open.svg`). It replaces the teal blob drawn in CSS. The eyes still open only during a focus block (P-2), and milestone stars now sit on the dark patch below the eyes.
- The slime is shown at the left of the panel's view switcher.
- Sounds: a macOS system sound when a break starts (Glass) and each time the pet calls you back (Ping), played by Rust with `afplay`. "Play a sound" in Settings turns them off (`sounds` command, `sounds` setting).
- The panel is the size of a small browser window: 1000 × 700, up from 420 × 620.
- "Sync now" in Settings says how it went: "Syncing…" while it runs, with the button disabled, then a green "✓ Synced at 3:42 PM", or in red which feed did not sync. A later automatic sync redraws the line, so it stays true.
- Streak section in the panel's Today view, below the tank: a pixel fire (`src/panel/fire.svg`) that is grey until today counts, the day count, milestone stars, a bar for today's minutes against the daily goal, and the freezes left. It replaces the line under "Today's focus blocks".
- App icon: the slime on a dark rounded tile, in place of the stock Tauri logo. Its source is `src-tauri/app-icon.svg`.
- The installed everyday app ("Study Pet" in `/Applications`) was rebuilt from `tauri.daily.conf.json` and reinstalled on 2026-10-04, so it has the new pet and Dock icon. No migration ran; its database was already at schema version 3.
- Wake-up splash on the pet at launch, from the logo design: the slime naps, blinks awake, then closes its eyes again (`src/pet/pet.css`, CSS only). The eyes are open for about 1.3 s with nothing being captured, a short exception to P-2. Skipped when the system asks for reduced motion. The design's full-screen backdrop and "Study Pet" wordmark are left out; the pet window is a small transparent overlay.
- Sign in with Google: a "Sign in with Google" button on the Google row in Settings reads the primary calendar through Google's API, so it updates without a new upload. It sits beside the secret address and the uploaded file; a source uses one of the three. The fetcher does the sign-in in the browser (`connect` in `backend/fetcher.py`), the refresh token is kept in the macOS Keychain, and no secret goes in the database. Needs a `google_client.json` beside `pet.db`; see `google_calendar_oauth.md`. New command `connect_google`. Covered by tests against a faked Google, and tried in the dev app against the real school account on 2026-10-05: 193 events, and it synced again after a restart with no new sign-in. The everyday app does not have it yet.
- A session can be for one task (S-2): each open task has a start button, and the start form has a "Working on" list. The running view names the task, each task shows its focus time against its estimate ("45 min of 2 h"), and in the break after such a block the panel asks "Finished it?" with a button that checks the task off. A block for a task that is deleted meanwhile is still saved, untied.
- Task reminders: once a minute, outside a focus block, the pet looks for open tasks whose time left is under twice the work left. Work left is the estimate less the focus time spent, with 1 hour assumed for a task with no estimate and never less than 15 minutes. The pet's bubble says, for the nearest one, "“Essay” is due in 2 h 30 min and needs about 2 h. Start now?" and counts the others; it stays until the pet is clicked or a focus block starts. Each task is mentioned once a day (`task.reminded_at`), and its row in the panel shows "start soon" for as long as the rule holds. Not built: holding reminders for a break, and the morning brief.
- "Did you submit it?": when an imported task's due time has passed, the pet asks once in its bubble ("“Lab 2” was due. Did you submit it?", with a count if there are more), under the same once-a-minute, never-during-focus check as reminders. The task's row shows a "Did you submit it? Yes" button that checks it off. A typed task is never asked about.
- Merge suggestion: a typed task that is due the same day as an imported one, in a course with the same number ("STAT 310" and "STATISTC 310"), shows "Same as ... from Canvas? Merge". Merging keeps the imported task with its source link and gives it the typed task's estimate and focus time. Nothing merges without a click, and there is no undo. New command `merge_task`.
- Courses in Settings: a checkbox for each course with open imported assignments. Unticking one hides its imported tasks; nothing is deleted. New commands `list_courses` and `set_course`.

**Changed**
- The Rust core is split into modules with no change in behavior: `lib.rs` (1,810 lines) became `lib.rs`, `db.rs`, `sidecar.rs`, `session.rs`, `tasks.rs`, `feeds.rs`, `keychain.rs` and `windows.rs`. Commands are listed by path in `generate_handler!`.
- Feed addresses are kept in the macOS Keychain, not in `pet.db` (PR-4). The `setting` table holds only a marker. An address saved by an older build is moved at launch; if the move fails it stays where it is and still works. A marker with no Keychain item shows "the address is missing from the Keychain; paste it again" for that feed, and the other feeds still sync. Tried in the dev app on 2026-10-05: the real Canvas address moved and the next sync brought the same 54 assignments.
- The version number is 0.2.0 in `package.json`, `Cargo.toml` and both Tauri configs (and the two lock files).
- `backend/feeds.py` reads event times with `event.decoded("DTSTART")` instead of `event["DTSTART"].dt`. Same values; it only stops the editor's type checker from flagging the line.
- Deleting an imported task hides it, so the next sync does not bring it back (`task.hidden_at`, schema version 3). A typed task is still deleted.
- Setup now includes a `pip install` step.

**Fixed**
- The sync judges "already past" and "still upcoming" on the laptop's day, not the UTC day. In the evening in a US time zone, an assignment due earlier that day was skipped on first import.
- A stuck download can no longer block every later sync: the fetcher ends itself after 2 minutes (6 for a sign-in), and Settings shows "the task fetcher took too long and was stopped".
- Today shows a line above the task list when a connected feed has gone a day without a good sync or has never synced. A failed sync now keeps the time of the last good one (`ok_at` in the feed's status).
- Tasks and today's schedule reload when the day changes while the panel stays open.

**Removed**
- Icon files the Tauri config never referenced: the Windows Store images (`Square*Logo.png`, `StoreLogo.png`) and `icon.png`. All were the stock Tauri logo.

**Not yet**
- A Today list built each morning with `planned_for`, choosing which Google calendars to read, quick commands (optional), reminders held for a break and the morning brief.

## 0.1.0 · Phase 1, unreleased

**Added**
- Pet window: transparent, always on top, drag to move, click to open the panel, `Alt+Shift+P` to hide or show.
- Panel with three views: Today, Wrap up and Settings.
- Session timer: focus, break, and a call back when the break ends.
- Presets for focus and break lengths (25/5 and 50/10 to start).
- Streak: adjustable daily goal, 2 freezes a month, milestones at 7, 30 and 100 days.
- Wrap-up view with today's total and the last 7 days.
- SQLite store for presets, focus blocks and settings.
- Python sidecar for the timer and streak rules, with tests.
- Docs: spec, architecture, roadmap and decisions.
