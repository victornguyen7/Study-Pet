# Known issues and risks

What is wrong, unfinished or fragile today, and what it will cost if it is left. Highest cost first within each group.
The reasoning behind each choice is in `DECISIONS.md`. Last reviewed 2026-10-06, with every item checked against the code after the Phase 2 features were finished.

## What to do first

Reviewed 2026-10-06. Item numbers refer to the lists below.

**Before the Phase 2 gate week**
- Items 7, 9, 15 and 17 were fixed on 2026-10-06. The stale-sync line in Today and the midnight reload have not been seen in the real window.
- Items 41, 45 and 46: click through the task start button, "Finished it?", the reminder bubble and "Did you submit it?" in the real window.
- Item 1: delete the three oldest backups after a few good days (the last reinstall was 2026-10-07) and take the address out of `.env`.

**Before Phase 3 (window monitor, drift ladder)**
- Item 23 was done on 2026-10-07: `lib.rs` is split into modules. Run the app once to confirm nothing changed.
- Item 22 was fixed on 2026-10-07.
- Items 40, 42, 43 and 46: the drift ladder needs a bubble that can be clicked; fix these with it.
- No content security policy, and the fixed hide shortcut (see "Older items"): the browser extension and the look-away shortcut make both matter.

**Before Phase 4 (capture)**
- Item 2 and the unencrypted database: both end with the SQLCipher task, which comes first in that phase.
- Item 13: bundling Python gets harder with each package added.
- Item 34: keep backing up before every reinstall.

**Before Phase 6 (more sources)**
- Items 20 and 21: event rows are not stable, and disconnecting leaves tasks behind.
- Items 37 and 38: the gate for that phase is duplicate merging.
- Items 10 and 19: edits to imported tasks, and hidden tasks.
- Item 14: move the Google Cloud project to "In production".

**When it starts to annoy:** items 5, 16 (raise the `certifi` pin in early 2027), 26, 36, 39 and 44. The rest cost little.

## How much of the import has been checked

| Part | Checked how |
| --- | --- |
| Parser, fetcher, storage rules, file upload path | Automated tests (53 Python, 16 Rust, counting the later features) |
| Real Canvas feed, end to end into a throwaway database | Run by hand on 2026-10-04: 102 assignments in the feed, 54 imported, 48 skipped as already past, a second sync added nothing |
| App starts and migrates the database | Launched once; no errors |
| Imports section in Settings (address, "Upload .ics", disconnect), schedule list, "removed from Canvas" note | **Not clicked through.** They compile and the commands behind them are tested, but nobody has looked at them on screen |
| Google Calendar by sign-in | Run by hand in the dev app on 2026-10-05 against the real school account: 193 events, and it synced again after a restart with no new sign-in |
| Google Calendar by secret address | **Not run against a real calendar.** The first real account had no secret address (item 14); only `backend/tests/google.ics`, a made-up feed |

## Security and privacy

1. **Old plain-text copies of the Canvas feed address remain.** New saves go to the macOS Keychain and the dev app's address was moved there on 2026-10-05, and the everyday app's on its 2026-10-05 reinstall, but the address is still readable in two places: the everyday app's three oldest backups (`pet.db.bak-2026-10-04`, `-05` and `-05-1623`) and `.env` (used by one ignored test). All of these were still there on 2026-10-07. Any program running as the user can also ask the `security` tool for a Keychain item; the Keychain protects the file on disk, not against other software in the session.
   *Cost if left:* a leaked backup or a shared laptop leaks every course and assignment.
   *Fix:* delete the old backups once the reinstalled app has run well for a few days, and remove the address from `.env` when the test is not needed. To make an exposed address useless, reset the feed in Canvas (or Google) and paste the new one. Tying Keychain items to this app alone needs a signed app and a keychain library.
2. **An uploaded calendar file is stored whole, as plain text.** Its text is kept in the `setting` table so it can be parsed again on each sync. That includes every event's description, location and guest e-mail addresses, for the calendar's whole history, not just the 30 days the app shows.
   *Cost if left:* the same exposure as item 1, with more in it. Disconnecting the feed deletes the stored file.
