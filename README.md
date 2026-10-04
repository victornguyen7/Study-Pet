# Study Pet

Tauri desktop pet. See `docs/spec.md`; Phase 1 (pet, timer, streak) is built and Phase 2 (daily list) is under way.

- `src/pet`, `index.html`: transparent always-on-top pet window. Drag to move, click to open the panel, `Alt+Shift+P` to hide/show.
- `src/panel`, `panel.html`: panel window with three views: Today (session controls, the task list with quick add, today's focus blocks), Wrap up (day summary, last 7 days) and Settings (presets, streak goal).
- `src-tauri/src/lib.rs`: Rust core: SQLite store (`pet.db` in the project root for dev builds, in the OS app data dir for release builds), commands, Python sidecar.
- `backend/timer.py`: session timer state machine (focus, break, call back). Rust ticks it once a second, stores finished focus blocks, and broadcasts the state to both windows as a `timer` event.
- `backend/streaks.py`: streak rules (daily threshold, 2 freezes a month, milestones). Rust feeds it study minutes per day from the `session` table.
- `backend/tasks.py`: quick-add parser. Turns a typed line such as "STAT 310 HW3 Fri 2h" into a title, course, due date and estimate; Rust stores the result in the `task` table.
- `backend/main.py`: Python sidecar, JSON lines over stdin/stdout. Add a function to `METHODS`, call it from the UI with `invoke("backend_call", { method, params })`.

## Run

    npm install
    python3 -m venv backend/.venv   # once
    npm run tauri dev

## Check

    npm run build                   # typecheck + bundle the UI
    cd src-tauri && cargo test      # DB schema + Rust <-> Python round trip
    backend/.venv/bin/python -m unittest backend.tests.test_timer backend.tests.test_streaks backend.tests.test_tasks   # timer, streak and task logic (from the project root)
