# Study Pet

Tauri desktop pet. See `docs/spec.md`; Phase 1 (pet, timer, streak) is built and Phase 2 (daily list) is under way: typed tasks, Today and All views, and imports from Canvas and a calendar feed or .ics file.

- `src/pet`, `index.html`: transparent always-on-top pet window, shown on every macOS Space. Drag to move, click to open the panel, `Alt+Shift+P` to hide/show. The pet is a pixel slime in two pictures, `closed.svg` and `open.svg`; its eyes are open only during a focus block.
- `src-tauri/app-icon.svg`: source of the app icon. `npm run tauri icon src-tauri/app-icon.svg` rewrites `src-tauri/icons` (delete the Windows Store, iOS and Android files it adds; no config uses them).
- `src/panel`, `panel.html`: panel window with three views: Today (session controls, the task list with quick add and a Today/All filter, today's focus blocks), Wrap up (day summary, last 7 days) and Settings (presets, streak goal, feed addresses for imports). Today also lists the day's calendar events. On macOS it opens on the Space you are on.
- `src-tauri/src/lib.rs`: Rust core: SQLite store (`pet.db` in the project root for dev builds, in the OS app data dir for release builds), commands, Python sidecar.
- `backend/timer.py`: session timer state machine (focus, break, call back). Rust ticks it once a second, stores finished focus blocks, and broadcasts the state to both windows as a `timer` event.
- `backend/streaks.py`: streak rules (daily threshold, 2 freezes a month, milestones). Rust feeds it study minutes per day from the `session` table.
- `backend/tasks.py`: quick-add parser. Turns a typed line such as "STAT 310 HW3 Fri 2h" into a title, course, due date and estimate; Rust stores the result in the `task` table.
- `backend/fetcher.py`: the task fetcher, the only process on the network. Rust starts it at launch, every half hour and from Settings; it downloads the saved Canvas and Google Calendar feed addresses, parses any uploaded .ics file, and exits.
- `backend/feeds.py`: ICS feed parser. Canvas assignments become tasks; everything else becomes calendar events. Rust stores the result with `sync_feed`.
- `backend/main.py`: Python sidecar, JSON lines over stdin/stdout. Add a function to `METHODS`, call it from the UI with `invoke("backend_call", { method, params })`.

## Run

    npm install
    python3 -m venv backend/.venv   # once
    backend/.venv/bin/python -m pip install -r backend/requirements.txt   # once; only the task fetcher needs these
    npm run tauri dev

## Install the everyday app

The everyday app ("Study Pet" in `/Applications`) keeps real data in `~/Library/Application Support/com.hiennguyen.pet/pet.db`, apart from the dev database. Quit it first, then from the project root:

    cp ~/Library/Application\ Support/com.hiennguyen.pet/pet.db ~/Library/Application\ Support/com.hiennguyen.pet/pet.db.bak-$(date +%F)   # skip on a first install
    npm run tauri build -- --config src-tauri/tauri.daily.conf.json --bundles app
    rm -rf "/Applications/Study Pet.app" && ditto "src-tauri/target/release/bundle/macos/Study Pet.app" "/Applications/Study Pet.app"
    open "/Applications/Study Pet.app"

Do it again after any change the everyday app should get. How to go back to a backup is in `docs/KNOWN_ISSUES.md`, item 34.

## Check

    npm run build                   # typecheck + bundle the UI
    cd src-tauri && cargo test      # DB schema + Rust <-> Python round trip
    backend/.venv/bin/python -m unittest backend.tests.test_timer backend.tests.test_streaks backend.tests.test_tasks backend.tests.test_feeds backend.tests.test_fetcher   # timer, streak, quick add, feed parsing and the fetcher (from the project root)

To run the whole import against a real feed, into a throwaway database:

    cd src-tauri && CANVAS_ICS_URL="https://..." cargo test -- --ignored --nocapture

What is unfinished or fragile is listed in `docs/KNOWN_ISSUES.md`.
