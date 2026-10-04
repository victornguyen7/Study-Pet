# Changelog

## 0.2.0 · Phase 2, in progress

**Added**
- Tables for the daily list: `task`, `task_source_link` and `calendar_event`, plus a nullable `task_id` on `session` (schema version 2).
- Quick add: a typed line such as "STAT 310 HW3 Fri 2h" becomes a task with a course, due date and estimate (`backend/tasks.py`, with tests).
- Tasks section in the panel's Today view: add a task, check it off, delete it. Overdue tasks are marked.
- Commands `add_task`, `list_tasks`, `set_task_done` and `delete_task`.
- `.env` files are ignored by git.
- Today and All filter on the panel's task list. Today shows what is due today or overdue, undated tasks and the ones checked off today; All shows every open task.
- The panel is larger: 420 × 620, up from 360 × 520.
- The pet shows on every macOS Space (`visibleOnAllWorkspaces`), so it follows you between desktops.
- The panel opens on the Space you are on instead of pulling you back to the one it was last on (`move_to_active_space`, macOS only). New macOS-only dependency: `objc2-app-kit`.

**Not yet**
- A Today list built each morning with `planned_for`, imports from Canvas and the calendar, tying a session to a task, reminders.

## 0.1.0 · Phase 1, unreleased

**Added**
- Pet window: transparent, always on top, drag to move, click to open the panel, `Alt+Shift+P` to hide or show.
- Panel with three views: Today, Wrap up and Settings.
- Session timer: focus, break, and a call back when the break ends.
- Presets for focus and break lengths (25/5 and 50/10 to start).
- Streak: adjustable daily goal, 2 freezes a month, milestones at 7, 30 and 100 days.
- Wrap-up view with today's total and the last 7 days.
- SQLite store for presets, focus blocks and settings.
- Python sidecar for the timer and streak rules, with tests.
- Docs: spec, architecture, roadmap and decisions.
