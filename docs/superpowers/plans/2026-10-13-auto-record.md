# Automatic Recording Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The focus block and its Start button are removed; the pet counts Study and Work time by itself, and the streak, the heatmap, the focus keeper and the break suggestion all run on that count.

**Architecture:** `label_day` in `backend/modes.py` already labels every stretch of a day. Rust saves each finished day's Study and Work seconds once, in a new `day_total` table, and labels today live; `streaks.py` gets the same minutes-per-day shape as before. A new `Keeper` class in `backend/drift.py` wraps the existing ladder (`Drift`): it turns the keeper on after 5 minutes of Study or Work, off after a break, and says when to suggest one.

**Tech Stack:** Python 3 standard library (`backend/`), Rust with `rusqlite` and `serde_json` (`src-tauri/`), plain TypeScript, HTML and CSS (`src/`, `panel.html`). No new package or process. One new table, two new settings.

**Spec:** `docs/superpowers/specs/2026-10-13-auto-record-design.md`

## Global Constraints

- **Tasks 1 to 5 only add to `backend/`.** The everyday app in `/Applications` starts Python from this source tree (`KNOWN_ISSUES.md`, item 33). The sidecar's `timer` and `drift` methods and `backend/timer.py` stay until Task 6.
- **Task 6 is not run without the user asking for it.** It breaks the installed everyday app until that app is rebuilt and reinstalled, and a reinstall happens only on the user's request, with `pet.db` backed up first.
- Do not rebuild or reinstall the everyday app. Run and check with the dev app only (`npm run tauri dev`), which has its own database in the project root.
- No table is dropped and no row is deleted from `session` or `preset`.
- Recorded time is Study plus Work seconds. Relax, Private and unknown time never count.
- Keeper numbers, exact: on after 300 s of Study or Work; off after 900 s of Relax in a row; off after 300 s with no sample; break suggested after 50 minutes by default, 0 turns it off.
- Heatmap shades: none, under 1 h, 1 to 2 h, 2 to 4 h, 4 h or more. Blue for a day with more Study, orange for more Work; a tie is Study.
- Local days are `"YYYY-MM-DD"` strings in the laptop's time zone, as `date(..., 'localtime')` gives them in SQLite and `toLocaleDateString("en-CA")` in the panel.
- A site's address is never shown in the panel or the pet's bubble: always its name.
- macOS only. Light and dark mode both have to read well.
- Match the surrounding code: comments explain why in plain words, Rust tests live in the `tests` module of `src-tauri/src/lib.rs`, Python tests in `backend/tests/`.

**Check commands** (the third runs from the project root):

    npm run build
    cd src-tauri && cargo test
    backend/.venv/bin/python -m unittest discover -s backend/tests -t .

## Review Focus

1. **The laptop sleeps or the user walks off mid-run** (no sample for 5 minutes or more): the keeper is off on return, nothing is nagged, and the 5 minutes are earned again. Pinned in Task 1.
2. **Totals that arrive as floats** (the sidecar adds a float time for a block still running, until Task 6): a day's minutes are still read, not taken as 0. Pinned in Task 2.
3. **A day with nothing noted, or only Relax**: no heatmap colour, no entry in the minutes, no error, and the day is not labelled again every minute. Pinned in Task 2.
4. **An answer about an app given after its days were totalled**: the saved totals are thrown away and rebuilt, so the past days count the answer. Pinned in Task 2.
5. **A break length that is 0, missing or not a number**: 0 never suggests; anything unreadable falls back to 50. Pinned in Task 1 (Steps 1 and 6) and Task 3.

## File Structure

| File | Change |
| --- | --- |
| `backend/drift.py` | New class `Keeper` around `Drift` |
| `backend/main.py` | New method `watch` (Task 1); `timer`, `drift` and the running-block patch removed (Task 6) |
| `backend/tests/test_drift.py` | `KeeperTest` |
| `backend/timer.py`, `backend/tests/test_timer.py` | Deleted (Task 6) |
| `src-tauri/src/db.rs` | `day_total` table, `auto_since`, `days_to_total`, `save_day_total`, new `streak_input`; presets, `Session`, `save_session`, `day_data`, `day` removed |
| `src-tauri/src/session.rs` | `day_input`, `day` command; `timer` and `timer_action` removed |
| `src-tauri/src/modes.rs` | `day_total` cleared on a rule change; `forget_rule`, `break_after`, `KEEPER_ON`; `keeper` takes a break length; the S-7 offer removed |
| `src-tauri/src/monitor.rs` | Calls `watch`; break suggestion; sets `KEEPER_ON` |
| `src-tauri/src/browser.rs` | `PAUSED` removed |
| `src-tauri/src/lib.rs` | The 1-second tick becomes a 60-second loop; command list; quit hook; tests |
| `src/pet/main.ts` | Off the timer: draws from the `drift` event's `on`; new lines |
| `src/timer.ts` | Deleted |
| `src/panel/main.ts`, `panel.html`, `src/panel/panel.css` | Tank, presets, blocks list and task Start removed; two-colour heatmap; break length setting; wording |
| `docs/` | `spec.md`, `ARCHITECTURE.md`, `DECISIONS.md`, `CHANGELOG.md`, `ROADMAP.md`, `KNOWN_ISSUES.md` |

**Between Task 3 and Task 4** the panel still has its timer, but the pet no longer speaks for breaks. That is expected on this branch.

**Before Task 1:** commit the spec and this plan.

```bash
git add docs/superpowers
git commit -m "spec and plan for automatic recording"
```

---

### Task 1: The keeper turns itself on and off (Python)

**Files:**
- Modify: `backend/drift.py` (append after class `Drift`)
- Modify: `backend/main.py`
- Test: `backend/tests/test_drift.py`

**Interfaces:**
- Consumes: `Drift.update(now, drifting, what=None, music=False)`, `is_drift`, `is_music`, `classify`, `GAP_SECS` (all exist).
- Produces: `Keeper.update(now, mode, drifting, what=None, music=False, relax_declared=False, break_after_min=50) -> {"step", "seconds", "what", "returned", "on": bool, "break": bool}`. Sidecar method `watch`, params `{app, bundle_id, domain, url, rules, declared, break_after_min}`, returns that state plus `"mode"`.

- [ ] **Step 1: Write the failing tests**

In `backend/tests/test_drift.py`, change the import to:

```python
from backend.drift import Drift, Keeper, feed, is_drift, is_music
```

Append:

