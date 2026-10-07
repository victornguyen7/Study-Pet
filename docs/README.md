# Study Pet

A desktop pet that keeps a student focused. It runs locally on a laptop: a small always-on-top pet that follows you across desktops, a focus timer with breaks and a sound when each one starts and ends, and a forgiving streak, and a daily to-do list that imports from Canvas and a calendar. Focus nudges are being built now; later phases add an end-of-day knowledge graph.

Phase 1 (pet, timer, streak) is built, and so are the Phase 2 features (daily list): you can add, check off and delete your own tasks, import assignments from Canvas and events from a calendar, and the pet reminds you when a task is due soon. Phase 3 (focus keeper and modes) is in progress: the app notes which app is in front and, through a Chrome extension, which allowed page, and labels the time Study, Work, Relax or Private. Nothing nudges yet. macOS first.

## Setup

You need Node.js, Rust and Python 3.

    npm install
    python3 -m venv backend/.venv
    backend/.venv/bin/python -m pip install -r backend/requirements.txt
    npm run tauri dev

The packages are only for the task fetcher, which downloads calendar feeds; the timer and streak logic uses the standard library alone.

To import, open the panel's Settings and paste a feed address under Imports. Canvas: Calendar, then "Calendar Feed". Google Calendar: the calendar's settings, then "Secret address in iCal format". If your calendar has no such address, export it (Settings, "Import & export", Export), unzip it, and use "Upload .ics"; upload again whenever the calendar changes. Signing in with Google is the third way; it needs a client file ([google_calendar_oauth.md](google_calendar_oauth.md)).

To record pages from Chrome, start the app once, then load the `extension/` folder unpacked from `chrome://extensions`. The steps are in the root `README.md`.

## Check

    npm run build
    cd src-tauri && cargo test
    backend/.venv/bin/python -m unittest backend.tests.test_timer backend.tests.test_streaks backend.tests.test_tasks backend.tests.test_feeds backend.tests.test_fetcher backend.tests.test_modes

Run the last command from the project root.

## How it fits together

- `src/`: the UI, in plain TypeScript (the pet and the panel).
- `src-tauri/`: the Rust core. It owns the windows, the database and the timer tick.
- `backend/`: the Python logic for the timer, the streak, task quick add and the mode labels, plus the task fetcher that downloads feeds.
- `extension/`: the Chrome extension that tells the app which allowed page is in front.

## Docs

- [spec.md](spec.md): what the product does.
- [ROADMAP.md](ROADMAP.md): the phases and their tasks.
- [ARCHITECTURE.md](ARCHITECTURE.md): how the parts connect.
- [DECISIONS.md](DECISIONS.md): why it is built this way.
- [KNOWN_ISSUES.md](KNOWN_ISSUES.md): what is unfinished or fragile, and what it costs.
- [CHANGELOG.md](CHANGELOG.md): what has been built.
- [google_calendar_oauth.md](google_calendar_oauth.md): the design for signing in with Google, the spike behind it and the Cloud console steps (built 2026-10-05).
