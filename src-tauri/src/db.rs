use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, PoisonError};

use rusqlite::{Connection, OptionalExtension};
use chrono::DateTime;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::State;

// ---- SQLite store ----

pub(crate) struct Db(pub(crate) Mutex<Connection>);

/// Takes a lock, also after a thread panicked while holding it: each use of the connection is one
/// statement or one transaction, which rolls back, so nothing is left half done. Without this, one
/// panic would make every later call panic too.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

// ponytail: plain SQLite. Spec PR-4 wants encryption at rest: swap rusqlite's
// "bundled" feature for "bundled-sqlcipher" + a keychain key before Phase 4 stores captured text.
pub(crate) fn init_db(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS preset (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL UNIQUE,
            focus_min INTEGER NOT NULL,
            break_min INTEGER NOT NULL
        );
        INSERT OR IGNORE INTO preset (name, focus_min, break_min) VALUES ('25/5', 25, 5), ('50/10', 50, 10);
        CREATE TABLE IF NOT EXISTS session (
            id INTEGER PRIMARY KEY,
            started_at TEXT NOT NULL,
            ended_at TEXT,
            planned_min INTEGER NOT NULL,
            break_min INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS setting (key TEXT PRIMARY KEY, value);
        CREATE TABLE IF NOT EXISTS task (
            id INTEGER PRIMARY KEY,
            title TEXT NOT NULL,
            course TEXT,
            due_at TEXT,   -- UTC timestamp, for items due at a time
            due_date TEXT, -- YYYY-MM-DD, for all-day items
            estimate_min INTEGER,
            status TEXT NOT NULL DEFAULT 'todo' CHECK (status IN ('todo', 'done')),
            done_at TEXT,
            planned_for TEXT, -- YYYY-MM-DD, the day it is on the Today list
            created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
            reminded_at TEXT,
            CHECK (due_at IS NULL OR due_date IS NULL)
        );
        CREATE TABLE IF NOT EXISTS task_source_link (
            id INTEGER PRIMARY KEY,
            task_id INTEGER NOT NULL REFERENCES task(id) ON DELETE CASCADE,
            source TEXT NOT NULL, -- such as 'canvas'
            source_uid TEXT NOT NULL,
            last_seen_at TEXT NOT NULL,
            removed_at TEXT,
            UNIQUE (source, source_uid)
        );
        CREATE TABLE IF NOT EXISTS calendar_event (
            id INTEGER PRIMARY KEY,
            source TEXT NOT NULL,
            uid TEXT NOT NULL,
            title TEXT NOT NULL,
            start_at TEXT NOT NULL, -- UTC timestamp, or YYYY-MM-DD when all_day
            end_at TEXT NOT NULL,
            all_day INTEGER NOT NULL DEFAULT 0,
            UNIQUE (source, uid)
        );
        CREATE TABLE IF NOT EXISTS activity_span (
            id INTEGER PRIMARY KEY,
            started_at TEXT NOT NULL, -- UTC timestamp
            ended_at TEXT NOT NULL,
            domain TEXT NOT NULL,
            url TEXT NOT NULL,        -- without its #fragment
            title TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS app_span (
            id INTEGER PRIMARY KEY,
            started_at TEXT NOT NULL, -- UTC timestamp
            ended_at TEXT NOT NULL,
            app TEXT NOT NULL,        -- the name the Dock shows
            bundle_id TEXT            -- such as 'com.apple.Safari'; some programs have none
        );
        CREATE TABLE IF NOT EXISTS audio_span (
            id INTEGER PRIMARY KEY,   -- PR-11: that sound was on, and when; never what was playing
            started_at TEXT NOT NULL, -- UTC timestamp
            ended_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS mode_declaration (
            id INTEGER PRIMARY KEY,
            started_at TEXT NOT NULL, -- UTC timestamp
            ended_at TEXT,            -- NULL while it is in force
            mode TEXT NOT NULL CHECK (mode IN ('study', 'work', 'relax'))
        );
        CREATE TABLE IF NOT EXISTS app_rule (
            key TEXT PRIMARY KEY,     -- a site, or an app's bundle ID (its name if it has none), in lower case
            name TEXT NOT NULL,       -- as shown to the user
            mode TEXT CHECK (mode IN ('study', 'work', 'relax', 'private')), -- NULL: asked, not answered yet
            asked_at TEXT             -- when the pet brought it up; NULL if the user answered unasked
        );
        CREATE TABLE IF NOT EXISTS day_total (
            day TEXT PRIMARY KEY,        -- a finished local day, YYYY-MM-DD
            study_secs INTEGER NOT NULL, -- as backend/modes.py labeled it; every row goes when a rule changes
            work_secs INTEGER NOT NULL
        );
        -- the first day recorded time counts toward the streak; earlier days keep their focus blocks
        INSERT OR IGNORE INTO setting (key, value) VALUES ('auto_since', date('now', 'localtime'));",
    )?;
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version < 1 {
        conn.execute_batch("ALTER TABLE session ADD COLUMN label TEXT; PRAGMA user_version = 1;")?;
    }
    if version < 2 {
        conn.execute_batch(
            "ALTER TABLE session ADD COLUMN task_id INTEGER REFERENCES task(id) ON DELETE SET NULL;
             PRAGMA user_version = 2;",
        )?;
    }
    if version < 3 {
        // set when the user deletes an imported task: the row stays, so the next sync cannot bring it back
        conn.execute_batch("ALTER TABLE task ADD COLUMN hidden_at TEXT; PRAGMA user_version = 3;")?;
    }
    Ok(())
}

#[derive(Serialize)]
pub(crate) struct Preset {
    id: i64,
    name: String,
    focus_min: i64,
    break_min: i64,
}

#[tauri::command]
pub(crate) fn list_presets(db: State<Db>) -> Result<Vec<Preset>, String> {
    let conn = lock(&db.0);
    let mut stmt = conn
        .prepare("SELECT id, name, focus_min, break_min FROM preset ORDER BY id")
        .map_err(|e| e.to_string())?;
    let presets = stmt
        .query_map([], |r| {
            Ok(Preset {
                id: r.get(0)?,
                name: r.get(1)?,
                focus_min: r.get(2)?,
                break_min: r.get(3)?,
            })
        })
        .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
        .map_err(|e| e.to_string())?;
    Ok(presets)
}

#[tauri::command]
pub(crate) fn add_preset(db: State<Db>, focus_min: i64, break_min: i64) -> Result<(), String> {
    if focus_min < 1 || break_min < 1 {
        return Err("focus and break must be at least 1 minute".into());
    }
    lock(&db.0)
        .execute(
            "INSERT OR IGNORE INTO preset (name, focus_min, break_min) VALUES (?1, ?2, ?3)",
            (format!("{focus_min}/{break_min}"), focus_min, break_min),
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub(crate) fn delete_preset(db: State<Db>, id: i64) -> Result<(), String> {
    lock(&db.0)
        .execute("DELETE FROM preset WHERE id = ?1", [id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// A finished focus block, as reported by backend/timer.py. Times are epoch seconds.
#[derive(Deserialize)]
pub(crate) struct Session {
    pub(crate) started_at: f64,
    pub(crate) ended_at: f64,
    pub(crate) planned_min: f64,
    pub(crate) break_min: f64,
    pub(crate) label: Option<String>,
    #[serde(default)]
    pub(crate) task_id: Option<i64>, // S-2: the task the block was for
}

pub(crate) fn save_session(conn: &Connection, s: &Session) -> rusqlite::Result<()> {
    let iso = |secs: f64| DateTime::from_timestamp(secs as i64, 0).map(|t| t.to_rfc3339());
    conn.execute(
        // the subquery turns a task deleted or merged away mid-block into "no task", so the block is still kept
        "INSERT INTO session (started_at, ended_at, planned_min, break_min, label, task_id)
         VALUES (?1, ?2, ?3, ?4, ?5, (SELECT id FROM task WHERE id = ?6))",
        (iso(s.started_at), iso(s.ended_at), s.planned_min, s.break_min, &s.label, s.task_id),
    )?;
    Ok(())
}

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

/// Held for a whole sync, so two never overlap and a feed is not changed in the middle of one.
#[derive(Default)]
pub(crate) struct Syncing(pub(crate) Mutex<()>);

pub(crate) fn setting(conn: &Connection, key: &str) -> rusqlite::Result<Option<String>> {
    conn.query_row("SELECT value FROM setting WHERE key = ?1", [key], |r| r.get(0)).optional()
}

/// Where pet.db lives; google_client.json sits beside it.
pub(crate) struct DataDir(pub(crate) PathBuf);