```python
def samples(keeper, start, secs, mode, drifting=False, **kw):
    """One sample every 5 s from `start` for `secs` seconds. -> {time: state}."""
    return {t: keeper.update(t, mode, drifting, "x", **kw) for t in range(start, start + secs + 1, 5)}


class KeeperTest(unittest.TestCase):
    def on_keeper(self):
        """A keeper that turned on at t=300, after samples of Study from 0."""
        keeper = Keeper()
        samples(keeper, 0, 300, "study")
        return keeper

    def test_on_after_five_minutes_of_study_or_work(self):
        for mode in ("study", "work"):
            states = samples(Keeper(), 0, 300, mode)
            self.assertEqual((states[295]["on"], states[300]["on"]), (False, True), mode)

    def test_what_does_not_earn(self):
        # unknown, Private and Relax time never turn it on, and Relax is not drift while it is off
        for mode, drifting in ((None, False), ("private", False), ("relax", True)):
            last = samples(Keeper(), 0, 900, mode, drifting)[900]
            self.assertEqual((last["on"], last["step"]), (False, 0), mode)
        # nor does a music app in front
        self.assertFalse(samples(Keeper(), 0, 900, None, music=True)[900]["on"])
        # an unknown app in the middle neither adds nor takes away
        keeper = Keeper()
        samples(keeper, 0, 200, "study")
        self.assertFalse(samples(keeper, 205, 195, None)[400]["on"])
        states = samples(keeper, 405, 95, "study")
        self.assertEqual((states[495]["on"], states[500]["on"]), (False, True))

    def test_the_ladder_runs_only_while_on(self):
        keeper = self.on_keeper()
        states = samples(keeper, 305, 120, "relax", True)
        self.assertEqual((states[425]["step"], states[425]["on"], states[425]["what"]), (2, True, "x"))
        back = keeper.update(430, "study", False)
        self.assertEqual((back["returned"], back["on"], back["step"]), (True, True, 0))

    def test_fifteen_minutes_of_relax_was_a_break(self):
        keeper = self.on_keeper()
        states = samples(keeper, 305, 900, "relax", True)
        self.assertEqual((states[1200]["on"], states[1200]["step"]), (True, 4))
        # the keeper lets go without a "welcome back", and the five minutes are earned again
        self.assertEqual(states[1205], {"step": 0, "seconds": 0, "what": None, "returned": False, "on": False, "break": False})
        states = samples(keeper, 1210, 295, "study")
        self.assertEqual((states[1500]["on"], states[1505]["on"]), (False, True))

    def test_five_minutes_away_was_a_break(self):
        # a shorter gap adds nothing and changes nothing
        self.assertTrue(self.on_keeper().update(595, "study", False)["on"])
        keeper = self.on_keeper()
        self.assertFalse(keeper.update(600, "study", False)["on"])
        # and a keeper that was nudging does not welcome anyone back after it
        keeper = self.on_keeper()
        samples(keeper, 305, 300, "relax", True)
        self.assertEqual(keeper.update(1000, "study", False)["returned"], False)

    def test_relax_declared_keeps_it_off(self):
        keeper = self.on_keeper()
        state = keeper.update(305, "relax", True, relax_declared=True)
        self.assertEqual((state["on"], state["step"]), (False, 0))
        self.assertFalse(samples(Keeper(), 0, 900, "relax", relax_declared=True)[900]["on"])

    def test_a_break_is_suggested_once_per_run(self):
        keeper = Keeper()
        states = samples(keeper, 0, 3600, "study")
        self.assertEqual([t for t, s in states.items() if s["break"]], [3000])  # 50 minutes of work
        # away for a while, then a new run: suggested again, 50 minutes in
        states = samples(keeper, 4000, 3100, "study")
        self.assertEqual([t for t, s in states.items() if s["break"]], [7000])

    def test_break_length_zero_never_suggests(self):
        for minutes in (0, -5):
            states = samples(Keeper(), 0, 7200, "study", break_after_min=minutes)
            self.assertFalse(any(s["break"] for s in states.values()), minutes)
        # shorter than the time it takes to turn on: said as it turns on
        states = samples(Keeper(), 0, 600, "study", break_after_min=1)
        self.assertEqual([t for t, s in states.items() if s["break"]], [300])
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `backend/.venv/bin/python -m unittest backend.tests.test_drift`
Expected: `ImportError: cannot import name 'Keeper'`

- [ ] **Step 3: Write `Keeper`**

Append to `backend/drift.py`:

```python
ON_AFTER = 300  # seconds of Study or Work before the keeper turns on
OFF_AFTER = 900  # this much Relax in a row was a break: the keeper turns off
AWAY_SECS = 300  # no sample for this long (the lid was shut, or nobody was there) was a break too


class Keeper:
    """Turns the focus keeper on and off by itself, and runs the ladder (Drift) while it is on.

    On after ON_AFTER of Study or Work. Off after a break: OFF_AFTER of Relax in a row, AWAY_SECS
    with no sample, or the user saying they are relaxing. A break is suggested once per run.
    """

    def __init__(self):
        self.drift = Drift()
        self.last = None  # when update was last called
        self.earned = 0  # seconds of Study or Work since the keeper was last off
        self.on_since = None  # None while the keeper is off
        self.suggested = False  # a break has been suggested in this run

    def _off(self):
        self.drift = Drift()  # a fresh ladder, so a break is not answered with "welcome back"
        self.earned, self.on_since, self.suggested = 0, None, False

    # ponytail: while the keeper is off, `earned` only goes up, so two minutes of work, an hour of
    # Relax with no gap, and three more minutes turn it on. Let Relax wear `earned` down if that
    # turns it on too easily.
    def update(self, now, mode, drifting, what=None, music=False, relax_declared=False, break_after_min=50):
        """Called with every sample of the front app. mode: study, work, relax, private or None.
        -> Drift.update's state, plus {"on": the keeper is on, "break": True on the one update
        that suggests a break}."""
        gap = AWAY_SECS if self.last is None else now - self.last
        self.last = now
        if gap >= AWAY_SECS or relax_declared:
            self._off()
        elif mode in ("study", "work") and not drifting and not music and gap <= GAP_SECS:
            self.earned += gap
        if self.on_since is None and self.earned >= ON_AFTER:
            self.on_since = now
        on = self.on_since is not None
        state = self.drift.update(now, on and drifting, what, music=on and music)
        if state["seconds"] >= OFF_AFTER:
            self._off()
            on, state = False, self.drift.update(now, False)
        # the run began ON_AFTER before the keeper turned on
        suggest = (
            on and not self.suggested and break_after_min > 0
            and now - self.on_since + ON_AFTER >= break_after_min * 60
        )
        self.suggested = self.suggested or suggest
        return state | {"on": on, "break": suggest}
```

- [ ] **Step 4: Run the tests to see them pass**

Run: `backend/.venv/bin/python -m unittest backend.tests.test_drift`
Expected: `OK`, 13 tests.

- [ ] **Step 5: Add the `watch` method to the sidecar**

In `backend/main.py`, change the import line to:

```python
from backend.drift import Drift, Keeper, is_drift, is_music
```

After the `drift` function, add:

```python
_keeper = Keeper()


def watch(params):
    """params: {app, bundle_id, domain, url, rules, declared, break_after_min}, what is in front now.
    The focus keeper turns itself on and off (Keeper in backend/drift.py).
    -> Keeper.update's state, plus "mode": study, work, relax, private or None, for the pet's face."""
    rules, declared = params.get("rules"), params.get("declared")
    after = params.get("break_after_min")
    mode = classify(params, declared, rules)[0]
    state = _keeper.update(
        time.time(), mode, is_drift(params, rules), params.get("domain") or params.get("app"),
        music=is_music(params, rules), relax_declared=declared == "relax",
        break_after_min=after if isinstance(after, (int, float)) else 50,
    )
    return state | {"mode": mode}
