# Study Pet

Tauri desktop pet. See `docs/spec.md`; Phase 1 (pet, streak) and the Phase 2 features (daily list, imports from Canvas and a calendar, reminders) are built. Phase 3 (focus keeper and modes) and Phase 4 (the recap) are built too: the app notes which app and which allowed web page is in front, and labels the time Study, Work, Relax or Private. Since 2026-10-13 there is no focus timer: Study and Work time counts by itself.

- `src/pet`, `index.html`: transparent always-on-top pet window, shown on every macOS Space. Drag to move, click to open the panel, `Alt+Shift+P` to hide/show, `Alt+Shift+L` to make it look away for 15 minutes. The pet is a pixel slime in two pictures, `closed.svg` and `open.svg`; its eyes are open only during a focus block.
- `src-tauri/app-icon.svg`: source of the app icon. `npm run tauri icon src-tauri/app-icon.svg` rewrites `src-tauri/icons` (delete the Windows Store, iOS and Android files it adds; no config uses them).
- `src/panel`, `panel.html`: panel window with three views: Today (a streak section with a pixel fire that lights once today counts, the task list with quick add and a Today/All filter; a typed task can be merged with its imported copy), Wrap up (a heatmap of study and work over the last 18 weeks, blue or orange by which had more, and, for today or any day clicked in it, the hours per mode as one bar and one total per app and site, with sites shown by name) and Settings (streak goal, the break length, sounds on or off, feed addresses for imports, "Sync now", which says whether the sync worked, the courses to show, what the pet has learned about apps and sites, in a section that folds shut, and the Accessibility permission). Today also lists the day's calendar events, has a "Right now I'm" picker for the mode, and lists the apps and sites the pet does not recognize under "What are these for?". On macOS it opens on the Space you are on. It is 1000 × 700, set in both Tauri configs.
- `src-tauri/src/`: Rust core, in eleven files: `lib.rs` (`run()` and the tests), `db.rs`, `sidecar.rs`, `session.rs` (sounds, streak, minutes per day), `tasks.rs`, `feeds.rs`, `keychain.rs`, `windows.rs`, `browser.rs` (pages from the Chrome extension), `monitor.rs` (the app in front, the privacy gate, the Accessibility permission) and `modes.rs` (the declared mode, the learned rules, and today's spans for the classifier). It holds the SQLite store (`pet.db` in the project root for dev builds, in the OS app data dir for release builds), commands, Python sidecar. Secrets (feed addresses, the Google token) go in the macOS Keychain, not the database.
- `backend/drift.py`: the focus keeper. `Keeper` turns it on after 5 minutes of Study or Work and off after a break, runs the drift ladder, and says when to suggest a break. Rust asks it with every 5-second sample of the front app (`watch`).
- `backend/streaks.py`: streak rules (daily threshold, 2 freezes a month, milestones). Rust feeds it minutes of Study and Work per day: `day_total` for finished days, today labeled live, and the `session` table for days before 2026-10-13.
- `backend/tasks.py`: quick-add parser. Turns a typed line such as "STAT 310 HW3 Fri 2h" into a title, course, due date and estimate; Rust stores the result in the `task` table.
- `backend/fetcher.py`: the task fetcher, the only process on the network. Rust starts it at launch, every half hour and from Settings; it downloads the saved Canvas and Google Calendar feed addresses, parses any uploaded .ics file, reads a signed-in Google account's primary calendar, and exits. It also runs the Google sign-in in the browser; the refresh token is kept in the macOS Keychain (`docs/google_calendar_oauth.md`).
- `backend/feeds.py`: ICS feed parser. Canvas assignments become tasks; everything else becomes calendar events. Rust stores the result with `sync_feed`.
- `backend/modes.py`: mode classifier. Labels today's app and page spans Study, Work, Relax or Private, smooths stretches under 3 minutes, and lists the apps and sites to ask the user about. Rust sends it the spans, the declarations and the learned rules.
- `extension/`: Chrome extension, loaded unpacked. It asks once per site and sends the address and title of an allowed page in front every 5 seconds, by native messaging.
- `backend/main.py`: Python sidecar, JSON lines over stdin/stdout. Add a function to `METHODS` and a Rust command that reaches it through `sidecar_call`.

## Run

    npm install
    python3 -m venv backend/.venv   # once
    backend/.venv/bin/python -m pip install -r backend/requirements.txt   # once; only the task fetcher needs these
    npm run tauri dev

To record pages from Chrome, start the app once (it tells Chrome where to find it), then open `chrome://extensions`, turn on "Developer mode", choose "Load unpacked" and pick the `extension/` folder. The pet asks once per site and remembers the answer; a yes or no covers every page on that site. To see every site you have answered, click the Study Pet button in Chrome's toolbar: each one has a Yes/No switch and a Forget button, which makes the pet ask again. After pulling a new version of `extension/`, press its reload button on `chrome://extensions`. If the everyday app and the dev build are both running, the pages go to the everyday app.

## Check

    npm run build                   # typecheck + bundle the UI
    cd src-tauri && cargo test      # DB schema + Rust <-> Python round trip
    backend/.venv/bin/python -m unittest backend.tests.test_streaks backend.tests.test_tasks backend.tests.test_feeds backend.tests.test_fetcher backend.tests.test_modes backend.tests.test_drift   # timer, streak, quick add, feed parsing, the fetcher and the mode rules (from the project root)

To run the whole import against a real feed, into a throwaway database:

    cd src-tauri && CANVAS_ICS_URL="https://..." cargo test -- --ignored --nocapture

What is unfinished or fragile is listed in `docs/KNOWN_ISSUES.md`.
