# Study Pet

A desktop pet that keeps a student focused. It runs locally on a laptop: a small always-on-top pet that follows you across desktops, a count of your study and work time that needs no Start button, a forgiving streak, a daily to-do list that imports from Canvas and a calendar, and gentle nudges when you drift after you have settled into work.

Phases 1 to 3 are in daily use: the pet and streak; the daily list with its imports and reminders; and the focus keeper, which notes which app is in front and, through a Chrome extension, which allowed page, and labels the time Study, Work, Relax or Private. Phase 4, built on 2026-10-11, is the recap in the Wrap up view: a heatmap of study and work over the last 18 weeks and, for today or any day clicked in it, the hours per mode as one bar and one total per app and site, with sites shown by name. On 2026-10-13 the focus timer was removed: the pet counts Study and Work time by itself, and the focus keeper turns itself on after 5 minutes of either. The knowledge graph and timelapse of the first spec were dropped on 2026-10-11. macOS only.

## Setup

You need Node.js, Rust and Python 3.

    npm install
    python3 -m venv backend/.venv
    backend/.venv/bin/python -m pip install -r backend/requirements.txt
    npm run tauri dev

The packages are only for the task fetcher, which downloads calendar feeds; the streak, keeper and mode logic uses the standard library alone.

To import, open the panel's Settings and paste a feed address under Imports. Canvas: Calendar, then "Calendar Feed". Google Calendar: the calendar's settings, then "Secret address in iCal format". If your calendar has no such address, export it (Settings, "Import & export", Export), unzip it, and use "Upload .ics"; upload again whenever the calendar changes. Signing in with Google is the third way; it needs a client file ([google_calendar_oauth.md](google_calendar_oauth.md)).

To record pages from Chrome, start the app once, then load the `extension/` folder unpacked from `chrome://extensions`. The steps are in the root `README.md`.

## Check

    npm run build
    cd src-tauri && cargo test
    backend/.venv/bin/python -m unittest backend.tests.test_streaks backend.tests.test_tasks backend.tests.test_feeds backend.tests.test_fetcher backend.tests.test_modes backend.tests.test_drift

Run the last command from the project root.

## How it fits together

- `src/`: the UI, in plain TypeScript (the pet and the panel).
- `src-tauri/`: the Rust core. It owns the windows, the database and the once-a-minute loop.
- `backend/`: the Python logic for the streak, the focus keeper, task quick add and the mode labels, plus the task fetcher that downloads feeds.
- `extension/`: the Chrome extension that tells the app which allowed page is in front.

## Docs

- [spec.md](spec.md): what the product does.
- [ROADMAP.md](ROADMAP.md): what is built, what is next, and what was dropped.
- [ARCHITECTURE.md](ARCHITECTURE.md): how the parts connect.
- [DECISIONS.md](DECISIONS.md): why it is built this way.
- [KNOWN_ISSUES.md](KNOWN_ISSUES.md): what is unfinished or fragile, and what it costs.
- [CHANGELOG.md](CHANGELOG.md): what has been built.
- [google_calendar_oauth.md](google_calendar_oauth.md): the design for signing in with Google, the spike behind it and the Cloud console steps (built 2026-10-05).