```

Add `"watch": watch,` to `METHODS`, after `"drift": drift,`. Leave `timer`, `drift` and `modes` as they are.

- [ ] **Step 6: Check the sidecar answers**

Run from the project root:

```bash
echo '{"method": "watch", "params": {"app": "Steam", "bundle_id": "com.valvesoftware.steam", "rules": {}, "declared": null, "break_after_min": "x"}}' | backend/.venv/bin/python -m backend.main
```

Expected: `{"ok": {"step": 0, "seconds": 0, "what": null, "returned": false, "on": false, "break": false, "mode": "relax"}}`

- [ ] **Step 7: Run every Python test, then commit**

Run: `backend/.venv/bin/python -m unittest discover -s backend/tests -t .`
Expected: `OK`

```bash
git add backend/drift.py backend/main.py backend/tests/test_drift.py
git commit -m "keeper: on after 5 min of study or work, off after a break"
```

---

### Task 2: Minutes per day come from recorded time; two-colour heatmap

**Files:**
- Modify: `src-tauri/src/db.rs`
- Modify: `src-tauri/src/session.rs`
- Modify: `src-tauri/src/modes.rs` (`set_rule`, `learned`)
- Modify: `src-tauri/src/lib.rs` (command list, tests)
- Modify: `src/panel/main.ts`, `panel.html`, `src/panel/panel.css`, `src/streak.ts`

**Interfaces:**
- Consumes: `modes::modes_day(app: AppHandle, day: Option<String>) -> Result<Value, String>`, whose reply has `totals: {mode: seconds}` (exists).
- Produces:
  - `db::days_to_total(conn) -> rusqlite::Result<Vec<String>>`
  - `db::save_day_total(conn, day: &str, totals: &Value) -> rusqlite::Result<()>`
  - `db::streak_input(conn, today_totals: &Value) -> rusqlite::Result<Value>`, returning `{minutes: {day: f64}, modes: {day: "study" | "work"}, today, threshold}`
  - `modes::forget_rule(conn, key: &str) -> rusqlite::Result<()>`
  - Tauri command `day` (now in `session.rs`), returning the same object as `streak_input`. It no longer has `sessions`.

- [ ] **Step 1: Write the failing test**

In the `tests` module of `src-tauri/src/lib.rs`, replace the whole test `streak_counts_a_day_with_enough_study` with:

```rust
    #[test]
    fn minutes_per_day_join_old_blocks_and_recorded_time() {
        use db::{days_to_total, save_day_total};
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        // a new database starts counting recorded time today
        let today: String = conn.query_row("SELECT date('now', 'localtime')", [], |r| r.get(0)).unwrap();
        assert_eq!(setting(&conn, "auto_since").unwrap().as_deref(), Some(today.as_str()));
        // here the switch was three days ago
        conn.execute("UPDATE setting SET value = date('now', 'localtime', '-3 days') WHERE key = 'auto_since'", []).unwrap();
        let day = |back: i64| -> String {
            conn.query_row("SELECT date('now', 'localtime', ?1)", [format!("-{back} days")], |r| r.get(0)).unwrap()
        };
        // local noon `back` days ago and half an hour later, as the UTC text the tables hold
        let noon = |back: i64| -> (String, String) {
            conn.query_row(
                "SELECT strftime('%Y-%m-%dT%H:%M:%SZ', noon), strftime('%Y-%m-%dT%H:%M:%SZ', noon, '+30 minutes')
                 FROM (SELECT datetime(date('now', 'localtime', ?1) || ' 12:00:00', 'utc') AS noon)",
                [format!("-{back} days")],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap()
        };
        let block = |back: i64| conn.execute("INSERT INTO session (started_at, ended_at, planned_min, break_min) VALUES (?1, ?2, 25, 5)", noon(back)).unwrap();
        let app = |back: i64| conn.execute("INSERT INTO app_span (started_at, ended_at, app) VALUES (?1, ?2, 'Preview')", noon(back)).unwrap();

        block(5); // before the switch: its 30 minutes count, as Study
        block(2); // after it: a block no longer counts
        app(5); // before the switch: never labeled
        app(2);
        app(1);
        app(0); // today is labeled live, never saved
        assert_eq!(days_to_total(&conn).unwrap(), [day(2), day(1)]);
        // totals as the sidecar sends them: seconds per mode, sometimes as floats
        save_day_total(&conn, &day(2), &json!({ "study": 600, "work": 1800.0, "relax": 9999 })).unwrap();
        save_day_total(&conn, &day(1), &json!({ "relax": 500 })).unwrap(); // a day of Relax only
        assert!(days_to_total(&conn).unwrap().is_empty()); // neither is labeled again

        let input = streak_input(&conn, &json!({ "study": 1200.0, "work": 300 })).unwrap();
        assert_eq!(
            input["minutes"],
            json!({ (day(5)): 30.0, (day(2)): 40.0, (day(0)): 25.0 })
        );
        assert_eq!(input["modes"], json!({ (day(5)): "study", (day(2)): "work", (day(0)): "study" }));
        assert_eq!((input["today"].as_str(), input["threshold"].as_f64()), (Some(today.as_str()), Some(25.0)));
        // a tie is Study; nothing recorded today is no entry at all; a stale row for today is not read
        assert_eq!(streak_input(&conn, &json!({ "study": 300, "work": 300 })).unwrap()["modes"][&today], "study");
        save_day_total(&conn, &today, &json!({ "study": 99999 })).unwrap();
        for nothing in [json!({}), json!(null), json!({ "relax": 4000, "unknown": 50 })] {
            assert_eq!(streak_input(&conn, &nothing).unwrap()["minutes"].get(&today), None, "{nothing}");
        }

        // the streak rule takes the minutes as they are
        let mut for_streak = input.clone();
        for_streak.as_object_mut().unwrap().remove("modes");
        let mut sidecar = spawn_sidecar().unwrap();
        let state = call(&mut sidecar, "streak", Some(for_streak)).unwrap();
        assert_eq!((&state["today_minutes"], &state["today_counted"]), (&json!(25), &json!(true)));

        // an answer about an app, or forgetting one, throws the saved totals away so they are rebuilt
        let saved = || conn.query_row("SELECT count(*) FROM day_total", [], |r| r.get::<_, i64>(0)).unwrap();
        assert_eq!(saved(), 3);
        modes::set_rule(&conn, "preview", "Preview", "study").unwrap();
        assert_eq!((saved(), days_to_total(&conn).unwrap()), (0, vec![day(2), day(1)]));
        save_day_total(&conn, &day(2), &json!({ "study": 1800 })).unwrap();
        modes::forget_rule(&conn, "preview").unwrap();
        assert_eq!((saved(), modes::rules(&conn).unwrap().len()), (0, 0));
    }
```

Change the test module's `use db::...` line to:

```rust
    use db::{save_session, setting, streak_input, Session};
```

- [ ] **Step 2: Run it to see it fail**

Run: `cd src-tauri && cargo test minutes_per_day`
Expected: does not compile: `unresolved imports db::days_to_total, db::save_day_total`.

- [ ] **Step 3: Add the table and the switch date**

In `init_db` in `src-tauri/src/db.rs`, inside the `execute_batch` string, after the `app_rule` table and before the closing `",`, add:

```sql
        CREATE TABLE IF NOT EXISTS day_total (
            day TEXT PRIMARY KEY,        -- a finished local day, YYYY-MM-DD
            study_secs INTEGER NOT NULL, -- as backend/modes.py labeled it; every row goes when a rule changes
            work_secs INTEGER NOT NULL
        );
        -- the first day recorded time counts toward the streak; earlier days keep their focus blocks
        INSERT OR IGNORE INTO setting (key, value) VALUES ('auto_since', date('now', 'localtime'));
```

(The `app_rule` statement currently ends with `);",` — make it end with `);` and put the closing `",` after the new `INSERT`.)

- [ ] **Step 4: Replace `streak_input`, `day_data` and `day`**

In `src-tauri/src/db.rs`, delete `streak_input`, `day_data` and the `day` command, and put this in their place:

```rust
/// Finished days since the switch that have something noted and no saved total yet, oldest first.
pub(crate) fn days_to_total(conn: &Connection) -> rusqlite::Result<Vec<String>> {
    conn.prepare(
        "SELECT DISTINCT date(started_at, 'localtime') AS d
         FROM (SELECT started_at FROM app_span UNION ALL SELECT started_at FROM activity_span)
         WHERE d >= (SELECT value FROM setting WHERE key = 'auto_since') AND d < date('now', 'localtime')
           AND d NOT IN (SELECT day FROM day_total)
         ORDER BY 1",
    )?
    .query_map([], |r| r.get(0))?
    .collect()
}

/// Saves one finished day from label_day's `totals` (seconds per mode). A day with no Study or Work
/// is saved too, as zeros, so it is not labeled again.
pub(crate) fn save_day_total(conn: &Connection, day: &str, totals: &Value) -> rusqlite::Result<()> {
    let secs = |mode: &str| totals[mode].as_f64().unwrap_or(0.0) as i64;
    conn.execute(
        "INSERT OR REPLACE INTO day_total (day, study_secs, work_secs) VALUES (?1, ?2, ?3)",
        (day, secs("study"), secs("work")),
    )?;
    Ok(())
}

/// What backend/streaks.py and the heatmap need: recorded minutes per local day, each day's larger
/// mode, today's date, the threshold. A day before the switch has its focus blocks, as Study; a
/// finished day since has its row in day_total; today is `today_totals`, label_day's totals for it.
// ponytail: a block or a span that runs past midnight counts wholly toward the day it started.
pub(crate) fn streak_input(conn: &Connection, today_totals: &Value) -> rusqlite::Result<Value> {
    let (mut minutes, mut modes) = (serde_json::Map::new(), serde_json::Map::new());
    let mut stmt = conn.prepare(
        "SELECT date(started_at, 'localtime'), sum(unixepoch(ended_at) - unixepoch(started_at)) / 60.0, 'study'
         FROM session
         WHERE ended_at IS NOT NULL AND date(started_at, 'localtime') < (SELECT value FROM setting WHERE key = 'auto_since')
         GROUP BY 1
         UNION ALL
         SELECT day, (study_secs + work_secs) / 60.0, iif(work_secs > study_secs, 'work', 'study')
         FROM day_total
         WHERE day >= (SELECT value FROM setting WHERE key = 'auto_since') AND day < date('now', 'localtime')
           AND study_secs + work_secs > 0",
    )?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let day: String = row.get(0)?;
        minutes.insert(day.clone(), json!(row.get::<_, f64>(1)?));
        modes.insert(day, json!(row.get::<_, String>(2)?));
    }
    let today: String = conn.query_row("SELECT date('now', 'localtime')", [], |r| r.get(0))?;
    // as_f64: a block still running reaches label_day as a float time
    let secs = |mode: &str| today_totals[mode].as_f64().unwrap_or(0.0);
    if secs("study") + secs("work") > 0.0 {
        minutes.insert(today.clone(), json!((secs("study") + secs("work")) / 60.0));
        modes.insert(today.clone(), json!(if secs("work") > secs("study") { "work" } else { "study" }));
    }
    let threshold: Option<f64> = conn
        .query_row("SELECT value FROM setting WHERE key = 'streak_threshold_min'", [], |r| r.get(0))
        .optional()?;
    Ok(json!({ "minutes": minutes, "modes": modes, "today": today, "threshold": threshold.unwrap_or(25.0) }))
}
```

- [ ] **Step 5: Label the days in `session.rs`**

In `src-tauri/src/session.rs`, change the `use crate::db` line to:

```rust
use crate::db::{days_to_total, lock, save_day_total, save_session, setting, streak_input, Db, Session};
```

Replace `streak_state` with:

```rust
/// Minutes per day for the streak and the heatmap. Each finished day is labeled once, by
/// backend/modes.py, and kept in day_total; today is labeled now.
fn day_input(app: &AppHandle) -> Result<Value, String> {
    let db = app.state::<Db>();
    let days = days_to_total(&lock(&db.0)).map_err(|e| e.to_string())?;
    for day in days {
        // modes_day takes the database lock itself, so none is held across it
        let labeled = crate::modes::modes_day(app.clone(), Some(day.clone()))?;
        save_day_total(&lock(&db.0), &day, &labeled["totals"]).map_err(|e| e.to_string())?;
    }
    let today = crate::modes::modes_day(app.clone(), None)?;
    let input = streak_input(&lock(&db.0), &today["totals"]).map_err(|e| e.to_string())?;
    Ok(input)
}

/// What the panel's heatmap shows: { minutes, modes, today, threshold }.
#[tauri::command(async)]
pub(crate) fn day(app: AppHandle) -> Result<Value, String> {
    day_input(&app)
}

/// Computes the streak and broadcasts it to every window as a "streak" event.
pub(crate) fn streak_state(app: &AppHandle) -> Result<Value, String> {
    let mut input = day_input(app)?;
    // backend/streaks.py takes minutes, today and threshold, and nothing else
    if let Some(input) = input.as_object_mut() {
        input.remove("modes");
    }
    let state = sidecar_call(&app.state::<Backend>(), "streak", Some(input))?;
    app.emit("streak", &state).map_err(|e| e.to_string())?;
    Ok(state)
}
```

In `generate_handler!` in `src-tauri/src/lib.rs`, change `db::day,` to `session::day,`.

- [ ] **Step 6: Throw the saved totals away when a rule changes**

In `src-tauri/src/modes.rs`, at the top of `set_rule`'s body add:

```rust
    // every finished day is labeled again with the new answer (days_to_total in db.rs)
    conn.execute("DELETE FROM day_total", [])?;
```

After `set_rule`, add:

```rust
/// Forgets an answer, so the pet may ask again. The saved day totals go, as in set_rule.
pub(crate) fn forget_rule(conn: &Connection, key: &str) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM day_total", [])?;
    conn.execute("DELETE FROM app_rule WHERE key = ?1", [key])?;
    Ok(())
}
```

In the `learned` command, replace

```rust
            conn.execute("DELETE FROM app_rule WHERE key = ?1", [&key]).map(drop)
```

with

```rust
            forget_rule(&conn, &key)
```

- [ ] **Step 7: Run the Rust tests**

Run: `cd src-tauri && cargo test`
Expected: all pass, with no warning about an unused import in `db.rs`. If `State` is reported unused there, leave it: the preset commands still use it until Task 4.

- [ ] **Step 8: The panel reads the new `day`**

In `src/panel/main.ts`:

Replace the `Day` type with:

```ts
// Mirrors streak_input() in src-tauri/src/db.rs
type Day = {
  minutes: Record<string, number>; // "YYYY-MM-DD" -> minutes of study and work
  modes: Record<string, "study" | "work">; // which of the two had more of that day
  today: string;
  threshold: number;
};
```

Replace the comment above `HEAT_WEEKS` and the body of the `for` loop in `showHeatmap` so they read:

```ts
// R-1: about four months of days, a column per week from Monday, shaded by hours of study and work
```

```ts
    const date = d.toLocaleDateString("en-CA"); // "YYYY-MM-DD"
    const minutes = day.minutes[date] ?? 0;
    const mode = day.modes[date] ?? "study";
    const label = `${d.toLocaleDateString([], { weekday: "short", day: "numeric", month: "short" })}: ${
      minutes ? `${duration(minutes)}, mostly ${mode}` : "nothing recorded"
    }`;
    // blue for a day of mostly Study, orange for mostly Work (panel.css)
    const cell = el("button", "", `l${heatLevel(minutes)} ${mode}`);
```

In `loadDay`, delete the `$("blocks").replaceChildren(...)` statement and the `if (!day.sessions.length)` statement after it. Replace the `todayLine` statement and the `$("wrap-total").textContent = ...` statement with:

```ts
  const now = day.minutes[day.today] ?? 0;
  const todayLine = now
    ? `You studied or worked for ${duration(now)} today.`
    : "Nothing recorded today yet. The pet counts study and work time by itself.";
```

```ts
  $("wrap-total").textContent = !recapDay
    ? todayLine
    : then
      ? `You studied or worked for ${duration(then)} on ${when}.`
      : `Nothing recorded on ${when}.`;
```

Change the comment `// an earlier day picked in the heatmap: its focus, and its name over the recap` to `// an earlier day picked in the heatmap: its time, and its name over the recap`.

In `src/streak.ts`, change the comment on `threshold` to `// minutes of study and work a day needs to count`.

- [ ] **Step 9: The panel's markup and colours**

In `panel.html`:

- Delete the whole `<section>` that holds `<h2>Today's focus blocks</h2>`.
- Change `aria-label="Today's focus toward your daily goal"` to `aria-label="Today's study and work toward your daily goal"`.
- In Wrap up, change `<h2>Focus</h2>` to `<h2>Study and work</h2>` and its button's `aria-label` to `Refresh Study and work`.
- Replace the `<p class="muted heat-key">` paragraph with:

```html
        <p class="muted heat-key">
          Study <span class="l1"></span><span class="l2"></span><span class="l3"></span><span class="l4"></span>
          Work <span class="l1 work"></span><span class="l2 work"></span><span class="l3 work"></span><span class="l4 work"></span>
          · under 1 h, 1 to 2 h, 2 to 4 h, 4 h or more. A day takes the colour of whichever had more time. Click a
          day to see it below.
        </p>
```

- In Settings, change `min of focus</label>` to `min of study or work</label>`.

In `src/panel/panel.css`, in `:root` after `--heat-4: #104281;` add:

```css
  /* the same four steps for a day of mostly Work, around --work */
  --heat-work-1: #f4b393;
  --heat-work-2: #ee8a5f;
  --heat-work-3: #c9501f;
  --heat-work-4: #8a3210;
```

In the dark block, after `--heat-4: #b7d3f6;` add:

```css
    --heat-work-1: #a8431a;
    --heat-work-2: #d95926;
    --heat-work-3: #ea8a61;
    --heat-work-4: #f6c4ad;
```

After the `.l4 { ... }` rule add:

```css
.work.l1 {
  background: var(--heat-work-1);
}

.work.l2 {
  background: var(--heat-work-2);
}

.work.l3 {
  background: var(--heat-work-3);
}

.work.l4 {
  background: var(--heat-work-4);
}
```

- [ ] **Step 10: Check the orange ramp**

Load the `dataviz` skill and run its palette validator on both orange ramps as ordinal ramps, against the panel's surfaces: `#ffffff` in light mode, `#18323b` in dark. The blue ramp passed the same check in Phase 4. If a step fails, change that step's value in both places it is defined and run the check again; write the final values in `DECISIONS.md` in Task 5.

- [ ] **Step 11: Build, and look at it in the dev app**

Run: `npm run build`
Expected: no TypeScript error. (`day.sessions` is no longer read anywhere.)

Run: `npm run tauri dev`, open the panel, then Wrap up. Expected: today's square is coloured once a few minutes of a Study or Work app have been in front; hovering it says "…, mostly study" or "…, mostly work"; squares from before today look as they did. Answer one "What are these for?" question as Work and press Refresh: the day's colour and total follow.

- [ ] **Step 12: Commit**

```bash
git add src-tauri/src src/panel src/streak.ts panel.html
git commit -m "streak and heatmap count recorded study and work time"
```

---

### Task 3: The core and the pet follow the keeper

**Files:**
- Modify: `src-tauri/src/modes.rs`
- Modify: `src-tauri/src/monitor.rs` (`start`)
- Modify: `src-tauri/src/lib.rs` (tests)
- Modify: `src/pet/main.ts`
- Modify: `src/panel/main.ts`, `panel.html`

**Interfaces:**
- Consumes: sidecar method `watch` from Task 1; `session::play_sound(app, name)` (exists).
- Produces:
  - `modes::KEEPER_ON: AtomicBool`, true while the keeper is on (Task 4 reads it)
  - `modes::break_after(conn) -> rusqlite::Result<i64>`, minutes, 50 when unset or unreadable
  - Tauri command `keeper(personality?, block?, breakAfter?)` returning `{ personality, block, break_after }`
  - The `drift` event's payload gains `on: boolean`

- [ ] **Step 1: Write the failing test**

In `src-tauri/src/lib.rs`, in the test that holds the comment `// the ladder, through the sidecar: nothing is drift without a focus block, and Steam is with one`, replace from that comment down to and including the line `assert_eq!(call(&mut sidecar, "drift", Some(preview)).unwrap()["mode"], "study");` with:

```rust
        // the keeper, through the sidecar: off until study or work has added up, so Steam is not drift yet
        let mut sidecar = spawn_sidecar().unwrap();
        let steam = json!({ "app": "Steam", "bundle_id": "com.valvesoftware.steam", "domain": null, "url": null, "rules": {}, "declared": null, "break_after_min": 50 });
        let off = json!({ "step": 0, "seconds": 0, "what": null, "returned": false, "on": false, "break": false, "mode": "relax" });
        assert_eq!(call(&mut sidecar, "watch", Some(steam)).unwrap(), off);
        // an app nothing knows has no mode; this is what the pet's face follows
        let preview = json!({ "app": "Preview", "bundle_id": "com.apple.Preview", "rules": {}, "declared": null });
        assert_eq!(call(&mut sidecar, "watch", Some(preview)).unwrap()["mode"], Value::Null);

        // the break length: 50 minutes unless the user set one, and 0 is kept as "never"
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        assert_eq!(modes::break_after(&conn).unwrap(), 50);
        for (stored, minutes) in [("0", 0), ("90", 90), ("soon", 50), ("", 50)] {
            conn.execute("INSERT OR REPLACE INTO setting (key, value) VALUES ('break_after_min', ?1)", [stored]).unwrap();
            assert_eq!(modes::break_after(&conn).unwrap(), minutes, "{stored:?}");
        }
```

- [ ] **Step 2: Run it to see it fail**

Run: `cd src-tauri && cargo test`
Expected: does not compile: `cannot find function break_after in module modes`.

- [ ] **Step 3: The setting, the flag and the command**

In `src-tauri/src/modes.rs`, replace the section comment `// ---- Focus keeper settings: the voice of the pet's lines, and the opt-in block (ladder step 4) ----` and the `keeper` command's doc comment and body with:

```rust
// ---- Focus keeper settings: the voice of the pet's lines, the opt-in block (ladder step 4), and
// how long a run of study or work is before the pet suggests a break ----

/// True while the keeper is on (Keeper in backend/drift.py). The front-app monitor sets it with each sample.
pub(crate) static KEEPER_ON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Minutes of study or work before a break is suggested. 0 is never; 50 when unset or unreadable.
pub(crate) fn break_after(conn: &Connection) -> rusqlite::Result<i64> {
    Ok(crate::db::setting(conn, "break_after_min")?.and_then(|v| v.parse().ok()).unwrap_or(50))
}

/// { personality: "gentle" | "sassy" | "roast", block: bool, break_after: minutes }. Pass any to
/// change it; every window hears the new settings as a "keeper" event.
#[tauri::command]
pub(crate) fn keeper(app: AppHandle, personality: Option<String>, block: Option<bool>, break_after: Option<i64>) -> Result<Value, String> {
    let db = app.state::<Db>();
    let conn = lock(&db.0);
    let set = |key: &str, value: &str| conn.execute("INSERT OR REPLACE INTO setting (key, value) VALUES (?1, ?2)", (key, value));
    if let Some(p) = personality.filter(|p| ["gentle", "sassy", "roast"].contains(&p.as_str())) {
        set("personality", &p).map_err(|e| e.to_string())?;
    }
    if let Some(on) = block {
        set("block_drift", if on { "1" } else { "0" }).map_err(|e| e.to_string())?;
    }
    // up to 10 hours; anything else is left as it was
    if let Some(minutes) = break_after.filter(|m| (0..=600).contains(m)) {
        set("break_after_min", &minutes.to_string()).map_err(|e| e.to_string())?;
    }
    let get = |key: &str| crate::db::setting(&conn, key).map_err(|e| e.to_string());
    let state = json!({
        "personality": get("personality")?.unwrap_or_else(|| "gentle".into()),
        "block": get("block_drift")?.as_deref() == Some("1"),
        "break_after": self::break_after(&conn).map_err(|e| e.to_string())?,
    });
    app.emit("keeper", &state).map_err(|e| e.to_string())?;
    Ok(state)
}
```

- [ ] **Step 4: The monitor asks `watch`**

In `start` in `src-tauri/src/monitor.rs`:

Change the `json!` that builds the front sample so it ends with the break length:

```rust
                    Ok((block, json!({
                        "app": name, "bundle_id": bundle, "domain": page.0, "url": page.1,
                        "rules": crate::modes::rule_map(&conn)?, "declared": crate::modes::declared(&conn)?,
                        "break_after_min": crate::modes::break_after(&conn)?,
                    })))
```

Replace the comment `// the ladder is in backend/drift.py; ...` (two lines) and the `checked` statement with:

```rust
            // the keeper and its ladder are in backend/drift.py; the pet hears each change as a "drift"
            // event, which also carries what its face shows: the mode of what is in front, and whether
            // audio is on
            let checked = stored.map_err(|e| e.to_string()).and_then(|(block, front)| {
                let site = front["domain"].as_str().map(str::to_string);
                let after = front["break_after_min"].as_i64().unwrap_or(50);
                let mut state = sidecar_call(&app.state::<Backend>(), "watch", Some(front))?;
                // R-5: the pet's line names the site, never its address
                if let (Some(site), true) = (&site, state["what"].is_string()) {
                    state["what"] = json!(crate::modes::site_name(site));
                }
                // step 4, once as the ladder reaches it, and only if the user opted in: a site can be
                // blocked, an app cannot
                if let (true, Some(site)) = (block && state["step"] == 4 && drift["step"] != 4, site) {
                    crate::browser::block(&site, BLOCK_MIN);
                }
                // the minute loop in lib.rs holds reminders and questions back while the keeper is on
                crate::modes::KEEPER_ON.store(state["on"] == true, std::sync::atomic::Ordering::Relaxed);
                // said once per run of study or work, then taken off so the windows are not told twice
                if state["break"] == true {
                    crate::session::play_sound(&app, "Glass");
                    let _ = app.emit("reminder", format!("You've been at it for {after} minutes. Take a break?"));
                }
                if let Some(state) = state.as_object_mut() {
                    state.remove("break");
                }
                Ok::<_, String>(state)
            });
```

Change the doc comment above `start` to:

```rust
/// Samples the front app every 5 s, except while the pet looks away and with nobody at the keyboard.
```

(`paused` still also covers a break of the old timer until Task 4; the comment describes where this is going.)

- [ ] **Step 5: Run the Rust tests**

Run: `cd src-tauri && cargo test`
Expected: all pass.

- [ ] **Step 6: The pet draws from the keeper**

In `src/pet/main.ts`:

Delete the import line `import { onTimer, type TimerState } from "../timer";`.

Replace the comment above `let reminder = "";` with:

```ts
// One line from Rust for the bubble: a task that has become due soon, an imported one that may have
// been handed in (take_reminder in src-tauri/src/tasks.rs), a question about an app, or "take a
// break". Rust sends none of the first three while the keeper is on. It stays until the pet is
// clicked or the keeper next turns on.
```

Delete the whole `function line(s: TimerState): string { ... }`.

In `DriftState`, add after `returned: boolean;`:

```ts
  on: boolean; // the keeper is on: study or work has added up (Keeper in backend/drift.py)
```

and change the initial value to:

```ts
let drift: DriftState = { step: 0, seconds: 0, what: null, returned: false, on: false, mode: null, audio: false };
```

In the `listen<DriftState>("drift", ...)` handler, replace its first four lines (from `const before = drift.step;` through the `if (drift.step === 1 && before === 0 && !calm())` line) with:

```ts
  const before = drift;
  drift = e.payload;
  if (drift.on && !before.on) reminder = ""; // back at work: an old line is out of date
  // step 1: a glance, which is the slime leaning over for a moment
  if (drift.step === 1 && before.step === 0 && !calm())
```

(the `pet.animate(...)` line that followed the old `if` stays as its body), and change the comment `// the Happy state, drawn with the rest in onTimer` to `// the Happy state, drawn with the rest in render`.

Replace the voices block (the comment above `type Voice` through the end of `VOICES`) with:

```ts
// The pet's voice (Settings, "Focus keeper"). Every line is about the time and what it went to;
// none says anything about the user.
type Voice = { bubble: (l: Line) => string; sitting: (l: Line) => string; welcome: string };
type Line = { min: number; what: string | null };
const VOICES: Record<string, Voice> = {
  gentle: {
    bubble: (l) => `${l.min} min on ${l.what}. Ready to get back to it?`,
    sitting: (l) => `${l.min} min on ${l.what} now. I'll sit here until you're back.`,
    welcome: "Welcome back!",
  },
  sassy: {
    bubble: (l) => `${l.what}, ${l.min} min and counting. The work is still waiting.`,
    sitting: (l) => `This is my seat now. ${l.min} min on ${l.what}, and the work misses you.`,
    welcome: "Look who's back. Let's go.",
  },
  roast: {
    bubble: (l) => `${l.min} min of ${l.what}? The work won't finish itself.`,
    sitting: (l) => `${l.what} has had ${l.min} min. I'm sitting here until the work gets some.`,
    welcome: "Finally. Back to it.",
  },
};
```

Replace everything from `/** Steps 2 and up speak, in the chosen voice. */` down to the line before `// The pet earns a star at each streak milestone` with:

```ts
/** Steps 2 and up speak, in the chosen voice. */
function nudge(): string {
  if (awayNote) return awayNote;
  if (welcome) return voice.welcome;
  if (!drift.on || drift.step < 2 || away()) return "";
  const line = { min: Math.floor(drift.seconds / 60), what: drift.what };
  return drift.step === 2 ? voice.bubble(line) : voice.sitting(line);
}

function render() {
  const on = drift.on && !away();
  // P-2: eyes open while the keeper is on, and shut again while something Private is in front
  pet.classList.toggle("watching", on && drift.mode !== "private");
  // the other states (pet.css): lying down, headphones on, rocking at the user, hopping for joy
  pet.classList.toggle("relaxing", !drift.on && drift.mode === "relax");
  pet.classList.toggle("headphones", drift.audio);
  pet.classList.toggle("nudging", on && drift.step >= 2 && !welcome);
  pet.classList.toggle("happy", welcome);
  if (!on) sit(false); // the keeper let go, or the pet was told to look away
  const text = nudge() || reminder;
  bubble.textContent = text;
  if (!text) bubble.hidden = true; // hide before shrinking, show after growing
  resize(!!text).then(() => (bubble.hidden = !text));
}
// once a second: "welcome back", the away note and looking away all end by the clock, not by an event
setInterval(render, 1000);
render();

```