3. **Fixed 2026-10-07: a fetcher crash showed the last line of Python's error in Settings, and failed every feed.** A crash while reading one source is now that source's own error and shows only the kind of error ("the task fetcher failed: KeyError"); the other feeds still sync. What is left: a crash before any source is read, such as a missing package, still shows Python's last line.

## Things that will show wrong data

4. **The feed does not say whether an assignment was handed in.** Imported tasks are never checked off automatically, and an assignment already past when the feed is first connected is never imported.
   *Cost if left:* finished Canvas work sits as "was due" until checked by hand; a missed assignment from before the connection is invisible.
   *Fix:* none from Canvas: the school does not give students an API token, so that route was dropped on 2026-10-05. The pet will ask "Did you submit it?" after the due time and "Finished it?" when a tied session ends; both are part of the reminders and session tasks in the roadmap and are not built yet. Assignments already past at first connection stay invisible.
5. **A valid but empty or wrong feed flags every upcoming task as "removed".** This happens with another account's address or at the start of a term. It clears on the next good sync, and nothing is deleted.
6. **A typed task and its Canvas copy both appear, until merged.** Fixed in part on 2026-10-05: the panel suggests a merge when the due day and course number match, and the user confirms. What the rule misses is in items 37 and 38.
7. **Fixed 2026-10-06: day comparisons in the sync used the UTC date.** "Already past" and "still upcoming" are now judged on the laptop's day.
8. **A time with no zone is read in the laptop's zone.** Correct for most feeds, wrong if the calendar was made for another zone.
9. **Fixed 2026-10-06: tasks and events did not reload at midnight.** The panel reloads them on the first once-a-minute streak update of a new day. Not yet watched across a real midnight.
10. **A sync overwrites the title, course and due date of an imported task.** Harmless now; the day tasks become editable, edits to imported ones will be silently undone.
11. **Whether Canvas limits its feed is unknown.** The real feed held everything from 8 September to 15 December. If Canvas caps the count or the date range, later assignments appear only once they enter the feed.
12. **An uploaded file never updates.** It is a snapshot from the moment of export. Events added or moved afterwards do not appear until a new file is uploaded, and nothing in the panel says how old the file is: the status line shows when it was last read, not when it was exported.
    *Cost if left:* the schedule quietly drifts from the real calendar. A class that moves rooms or times is shown at the old time.
    *Fix:* store the upload date and show it; or OAuth.

## Things that will cost time later

13. **A release build cannot import, or run at all.** `run_fetcher` and the sidecar both start Python from the source tree. Packaging now has to bundle Python plus `icalendar`, `recurring-ical-events`, `certifi` and their dependencies, which is more work than the standard-library backend was.
14. **Google by secret address may not work for a school account.** Workspace admins can turn it off, and the first real account had none. The stopgap is uploading an exported .ics file, which goes stale (item 12). The lasting fix, signing in with Google, is built (`google_calendar_oauth.md`). What it still costs: the refresh token expires every 7 days while the Cloud project is in "Testing", so the app asks for a sign-in weekly until the project is moved to "In production"; the token in the Keychain can be read by any program running as the user; and the everyday app got its own `google_client.json` on 2026-10-05, but a sign-in from it is not recorded.
15. **Fixed 2026-10-06: the fetcher had no overall time limit.** It now sets an alarm on itself: 2 minutes for a sync, 6 for a sign-in. Settings shows "the task fetcher took too long and was stopped". The alarm is set after the request is read, so a fetcher that hangs while starting is not covered.
16. **Pinned packages go stale.** `certifi`'s certificate list is fixed at the pinned version. When it gets old enough, downloads fail, and the panel only says "could not reach that address" (the message is kept vague so it cannot leak the address).
    *Fix:* raise the pins in `backend/requirements.txt` a few times a year. If a sync fails for no clear reason, run the opt-in test in the root `README.md` first.
