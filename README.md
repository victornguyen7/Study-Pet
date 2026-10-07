# Study Pet

Tauri desktop pet. See `docs/spec.md`; Phase 1 (pet, timer, streak) is built and Phase 2 (daily list) is under way: typed tasks, Today and All views, and imports from Canvas and a calendar feed or .ics file.

- `src/pet`, `index.html`: transparent always-on-top pet window, shown on every macOS Space. Drag to move, click to open the panel, `Alt+Shift+P` to hide/show. The pet is a pixel slime in two pictures, `closed.svg` and `open.svg`; its eyes are open only during a focus block.
- `src-tauri/app-icon.svg`: source of the app icon. `npm run tauri icon src-tauri/app-icon.svg` rewrites `src-tauri/icons` (delete the Windows Store, iOS and Android files it adds; no config uses them).
- `src/panel`, `panel.html`: panel window with three views: Today (session controls, a streak section with a pixel fire that lights once today counts, the task list with quick add and a Today/All filter, today's focus blocks; a session can be started for a task, and a typed task can be merged with its imported copy), Wrap up (day summary, last 7 days) and Settings (presets, streak goal, sounds on or off, feed addresses for imports, and "Sync now", which says whether the sync worked). Today also lists the day's calendar events. On macOS it opens on the Space you are on. It is 1000 × 700, set in both Tauri configs.
- `src-tauri/src/`: Rust core, in nine files: `lib.rs` (`run()` and the tests), `db.rs`, `sidecar.rs`, `session.rs` (timer, sounds, streak), `tasks.rs`, `feeds.rs`, `keychain.rs`, `windows.rs` and `browser.rs` (pages from the Chrome extension). It holds the SQLite store (`pet.db` in the project root for dev builds, in the OS app data dir for release builds), commands, Python sidecar. Secrets (feed addresses, the Google token) go in the macOS Keychain, not the database.
- `backend/timer.py`: session timer state machine (focus, break, call back). Rust ticks it once a second, stores finished focus blocks, and broadcasts the state to both windows as a `timer` event. It also plays a macOS system sound when a break starts and when the pet calls you back.
- `backend/streaks.py`: streak rules (daily threshold, 2 freezes a month, milestones). Rust feeds it study minutes per day from the `session` table.
- `backend/tasks.py`: quick-add parser. Turns a typed line such as "STAT 310 HW3 Fri 2h" into a title, course, due date and estimate; Rust stores the result in the `task` table.
- `backend/fetcher.py`: the task fetcher, the only process on the network. Rust starts it at launch, every half hour and from Settings; it downloads the saved Canvas and Google Calendar feed addresses, parses any uploaded .ics file, reads a signed-in Google account's primary calendar, and exits. It also runs the Google sign-in in the browser; the refresh token is kept in the macOS Keychain (`docs/google_calendar_oauth.md`).
- `backend/feeds.py`: ICS feed parser. Canvas assignments become tasks; everything else becomes calendar events. Rust stores the result with `sync_feed`.
- `backend/main.py`: Python sidecar, JSON lines over stdin/stdout. Add a function to `METHODS` and a Rust command that reaches it through `sidecar_call`.

## Run

    npm install
    python3 -m venv backend/.venv   # once
    backend/.venv/bin/python -m pip install -r backend/requirements.txt   # once; only the task fetcher needs these
    npm run tauri dev

To record pages from Chrome, start the app once (it tells Chrome where to find it), then open `chrome://extensions`, turn on "Developer mode", choose "Load unpacked" and pick the `extension/` folder. The pet asks once per site and remembers the answer; a yes or no covers every page on that site. To see every site you have answered, click the Study Pet button in Chrome's toolbar: each one has a Yes/No switch and a Forget button, which makes the pet ask again. After pulling a new version of `extension/`, press its reload button on `chrome://extensions`.

## Check

    npm run build                   # typecheck + bundle the UI
    cd src-tauri && cargo test      # DB schema + Rust <-> Python round trip
    backend/.venv/bin/python -m unittest backend.tests.test_timer backend.tests.test_streaks backend.tests.test_tasks backend.tests.test_feeds backend.tests.test_fetcher backend.tests.test_modes   # timer, streak, quick add, feed parsing and the fetcher (from the project root)

To run the whole import against a real feed, into a throwaway database:

    cd src-tauri && CANVAS_ICS_URL="https://..." cargo test -- --ignored --nocapture

What is unfinished or fragile is listed in `docs/KNOWN_ISSUES.md`.