- [ ] **Step 7: The break length in Settings**

In `panel.html`, in the "Focus keeper" section, after the `<label>The pet's voice when you drift ... </label>` element, add:

```html
        <p>
          <label>Suggest a break after
            <input id="break-after" type="number" min="0" max="600" required /> min of study or work</label>
          <span class="muted">0 turns it off.</span>
        </p>
```

In the same section, replace the tick-box paragraph and the muted paragraph under it with:

```html
        <p><label><input id="block" type="checkbox" /> After 10 minutes of drift, block that site for 5 minutes</label></p>
        <p class="muted">
          The pet starts watching by itself once you have studied or worked for 5 minutes. Then it glances at you
          after 30 seconds on something Relax, speaks after 2 minutes and sits in the middle of the screen after 5.
          After 15 minutes it takes that as a break and lets go, and so does 5 minutes away from the laptop.
          Blocking is off unless you tick it, works in Chrome only, and covers the site's pages until the 5 minutes
          are over.
        </p>
```

In `src/panel/main.ts`, replace the block from `type Keeper = ...` through `blockDrift.onchange = ...` with:

```ts
type Keeper = { personality: string; block: boolean; break_after: number };
const personality = $<HTMLSelectElement>("personality");
const blockDrift = $<HTMLInputElement>("block");
const breakAfter = $<HTMLInputElement>("break-after");
const showKeeper = (k: Keeper) => {
  personality.value = k.personality;
  blockDrift.checked = k.block;
  if (document.activeElement !== breakAfter) breakAfter.value = String(k.break_after);
};
personality.onchange = () => run(async () => showKeeper(await invoke<Keeper>("keeper", { personality: personality.value })));
blockDrift.onchange = () => run(async () => showKeeper(await invoke<Keeper>("keeper", { block: blockDrift.checked })));
breakAfter.onchange = () => {
  if (breakAfter.reportValidity())
    run(async () => showKeeper(await invoke<Keeper>("keeper", { breakAfter: breakAfter.valueAsNumber })));
};
```

