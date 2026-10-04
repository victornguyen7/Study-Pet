# Study Pet

A desktop pet that keeps a student focused. It runs locally on a laptop: a small always-on-top pet, a focus timer with breaks, and a forgiving streak. Later phases add a daily to-do list, focus nudges and an end-of-day knowledge graph.

Currently in Phase 1 (pet, timer, streak), macOS first.

## Setup

You need Node.js, Rust and Python 3.

    npm install
    python3 -m venv backend/.venv
    npm run tauri dev

The Python backend uses only the standard library, so there is nothing to `pip install`.

## Check

    npm run build
    cd src-tauri && cargo test
    backend/.venv/bin/python -m unittest backend.tests.test_timer backend.tests.test_streaks

Run the last command from the project root.

## How it fits together

- `src/`: the UI, in plain TypeScript (the pet and the panel).
- `src-tauri/`: the Rust core. It owns the windows, the database and the timer tick.
- `backend/`: the Python logic for the timer and the streak.

## Docs

- [spec.md](spec.md): what the product does.
- [ROADMAP.md](ROADMAP.md): the phases and their tasks.
- [ARCHITECTURE.md](ARCHITECTURE.md): how the parts connect.
- [DECISIONS.md](DECISIONS.md): why it is built this way.
- [CHANGELOG.md](CHANGELOG.md): what has been built.
