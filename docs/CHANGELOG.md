# Changelog

## Unreleased · Phase 3

**Changed**
- The Chrome extension (now 0.2.0) asks once per site and remembers the answer, where it asked on every page. Answers are kept in Chrome's storage by site; the popup lists every answered site, where each can be switched between yes and no or forgotten.

- Private windows in ordinary apps (PR-1): time is recorded as "Private" while a password field has the keyboard in any app, while a Chrome incognito window is in front, and while the page in front shows a password field. The extension (0.3.0) now tells the app with every sample whether Chrome's front window is private, and never sends such a page.

**Added**
- Every headline in Wrap up has a Refresh button (it was only on "Sites today"). Each one reloads the whole day, so the lists stay in step.
- Sound from any app, not only Chrome, puts the pet's headphones on, and the time with sound on is logged in the new table `audio_span` (start and end only, PR-11) and shown in Wrap up. New command `audio_time`.
- Settings counts down to the next Google sign-in ("Sign in again in 3 days"): the sign-in ends after 7 days while the Cloud project is in "Testing". The time of the sign-in is kept in `feed_account_google`; a sign-in made before this has no time and shows no countdown.
- Time balance in the Wrap up view: seconds per mode today, and each labeled stretch of a minute or more with its times, label and the apps and sites in it.
- Drift ladder step 4, off by default: after 10 minutes of drift on a site the app asks the extension (0.5.0) to cover its pages for 5 minutes. "Focus keeper" in Settings holds the switch and the pet's voice: Gentle, Sassy or Roast. New command `keeper`.
- Look-away shortcut (PR-3): Option-Shift-L pauses all noting for 15 minutes; pressing it again ends that.
- The pet offers to start a focus block after 5 minutes of Study with no session running (S-7), at most once in half an hour.
- Pet states: Relaxing, Headphones, Nudging and Happy (`pet.css`, `headphones.svg`). The "drift" event now also carries the mode of what is in front and whether audio is on; the extension (0.4.0) reports that some tab is playing, never which.
- Background-audio rules: music apps and sites have no mode of their own, so time in them takes the label around it and the pet never asks about them. In front during a focus block they get 60 seconds of grace before the drift ladder starts.
- Drift ladder, steps 1 to 3 (`backend/drift.py`): during a focus block the pet glances after 30 seconds on a Relax app or site, speaks after 2 minutes, and sits in the middle of the screen after 5, then welcomes the user back. YouTube's home page, feeds and Shorts count as drift; a video does not. The monitor sends each sample to the sidecar and the pet hears changes as a "drift" event.
- Chrome extension in `extension/` (load it unpacked): on every page opened it asks "can I look at this page?", and for an allowed page in front sends its address and title to the app every 5 seconds. No answer is remembered: a yes covers that page in that tab until the tab goes somewhere else. Its popup says whether the app is connected.
- The app receives pages by native messaging (`browser.rs`): a Unix socket `ext.sock` and a host script `chrome-host.sh` beside `pet.db`, and a host manifest written into Chrome's folder at launch. Nothing is recorded while the app is closed or during a break.
- New table `activity_span` (start, end, site, address, title) and command `site_time`. "Sites today" in the Wrap up view lists time per site.
- Front-app monitor (`monitor.rs`, macOS): every 5 seconds the name and bundle ID of the app in front go into the new table `app_span` (start, end, app, bundle ID). No window title is read. Nothing is recorded during a break, for the pet itself or at the lock screen. New command `app_time`; "Apps today" in the Wrap up view lists time per app.
- "Permissions" in Settings (PR-10): what the Accessibility permission is for, whether it is allowed, and a button that makes macOS ask. New command `accessibility`. Nothing uses the permission yet.
- Privacy gate for apps (PR-1): password managers, chat and mail apps on a built-in list are recorded as "Private", with no name or bundle ID.
- Mode classifier (`backend/modes.py`): labels today's app and page spans Study, Work, Relax or Private, from the user's declaration, a learned rule, the app or site category, then a running focus block; a stretch under 3 minutes keeps the label it interrupts. New table `mode_declaration`, commands `modes_today` and `declared_mode`, and a "Right now I'm" picker in the Today view.
- Ask once and remember (PR-2): the pet asks what an unrecognized app or site is for, once, and the Today view lists the open questions with Study, Work, Relax and Private buttons. New table `app_rule` and command `learned`; "What the pet has learned" in Settings lists the answers. A Private answer renames the app's past spans to "Private", deletes the site's recorded pages, and keeps new ones out.
- The everyday app in `/Applications` was rebuilt and reinstalled on 2026-10-07 with everything here; its database gained the four new tables.

**Fixed**
- Quitting the app during a focus block stores the block with the minutes it ran, as Stop does. It used to be lost.
- With the extension in two Chrome profiles, the profile behind no longer makes Chrome count as Private, which shut the pet's eyes during a focus block and hid that time from "Apps today" and the drift ladder. Each copy's answer is kept apart (`SAID` in `browser.rs`); the same went for Chrome's sound and the page in front.
- Time with nobody at the keyboard is no longer counted: after 5 minutes without input the front app and Chrome's pages are not noted, unless Chrome is in front playing sound.
- A declared mode ends at midnight, and "Relax" ends when a focus block starts. It used to last until changed, so "Relax" left on in the evening labeled the next morning.
- A global shortcut that another program holds no longer stops the app from starting.
- An error from the Python sidecar no longer throws away the running focus block. Any error, such as quick add's "a task needs a title", used to replace the process, and the timer lives in it. The process is now replaced only when the pipe or the reply is broken.
- A crash while reading one feed no longer fails every feed. It is that source's own error and shows only the kind of error ("the task fetcher failed: KeyError"), never Python's own message.
- A panic while a lock is held no longer makes every later call panic. The database and sync locks go through `lock` in `db.rs`; the sidecar's lock starts a fresh Python process.
- The dev and the everyday app no longer take the Chrome extension from each other. Each has its own native-messaging host, named after its identifier, and the extension (0.2.1) tries the everyday app first, then the dev build.
- The everyday app keeps its Accessibility permission across reinstalls: it is signed with a self-signed certificate, "Study Pet Local Signing", kept in the login keychain and named in `tauri.daily.conf.json`.

**Removed**
- The `backend_call` command, which let a window call any sidecar method with any arguments. Nothing used it.

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