- [ ] **Step 8: Build, and check by hand in the dev app**

Run: `npm run build`
Expected: no error. `src/timer.ts` is still imported by the panel, so it stays for now.

Run: `npm run tauri dev`. In Settings, set "Suggest a break after" to 6. Then:

1. Keep a Study or Work app in front (a `.edu` page, or an app answered as Study) for 5 minutes. Expected: the pet's eyes open.
2. One minute later. Expected: the Glass sound, and the bubble says "You've been at it for 6 minutes. Take a break?".
3. Put a Relax site in front for 2 minutes. Expected: a glance at 30 seconds, then the bubble names the site.
4. Go back to the Study app. Expected: "Welcome back!" for about 6 seconds.
5. Set the break length back to 50.

- [ ] **Step 9: Commit**

```bash
git add src-tauri/src src/pet/main.ts src/panel/main.ts panel.html
git commit -m "the pet follows the automatic keeper and suggests a break"
```

---

### Task 4: Remove the timer from the core and the panel

**Files:**
- Modify: `src-tauri/src/lib.rs`, `session.rs`, `db.rs`, `modes.rs`, `browser.rs`, `monitor.rs`
- Modify: `src/panel/main.ts`, `panel.html`, `src/panel/panel.css`
- Delete: `src/timer.ts`

**Interfaces:**
- Consumes: `modes::KEEPER_ON` from Task 3.
- Produces: nothing new. Removed commands: `timer`, `list_presets`, `add_preset`, `delete_preset`. Removed event: `timer`.

This task has no new behaviour to test first; its check is that the three check commands pass and the dev app runs.

- [ ] **Step 1: The tick becomes a minute loop**

In `src-tauri/src/lib.rs`:

Change the imports at the top: delete `use serde_json::json;` (put it back if the compiler then reports `json!` missing outside the tests), and change `use session::{play_sound, streak_state, timer_action};` to `use session::streak_state;`. In the `tests` module, change `use serde_json::Value;` to `use serde_json::{json, Value};`.

Replace the whole thread that begins with the comment `// drive the timer once a second; every window hears the new state as a "timer" event` (through its closing `});`) with:

```rust
            // once a minute: reminders, questions, and the streak, so "today" rolls over at midnight
            // while the pet stays open
            let handle = app.handle().clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_secs(60));
                // a task that has become due soon is mentioned by the pet, but never while the keeper
                // is on: it waits, unmarked, and the first check after the user stops picks it up
                if !modes::KEEPER_ON.load(std::sync::atomic::Ordering::Relaxed) {
                    let text = take_reminder(&lock(&handle.state::<Db>().0));
                    match text {
                        Ok(Some(text)) => drop(handle.emit("reminder", text)),
                        // nothing due: an app or site the pet cannot place is asked about, once each
                        Ok(None) => match modes::question(&handle) {
                            Ok(Some(text)) => drop(handle.emit("reminder", text)),
                            Ok(None) => {}
                            Err(e) => eprintln!("mode question failed: {e}"),
                        },
                        Err(e) => eprintln!("reminder check failed: {e}"),
                    }
                }
                if let Err(e) = streak_state(&handle) {
                    eprintln!("streak refresh failed: {e}");
                }
            });
```