17. **Fixed 2026-10-06: sync failures were only visible in Settings.** Today shows a line above the task list when a connected feed has gone a day without a good sync, or has never synced. The line is checked after each sync, so it can be up to half an hour late. Not yet seen in the real window.
18. **The half-hour timer does not count time asleep.** After the lid opens, the next sync can be up to 30 minutes away. "Sync now" is the workaround.
19. **Hidden tasks cannot be brought back and are never cleaned up.** Deleting an imported task sets `hidden_at`; there is no screen for hidden tasks. Done and hidden rows stay in the database for good.
20. **Event rows are not stable.** Every sync deletes and re-inserts a source's events, and only yesterday to 30 days ahead is kept. Anything that needs to point at an event (a reminder, a session) has to store the feed's own id and start time, or the storage has to change first.
21. **Disconnecting a feed leaves its tasks behind.** They stay on the list with their source link, stop updating, and are never flagged as removed.
22. **Fixed 2026-10-07: a panic while a lock was held broke the app until restart.** Every lock was taken with `.lock().unwrap()` (38 places), so one panic made every later call panic too, and Phase 3 added three kinds of thread that take the database lock. The database and sync locks now go through `lock` in `db.rs`, which carries on after a panic; the sidecar's lock does the same and starts a fresh Python process, so a focus block in progress at that moment is lost. A panic still ends the thread it happens in: one in the once-a-second timer thread stops the timer until restart.
23. **Fixed 2026-10-07: `lib.rs` was one file of 1,810 lines.** It is now eight files (`DECISIONS.md`, "The Rust core is eight files"), and `DECISIONS.md` no longer quotes line numbers. What is left: the tests are still one module of about 490 lines in `lib.rs`, and the split app has been compiled and tested but not yet run.
24. **No automated test covers the panel.** `npm run build` only type-checks it.
25. **Google's export is a .zip with one .ics per calendar.** The upload takes a single .ics of at most 5 MB, so the user has to unzip it and pick one calendar; a second calendar would need a second source. A large file is also sent to the fetcher and parsed again every half hour.
26. **All is one flat list.** The first real import put 54 upcoming assignments in it, sorted by due date, with no grouping by course or week and no search.
37. **A merge cannot be undone, and a suggestion cannot be dismissed.** Merging deletes the typed task after moving its estimate and focus time to the imported one; its title is gone. A wrong suggestion stays on the row until one of the two tasks is done, deleted or re-dated. *Fix when it annoys:* a "not the same" button that remembers the pair.
38. **The merge rule is narrow on purpose.** It needs a due date and a course on the typed task, the same due day, and the same course number. Two imported matches on one day give no suggestion at all. A typed task with no course is never matched, and two courses that share a number ("STAT 310", "MARKETNG 310") can be suggested wrongly.
39. **Hiding a course hides tasks only.** Calendar events that come from the Canvas feed carry no course and are still listed. The feed status still counts hidden assignments.
40. **"Finished it?" appears only in the panel.** If the panel is closed during the break, nothing asks. The pet's bubble is plain text.
41. **The session, merge and course features were checked in a headless browser with made-up data, not clicked in the real window.** The Rust and Python behind them are tested, including the task id's trip through the Python timer. Starting a block from a task, the "Finished it?" button and the course checkboxes have not been pressed in the app.
42. **A reminder can be missed.** It is said once a day per task. If the pet's bubble is up while nobody is at the laptop and a focus block then starts, the bubble is cleared and that task is not mentioned again until tomorrow. The "start soon" mark on its row stays.
43. **An urgent task waits for the focus block to end.** The spec lets a task due within 2 hours break through; that exception is not built, so a 50-minute block can use up most of the time a task had left.
44. **Reminders assume an hour for a task with no estimate.** Most Canvas imports have none, so a 4-hour assignment is mentioned only 2 hours before it is due. Merging it with a typed task that has an estimate, or working on it, is the only way to change that; there is no way to edit an estimate.
45. **The reminder bubble has not been seen.** The rule and the text are tested, and in the dev app the once-a-minute check marked a throwaway task as reminded, so the event is sent. Nobody has watched the bubble appear, grow the pet window and clear on a click.
46. **"Did you submit it?" can only be answered in the panel, and only with yes.** The pet's bubble asks once and is plain text; the answer is the button on the task's row, or its checkbox. There is no "no": an unanswered task stays on the list as "was due" with the button until it is checked off or deleted. If a due-soon reminder and a question come up in the same minute, the reminder shows first and the question replaces it a minute later. Not seen in the real window.

