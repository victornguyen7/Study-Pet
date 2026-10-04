# Study Pet

A desktop pet that keeps a student focused. It runs locally on a laptop: a small always-on-top pet that follows you across desktops, a focus timer with breaks, and a forgiving streak. A daily to-do list is being built now; later phases add focus nudges and an end-of-day knowledge graph.

Phase 1 (pet, timer, streak) is built. Phase 2 (daily list) is in progress: you can add, check off and delete your own tasks. macOS first.

## Setup

You need Node.js, Rust and Python 3.

    npm install
    python3 -m venv backend/.venv
    npm run tauri dev

The Python backend uses only the standard library, so there is nothing to `pip install`.

## Check

    npm run build
    cd src-tauri && cargo test
    backend/.venv/bin/python -m unittest backend.tests.test_timer backend.tests.test_streaks backend.tests.test_tasks

Run the last command from the project root.

## How it fits together

- `src/`: the UI, in plain TypeScript (the pet and the panel).
- `src-tauri/`: the Rust core. It owns the windows, the database and the timer tick.
- `backend/`: the Python logic for the timer, the streak and task quick add.

## Docs

- [spec.md](spec.md): what the product does.
- [ROADMAP.md](ROADMAP.md): the phases and their tasks.
- [ARCHITECTURE.md](ARCHITECTURE.md): how the parts connect.
- [DECISIONS.md](DECISIONS.md): why it is built this way.
- [CHANGELOG.md](CHANGELOG.md): what has been built.