In `generate_handler!`, delete the lines `db::list_presets,`, `db::add_preset,`, `db::delete_preset,` and `session::timer,`.

Replace the `.run(|app, event| { ... });` at the end of `run` (the closure with the comment `// quitting stops the timer first`) with:

```rust
        .run(|_, _| {});
```

- [ ] **Step 2: `session.rs`**

Delete `timer_action` and the `timer` command, with their comments. Change the `use crate::db` line to:

```rust
use crate::db::{days_to_total, lock, save_day_total, setting, streak_input, Db};
```

Replace the first section comment with nothing, and change the sounds comment to:

```rust
// ---- Sounds: a macOS system sound when the pet suggests a break ----
```

- [ ] **Step 3: `db.rs`**

Delete the `Preset` struct, `list_presets`, `add_preset`, `delete_preset`, the `Session` struct and `save_session`. Keep the `preset` and `session` tables and the preset seed row in `init_db`. Then run `cd src-tauri && cargo build` and delete each `use` the compiler reports as unused in `db.rs` (expected: `chrono::DateTime`, `serde::{Deserialize, Serialize}`, `tauri::State`).

Above the `CREATE TABLE IF NOT EXISTS preset` line's string, add this comment line before `conn.execute_batch(`:

```rust
    // preset and session are from the focus-block timer, which is gone: preset is no longer read, and
    // session only for days before `auto_since` (streak_input) and old tasks' time (tasks.rs)
```

- [ ] **Step 4: `modes.rs`, `browser.rs`, `monitor.rs`**

In `modes.rs`, delete the section `// ---- S-7: study with no session running, and the pet offers to start one ----` to the end of the file (`OFFER_AFTER_SECS`, `OFFER_EVERY_SECS`, `LAST_OFFER`, `studying`, `offer`).

In `browser.rs`:

- Delete the `PAUSED` static and its doc comment.
- Replace `paused` and its doc comment with:

```rust
/// Nothing is noted while the pet has been told to look away.
pub(crate) fn paused(now: i64) -> bool {
    now < AWAY_UNTIL.load(Ordering::Relaxed)
}
```

- Change the `GAP_SECS` doc comment to `/// Two samples further apart than this are not joined: Chrome lost focus, or the laptop slept.`
- Remove `AtomicBool` from the `use std::sync::atomic` line if the compiler reports it unused.

In `monitor.rs` nothing changes; it already says what `paused` now means.

- [ ] **Step 5: The Rust tests**

In the `tests` module of `src-tauri/src/lib.rs`:

Change the `use db::...` line to `use db::{setting, streak_input};` and add this helper after the `use` lines:

```rust
    /// A finished focus block from before recording was automatic, `secs` long, from epoch second `start`.
    fn old_block(conn: &Connection, start: i64, secs: i64, task_id: Option<i64>) {
        conn.execute(
            // the subquery turns a task that is gone into "no task", as the timer's own insert did
            "INSERT INTO session (started_at, ended_at, planned_min, break_min, task_id)
             VALUES (strftime('%Y-%m-%dT%H:%M:%SZ', ?1, 'unixepoch'), strftime('%Y-%m-%dT%H:%M:%SZ', ?1 + ?2, 'unixepoch'), 50, 10,
                     (SELECT id FROM task WHERE id = ?3))",
            (start, secs, task_id),
        )
        .unwrap();
    }
```

In `db_seeds_presets_once`, delete the last four lines (from `let s = Session {` through the `assert_eq!(ended, ...)`).

In `todays_spans_are_labeled_by_the_python_rules`, delete the block from the comment `// S-7: an offer needs 5 minutes of Study that is still going` through `assert!(!modes::studying(&json!({ "spans": [] }), 0));`.

In `sidecar_ping`, replace the block from `// an error a method raises leaves the process, and the block in progress, alone` through the `assert_eq!(... ["phase"], "focus");` line with:

```rust
        // an error a method raises leaves the process alone
        let backend = Backend::default();
        assert!(sidecar_call(&backend, "quick_add", Some(json!({ "text": "", "today": "2026-10-20" }))).is_err());
        assert_eq!(sidecar_call(&backend, "ping", None).unwrap()["pong"], true);
```

and delete the end of the test body (from `let start = json!({ "action": "start_focus", ...` through `assert_eq!(sessions.len(), 1);`).

In `sessions_tie_to_tasks_typed_merges_into_imported_and_courses_hide`:

- Replace the `let block = |task_id: Option<i64>| Session { ... };` line and the three `save_session` lines after it with:

```rust
        let block = |task_id: Option<i64>| old_block(&conn, 0, 45 * 60, task_id);
        block(Some(hw));
        block(Some(9999));
        block(None);
```

- Change the comment above them to `// a block from the old timer still counts toward its task; one for a task that is gone was kept, untied`.
- Delete the block from the comment `// the task id makes the whole trip: ...` through the `assert_eq!(finished.iter()...` line.
- Change `save_session(&conn, &block(Some(essay))).unwrap();` to `block(Some(essay));`.

Run: `cd src-tauri && cargo test`
Expected: all pass, no warnings.

- [ ] **Step 6: The panel's markup**

In `panel.html`:

- Delete the whole `<section class="tank" aria-label="Session"> ... </section>` and the comment line above it.
- Delete the Settings section that holds `<h2>Presets</h2>`.
- Change the Sounds label to `<label><input id="sounds" type="checkbox" /> Play a sound when the pet suggests a break</label>`.
- In the "Apps and sites" muted paragraph, change `Not during breaks, and only while Study Pet is running.` to `Only while Study Pet is running.`

- [ ] **Step 7: The panel's script**

In `src/panel/main.ts`, delete:

- the line `import { onTimer, timer } from "../timer";`
- the `Preset` type
- the `spent_min` line in the `Task` type
- the consts `focus`, `brk` and `form`
- `const mmss = ...`
- the whole presets section: the comment `// ---- presets (S-1): ...` and `loadPresets`
- the whole session controls section: the comment `// ---- session controls ----` through `$("stop").onclick = ...` (`taskPick`, `openTasks`, `startFocus`, `form.onsubmit`, `$("save").onclick`, the three button handlers)
- in `loadTasks`: the five statements from `openTasks = all.filter(...)` through `showFinished();`
- in `loadTasks`: the `if (!t.done) { const start = ... extras.push(start); }` block
- the whole section `// ---- "Finished it?": ...` (`lastTimer`, `showFinished`, `$("mark-done").onclick`)
- `const titles = ...` and `const level = ...`
- `run(loadPresets);`
- the whole `run(() => onTimer((s) => { ... }));` statement

In `loadTasks`, replace the estimate entry of `details` (the five lines from `t.estimate_min` through `` `${duration(t.spent_min)} spent`, ``) with:

```ts
        t.estimate_min && duration(t.estimate_min),
```

Change the comment `// it ends by itself at midnight, and "relax" ends when a focus block starts` to `// it ends by itself at midnight`.

Change the comment `// ---- the day: today's blocks, the wrap-up and the 7-day strip ----` to `// ---- the day: the streak, the wrap-up and the heatmap ----`.

Delete the file `src/timer.ts`.

- [ ] **Step 8: The panel's styles**

In `src/panel/panel.css`, delete these rules and their comments: `.tank`, `.level`, `.tank > :not(.level)`, `#clock`, `#task-pick`, `#working-on`, `#finished`, and the `@media (prefers-reduced-motion: reduce) { .level { transition: none; } }` block. Change

```css
/* a task's own start button, and the suggestion that it is the same as an imported one */
#tasks .start,
#tasks .merge {
```

to

```css
/* the suggestion that a typed task is the same as an imported one */
#tasks .merge {
```

- [ ] **Step 9: Check everything**

Run all three check commands. Expected: `npm run build` has no error; `cargo test` and the Python tests pass.

Run: `grep -rn "onTimer\|start_focus\|list_presets\|PAUSED\|timer_action" src src-tauri/src panel.html`
Expected: no output.

Run: `npm run tauri dev`. Expected: Today opens on the mode picker with no tank above it; tasks have no ▶ button; Settings has no Presets; the pet still nudges as in Task 3; quitting with Cmd-Q prints no error.

- [ ] **Step 10: Commit**

```bash
git add -A src src-tauri/src panel.html
git commit -m "remove the focus block: timer, presets, breaks and task start"
```