## The new look and the two builds

30. **The pet pictures are generated, and their source is not in the repository.** `src/pet/closed.svg` and `src/pet/open.svg` were rendered once from a React design file (`App.tsx`, kept outside the project). They are plain lists of colored rectangles and are not practical to edit by hand.
    *Cost if left:* any change to the look needs that file and a small script to render it again. If the file is lost, the pictures have to be redrawn.
    *Fix:* keep the design file and a render script under `scripts/` the first time the look changes.
31. **Four things depend on the exact drawing.** The pet is sized 120 x 80 px for a 24 x 16 grid (`src/pet/pet.css`); the pet window is that same size, written again in both Tauri configs and as `SMALL` in `src/pet/main.ts`; the milestone stars are placed at `top: 52px` to land on the dark patch; and the two pictures must share one outline, because the open-eyed one sits under the closed one.
    *Cost if left:* a redrawn slime with another size or outline shows blurred pixels, stars in the wrong place, or the open eyes peeking out from behind the closed picture.
32. **The wake-up splash has not been looked at in the real window.** The rest of the new pet has: the roadmap records the focus block (eyes open), the call back (hop) and a milestone star as seen, and the reinstalled app is in daily use.
33. **The installed everyday app is a mix of frozen and live code.** The copy in `/Applications` is dated 2026-10-07; the commit it was built from is not recorded (the build before it was `c90c6af` on `phase_2`, 2026-10-05). Its window pages, icon and Rust code are frozen at build time, but it starts Python from this source tree (item 13), so every edit under `backend/` reaches it at once.
    *Cost if left:* a change to what the sidecar or the fetcher sends or expects, made on a dev branch, breaks the everyday app until it is rebuilt. Moving or renaming the project folder, or deleting `backend/.venv`, stops it starting the timer at all.
    *Fix:* rebuild and reinstall after any backend change; lasting fix is bundling Python (item 13).
34. **Rebuilding the everyday app runs new migrations on real data.** Its database is in the OS app data dir and there are no down-migrations. The dev database in the project root is separate (`DECISIONS.md`, "Dev database in the project root"), so `npm run tauri dev` never touches real data; the risk starts when a new build is installed in `/Applications`.
    *Cost if left:* a faulty migration reaches the only copy of the real history.
    *What to do:* back up before every reinstall, in this order.

    1. Quit Study Pet (the everyday app, not the dev one). A focus block in progress is lost when it quits, so finish or stop it first.
    2. Copy the database to a dated backup beside it:

           cp ~/Library/Application\ Support/com.hiennguyen.pet/pet.db ~/Library/Application\ Support/com.hiennguyen.pet/pet.db.bak-$(date +%F)

    3. Check the copy is there and is not empty:

           ls -l ~/Library/Application\ Support/com.hiennguyen.pet/

    4. Build the everyday app from the project root:

           npm run tauri build -- --config src-tauri/tauri.daily.conf.json --bundles app

    5. Replace the installed app: drag `src-tauri/target/release/bundle/macos/Study Pet.app` into `/Applications` and choose Replace.
    6. Open it and check that today's focus blocks, the streak and the task list are still there.

    *To go back:* quit the app, then copy the backup over the real file. Use the date of the backup you want. If the new build added a migration, also reinstall the older app, because an older database with a newer app is migrated again on the next launch.

           cp ~/Library/Application\ Support/com.hiennguyen.pet/pet.db.bak-2026-10-04 ~/Library/Application\ Support/com.hiennguyen.pet/pet.db

    Five backups exist: `pet.db.bak-2026-10-04`, `-05`, `-05-1623`, `-06` and `-07`, the last from just before the latest reinstall. Old backups can be deleted once the app has run well for a few days; the first three also hold the old plain-text feed address (item 1).
