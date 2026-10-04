# Known issues and risks

What is wrong, unfinished or fragile today, and what it will cost if it is left. Highest cost first within each group.
The reasoning behind each choice is in `DECISIONS.md`. Last reviewed 2026-10-04, when the Canvas and calendar import and the .ics upload were built.

## How much of the import has been checked

| Part | Checked how |
| --- | --- |
| Parser, fetcher, storage rules, file upload path | Automated tests (17 Python, 3 Rust) |
| Real Canvas feed, end to end into a throwaway database | Run by hand on 2026-10-04: 102 assignments in the feed, 54 imported, 48 skipped as already past, a second sync added nothing |
| App starts and migrates the database | Launched once; no errors |
| Imports section in Settings (address, "Upload .ics", disconnect), schedule list, "removed from Canvas" note | **Not clicked through.** They compile and the commands behind them are tested, but nobody has looked at them on screen |
| Google Calendar, by address or by uploaded file | **Never run against a real calendar or a real export.** Only against `backend/tests/google.ics`, a made-up feed |

## Security and privacy

1. **Feed addresses are stored as plain text.** They are in the `setting` table of `pet.db`, and the Canvas one is also in `.env`. Anyone or anything that can read those files can read the calendars. This breaks PR-4 (keychain for tokens).
   *Cost if left:* a leaked backup or a shared laptop leaks every course and appointment. Google's secret address can be reset in calendar settings; do that if a file is ever exposed.
   *Fix:* the `keyring` crate, at the same time as SQLCipher, before any release build.
2. **An uploaded calendar file is stored whole, as plain text.** Its text is kept in the `setting` table so it can be parsed again on each sync. That includes every event's description, location and guest e-mail addresses, for the calendar's whole history, not just the 30 days the app shows.
   *Cost if left:* the same exposure as item 1, with more in it. Disconnecting the feed deletes the stored file.
3. **A fetcher crash shows the last line of Python's error in Settings.** The normal error messages are written so they cannot contain an address, and a test checks this. A crash outside those paths is passed through unfiltered.
   *Cost if left:* low; no known crash prints an address.

## Things that will show wrong data

4. **The feed does not say whether an assignment was handed in.** Imported tasks are never checked off automatically, and an assignment already past when the feed is first connected is never imported.
   *Cost if left:* finished Canvas work sits as "was due" until checked by hand; a missed assignment from before the connection is invisible.
   *Fix:* a Canvas API token (the spec lists it as later).
5. **A valid but empty or wrong feed flags every upcoming task as "removed".** This happens with another account's address or at the start of a term. It clears on the next good sync, and nothing is deleted.
6. **A typed task and its Canvas copy both appear.** There is no merge on course, title and due date yet (roadmap item). Matching will not be simple: Canvas calls a course "STATISTC 310" where a typed task says "STAT 310".
   *Cost if left:* the list is only trustworthy if tasks are never typed for a course that is on Canvas.
7. **Day comparisons in the sync use the UTC date.** "Already past" and "still upcoming" are judged on the UTC day, so within a few hours of midnight an item due today can be treated as tomorrow's or yesterday's.
   *Cost if left:* an assignment due today could be skipped on first import in the evening (US time zones).
8. **A time with no zone is read in the laptop's zone.** Correct for most feeds, wrong if the calendar was made for another zone.
9. **Nothing reloads at midnight.** Today's schedule and the Today filter are worked out when the panel last redrew, so yesterday's view can stay until the next sync or change (at most half an hour when a feed is connected).
10. **A sync overwrites the title, course and due date of an imported task.** Harmless now; the day tasks become editable, edits to imported ones will be silently undone.
11. **Whether Canvas limits its feed is unknown.** The real feed held everything from 8 September to 15 December. If Canvas caps the count or the date range, later assignments appear only once they enter the feed.
12. **An uploaded file never updates.** It is a snapshot from the moment of export. Events added or moved afterwards do not appear until a new file is uploaded, and nothing in the panel says how old the file is: the status line shows when it was last read, not when it was exported.
    *Cost if left:* the schedule quietly drifts from the real calendar. A class that moves rooms or times is shown at the old time.
    *Fix:* store the upload date and show it; or OAuth.