---

### Task 5: Docs

**Files:**
- Modify: `docs/spec.md`, `docs/ARCHITECTURE.md`, `docs/DECISIONS.md`, `docs/CHANGELOG.md`, `docs/ROADMAP.md`, `docs/KNOWN_ISSUES.md`

Read each file's existing entries first and write in the same form and plain wording.

- [ ] **Step 1: `docs/spec.md`**

- "Pet, sessions and breaks": mark S-1, S-2, S-3, S-4, S-5 and S-7 as `Removed 2026-10-13`, each with "replaced by automatic recording". Reword S-6 to "All Study and Work time counts toward the day. There is no minimum stretch." Add two rows:
  - `S-8 | The pet counts Study and Work time by itself, from the labels in "Activity modes". There is nothing to start. | Built`
  - `S-9 | After a run of study or work (50 minutes by default, set in Settings, 0 for never), the pet suggests a break once, with a sound. | Built`
- C-4: reword to "Noting pauses when the pet is told to look away, and after 5 minutes with nobody at the keyboard."
- "Streaks": a day counts once it has the threshold in minutes of Study and Work.
- "Wrap up and recap": the heatmap is blue for a day with more Study, orange for more Work.
- "Focus keeper": it turns on after 5 minutes of Study or Work, and off after 15 minutes of Relax in a row, 5 minutes away, or "I'm relaxing".
- "A day with the pet": remove the step where the user starts a session.

- [ ] **Step 2: `docs/ARCHITECTURE.md`**

- The diagram: remove `TIMER["timer.py"]` and its arrow; rename `TICK` to `"1-minute loop"`; add `DRIFT["drift.py"]` with the same arrow as the others; the events are `streak` and `drift`.
- The layer table: the sidecar's job is "Streak and keeper rules, quick-add parsing, mode labels".
- The table list: eleven tables. Add `day_total | A finished day's Study and Work seconds, saved once; emptied when an app rule changes | Yes`. `preset`: "Not read. Left from the focus-block timer". `session`: "Focus blocks from before 2026-10-13. Read for the streak and heatmap of days before `auto_since`, and for an old task's time". `setting`: add `auto_since` and `break_after_min`.
- "How the layers talk": the command list loses `timer`, `list_presets`, `add_preset`, `delete_preset`; the events are `streak`, `tasks`, `reminder`, `drift`, `keeper`, `away`; the sidecar methods are `ping`, `streak`, `modes`, `watch`, `quick_add` (and, until the everyday app is reinstalled, `timer` and `drift`); "Sounds" is "Glass when the pet suggests a break, played by the monitor thread".
- Replace "One example: a focus block, start to finish" with "One example: a day counts by itself":
  1. Every 5 seconds the monitor stores the front app and asks `watch` in the sidecar.
  2. `Keeper` adds up Study and Work time, turns on at 5 minutes, and runs the ladder.
  3. Once a minute Rust labels today (`modes_day`), saves any finished day that has no row in `day_total`, and sends minutes per day to `streak`.
  4. The `streak` event redraws the panel.
- "A fourth example": remove step 3 (the break pause) and the words "the focus blocks" in step 4; step 5 says "while the keeper is off".
- The parts table: "Timer and streak" becomes "Streak", built with "Recorded Study and Work time".

- [ ] **Step 3: `docs/DECISIONS.md`**

Add a section at the end, `## 2026-10-13 · Recording is automatic`, with these entries, each in the file's usual form (the choice, why, what it costs):

- **The focus block is removed; Study and Work time counts by itself.** Why: a day with no Start press counted as nothing. Cost: the streak is as good as the mode labels, and an unanswered app is uncounted time.
- **A finished day's totals are saved; today is labeled live.** Why: the streak is asked for every minute and labeling all of history each time grows without end. Cost: one more table, emptied on every rule change. This replaces "Modes are worked out when asked for, not stored on the spans" for the two totals only; spans are still not labeled in storage.
- **Days before the switch keep their focus-block minutes.** Why: the user's streak and heatmap must not change on the day of the update. Cost: two rules for a day's minutes, split at `auto_since`.
- **The keeper turns itself on after 5 minutes and off after a break.** This replaces "The focus keeper is on during a focus block only". Give the numbers (300, 900, 300 seconds) and the reason for each.
- **Time per task is dropped.** Why: the pet cannot tell which task an app belongs to. Old blocks still shrink an old task's work left.
- **The heatmap has two ramps.** Give the final eight orange values from Task 2, Step 10, and say they passed the same check as the blue ones.
- **The sidecar's `timer` and `drift` stay until the everyday app is reinstalled.** Why: item 33 in `KNOWN_ISSUES.md`.

Mark the old entry "The focus keeper is on during a focus block only, and drift is what is Relax" as replaced on 2026-10-13, the way the extension entry was marked on 2026-10-07.

- [ ] **Step 4: `docs/CHANGELOG.md`, `docs/ROADMAP.md`, `docs/KNOWN_ISSUES.md`**

- `CHANGELOG.md`: one entry dated 2026-10-13: recording is automatic; removed the focus timer, presets, breaks countdown, task Start and time per task; added the break suggestion and its setting; the heatmap shows Study or Work.
- `ROADMAP.md`: add the change as done, and one open item: "Remove `timer` and `drift` from the sidecar when the everyday app is next reinstalled (plan Task 6)."
- `KNOWN_ISSUES.md`: add these items in the file's form (what, cost if left, fix):
  - An unanswered app is uncounted time. Fix: answer the pet's question; past days are counted again.
  - "I'm relaxing" keeps the keeper off until midnight, since no focus block takes it back.
  - `earned` only goes up while the keeper is off (the `ponytail:` note in `backend/drift.py`).
  - A rule change labels every finished day since `auto_since` again.
  - The everyday app runs the old timer against a `backend/` that still has it only until Task 6; update item 33 to say so.
  - Remove or reword any item about the timer losing a running block on a crash, and the `ponytail:` note it points to.

- [ ] **Step 5: Commit**

```bash
git add docs
git commit -m "docs: recording is automatic"
```

---

### Task 6: Remove the timer from the sidecar (only when the user asks for a reinstall)

**Do not start this task without the user asking.** It breaks the everyday app in `/Applications` at once, until that app is rebuilt from this branch and reinstalled. The reinstall itself follows `KNOWN_ISSUES.md`, items 34 and 62: back up the everyday `pet.db` first, named with the time.

**Files:**
- Modify: `backend/main.py`
- Delete: `backend/timer.py`, `backend/tests/test_timer.py`
- Modify: `docs/ARCHITECTURE.md`, `docs/ROADMAP.md`, `docs/KNOWN_ISSUES.md`

- [ ] **Step 1: Check nothing in the dev app calls them**

Run: `grep -rn '"timer"\|"drift"' src-tauri/src | grep -v 'emit("drift"\|listen'`
Expected: no line that passes `"timer"` or `"drift"` to `sidecar_call` or `call`.

- [ ] **Step 2: `backend/main.py`**

Delete `from backend.timer import Timer`, `_timer`, `TIMER_ACTIONS`, the `timer` function, the `modes` function, `_drift`, and the `drift` function. Change the drift import to `from backend.drift import Keeper, is_drift, is_music`. `METHODS` becomes:

```python
METHODS = {
    "ping": ping,
    "streak": lambda params: streak(**params),
    "modes": lambda params: label_day(**params),
    "watch": watch,
    "quick_add": lambda params: parse_quick_add(**params),
}
```

In the module docstring, replace the last line with `Spec PR-6: this process must stay offline.`

- [ ] **Step 3: Delete the timer**

```bash
git rm backend/timer.py backend/tests/test_timer.py
```

- [ ] **Step 4: Check**

Run all three check commands. Expected: all pass.

Run: `echo '{"method": "timer", "params": {"action": "tick"}}' | backend/.venv/bin/python -m backend.main`
Expected: `{"error": "KeyError('timer')"}`

- [ ] **Step 5: Docs, then commit**

In `ARCHITECTURE.md` remove "(and, until the everyday app is reinstalled, `timer` and `drift`)". In `ROADMAP.md` mark the open item done. In `KNOWN_ISSUES.md` remove the note added to item 33 in Task 5.

```bash
git add -A backend docs
git commit -m "sidecar: remove the timer and the old drift method"
```

Then tell the user the everyday app must be rebuilt and reinstalled now, and wait for them to ask for it.