35. **Only macOS icon needs are covered.** The Windows Store images and `icon.png` were deleted because no config named them. A Windows Store package, or any config that lists `icons/icon.png`, will fail to build until `npm run tauri icon src-tauri/app-icon.svg` is run again.
36. **The version number lives in four places.** `package.json`, `Cargo.toml`, `tauri.conf.json` and `tauri.daily.conf.json` all say 0.2.0 now, matching the changelog, but each has to be changed by hand at the next bump.
    *Fix:* remove `"version"` from both Tauri configs so they fall back to `Cargo.toml`, leaving two places.
47. **The break sounds and the streak section have not been tried in the running app.** Added 2026-10-06. The code type-checks, the Rust tests pass and both sound files play from a terminal, but nobody has yet heard a sound at a real break or looked at the new section in the panel, in light or dark mode.
    *Fix:* run `npm run tauri dev`, start a 1-minute focus with a 1-minute break, and check both sounds, the "Play a sound" checkbox and the fire going from grey to lit. Look at the panel at its new size at the same time (item 49).
48. **The sounds are macOS only and have no test.** `play_sound` runs `afplay` on a file under `/System/Library/Sounds`. On another platform every break logs "sound failed" and stays silent. The rule for when to ring lives in the tick thread, where no test reaches it.
    *Fix:* at the Windows build, put the call behind `cfg(target_os = "macos")` and choose that platform's way to play a sound.
49. **The panel is 1000 × 700 but its layout is still one narrow column.** Changed 2026-10-06 from 420 × 620. Nothing in `src/panel/panel.css` limits the content width, so every section stretches across the window. Not yet looked at in the running app.
    *Cost if left:* long, thin rows with the text at one end and the buttons at the other.
    *Fix:* give `main` a `max-width` and centre it, or lay Today out in two columns.
50. **Seen 2026-10-07: the "Sync now" result line works in the running app.** Pressing the button showed the sync working. The failure wording (red, naming the feed) has not been seen, since no feed failed.
51. **Fixed 2026-10-07: the result line could go stale.** It was only written when the button was pressed, so a later automatic sync that failed left an old "✓ Synced at…" showing. The line is now redrawn from the feeds' own status on every `tasks` event (`refreshSyncResult` in `src/panel/main.ts`). Not yet watched across an automatic sync.
52. **Fixed 2026-10-07: any error from the Python sidecar threw away the running focus block.** `sidecar_call` dropped the process on every error, including ordinary ones such as quick-add's "a task needs a title", and the timer lives in that process. It is now dropped only when the pipe or the reply is broken. The unused `backend_call` command, which let the window call any sidecar method, was removed with it.

## Setup traps

27. **`backend/.venv/bin/pip` is broken on the first developer's laptop.** The virtual environment was created when the project lived in another folder, so the `pip` script points at a Python that no longer exists. `backend/.venv/bin/python -m pip ...` works, and the docs use that form. Recreating the environment fixes it.
28. **A fresh checkout needs a `pip install` now.** Without it the timer and streak still work, and Settings shows "the task fetcher failed: ModuleNotFoundError" for each feed.
29. **python.org's Python has no root certificates.** The fetcher brings its own through `certifi`, so nothing needs doing; this is recorded because it is the reason for that dependency.

## Older items, still open

These predate the import and are described in `DECISIONS.md`: the database is not encrypted (PR-4), a session in progress is lost if the app quits, there is no content security policy, and the hide shortcut is fixed to Option-Shift-P.