## Things that will cost time later

13. **A release build cannot import, or run at all.** `run_fetcher` and the sidecar both start Python from the source tree. Packaging now has to bundle Python plus `icalendar`, `recurring-ical-events`, `certifi` and their dependencies, which is more work than the standard-library backend was.
14. **Google by secret address may not work for a school account.** Workspace admins can turn it off, and the first real account had none. The stopgap is uploading an exported .ics file, which goes stale (item 12). The lasting fix is OAuth: a Google Cloud project, a consent screen, a localhost redirect, token storage, and a refresh token that expires every 7 days until the project is published.
15. **The fetcher has no overall time limit.** Each network operation stops after 20 s, but a server that sends data very slowly could hold the process open. Because one lock covers a whole sync, every later sync, "Sync now" and saving an address would wait behind it until the app restarts.
    *Fix:* kill the child process after a fixed time in `run_fetcher`.
16. **Pinned packages go stale.** `certifi`'s certificate list is fixed at the pinned version. When it gets old enough, downloads fail, and the panel only says "could not reach that address" (the message is kept vague so it cannot leak the address).
    *Fix:* raise the pins in `backend/requirements.txt` a few times a year. If a sync fails for no clear reason, run the opt-in test in the root `README.md` first.
17. **Sync failures are only visible in Settings.** A feed that has been failing for days looks the same in Today as one that is working.
    *Fix:* show a line in Today when the last good sync is more than a day old.
18. **The half-hour timer does not count time asleep.** After the lid opens, the next sync can be up to 30 minutes away. "Sync now" is the workaround.
19. **Hidden tasks cannot be brought back and are never cleaned up.** Deleting an imported task sets `hidden_at`; there is no screen for hidden tasks. Done and hidden rows stay in the database for good.
20. **Event rows are not stable.** Every sync deletes and re-inserts a source's events, and only yesterday to 30 days ahead is kept. Anything that needs to point at an event (a reminder, a session) has to store the feed's own id and start time, or the storage has to change first.
21. **Disconnecting a feed leaves its tasks behind.** They stay on the list with their source link, stop updating, and are never flagged as removed.
22. **A panic while a lock is held breaks the app until restart.** The database and the sync lock are taken with `.lock().unwrap()`; if any code panics while holding one, every later call panics too. This was already true of the database lock; the sync lock adds a second one.
23. **`lib.rs` is about 1,140 lines in one file.** `DECISIONS.md` set "hard to navigate" as the point to split it; the import code added about 370 lines. Line numbers quoted in `DECISIONS.md` go out of date with every change to that file.
24. **No automated test covers the panel.** `npm run build` only type-checks it.
25. **Google's export is a .zip with one .ics per calendar.** The upload takes a single .ics of at most 5 MB, so the user has to unzip it and pick one calendar; a second calendar would need a second source. A large file is also sent to the fetcher and parsed again every half hour.
26. **All is one flat list.** The first real import put 54 upcoming assignments in it, sorted by due date, with no grouping by course or week and no search.

## Setup traps

27. **`backend/.venv/bin/pip` is broken on the first developer's laptop.** The virtual environment was created when the project lived in another folder, so the `pip` script points at a Python that no longer exists. `backend/.venv/bin/python -m pip ...` works, and the docs use that form. Recreating the environment fixes it.
28. **A fresh checkout needs a `pip install` now.** Without it the timer and streak still work, and Settings shows "the task fetcher failed: ModuleNotFoundError" for each feed.
29. **python.org's Python has no root certificates.** The fetcher brings its own through `certifi`, so nothing needs doing; this is recorded because it is the reason for that dependency.

## Older items, still open

These predate the import and are described in `DECISIONS.md`: the database is not encrypted (PR-4), a session in progress is lost if the app quits, there is no content security policy, and the hide shortcut is fixed to Option-Shift-P.
