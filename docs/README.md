# Study Pet

A desktop pet that keeps a student focused. It runs locally on a laptop: a small always-on-top pet that follows you across desktops, a focus timer with breaks, and a forgiving streak. A daily to-do list is being built now; later phases add focus nudges and an end-of-day knowledge graph.

Phase 1 (pet, timer, streak) is built. Phase 2 (daily list) is in progress: you can add, check off and delete your own tasks, and import assignments from Canvas and events from a calendar feed. macOS first.

## Setup

You need Node.js, Rust and Python 3.

    npm install
    python3 -m venv backend/.venv
    backend/.venv/bin/python -m pip install -r backend/requirements.txt
    npm run tauri dev

The packages are only for the task fetcher, which downloads calendar feeds; the timer and streak logic uses the standard library alone.

To import, open the panel's Settings and paste a feed address under Imports. Canvas: Calendar, then "Calendar Feed". Google Calendar: the calendar's settings, then "Secret address in iCal format". If your calendar has no such address, export it (Settings, "Import & export", Export), unzip it, and use "Upload .ics"; upload again whenever the calendar changes.

## Install the everyday app

`npm run tauri dev` runs a dev copy with its own database. The everyday app, "Study Pet" in `/Applications`, keeps your real history in `~/Library/Application Support/com.hiennguyen.pet/pet.db`. To install or update it, quit it, then from the project root:

    cp ~/Library/Application\ Support/com.hiennguyen.pet/pet.db ~/Library/Application\ Support/com.hiennguyen.pet/pet.db.bak-$(date +%F)
    npm run tauri build -- --config src-tauri/tauri.daily.conf.json --bundles app
    rm -rf "/Applications/Study Pet.app" && ditto "src-tauri/target/release/bundle/macos/Study Pet.app" "/Applications/Study Pet.app"
    open "/Applications/Study Pet.app"

The first line backs up the database; skip it on a first install. Do this again after any change the everyday app should get. To go back to a backup, see [KNOWN_ISSUES.md](KNOWN_ISSUES.md), item 34.

## Check

    npm run build
    cd src-tauri && cargo test
    backend/.venv/bin/python -m unittest backend.tests.test_timer backend.tests.test_streaks backend.tests.test_tasks backend.tests.test_feeds backend.tests.test_fetcher

Run the last command from the project root.

## How it fits together

- `src/`: the UI, in plain TypeScript (the pet and the panel).
- `src-tauri/`: the Rust core. It owns the windows, the database and the timer tick.
- `backend/`: the Python logic for the timer, the streak and task quick add, plus the task fetcher that downloads feeds.

## Docs

- [spec.md](spec.md): what the product does.
- [ROADMAP.md](ROADMAP.md): the phases and their tasks.
- [ARCHITECTURE.md](ARCHITECTURE.md): how the parts connect.
- [DECISIONS.md](DECISIONS.md): why it is built this way.
- [KNOWN_ISSUES.md](KNOWN_ISSUES.md): what is unfinished or fragile, and what it costs.
- [CHANGELOG.md](CHANGELOG.md): what has been built.
