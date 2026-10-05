use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Mutex;

use rusqlite::{Connection, OptionalExtension};
use chrono::DateTime;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};

// ---- SQLite store ----

struct Db(Mutex<Connection>);

// ponytail: plain SQLite. Spec PR-4 wants encryption at rest: swap rusqlite's
// "bundled" feature for "bundled-sqlcipher" + a keychain key before Phase 4 stores captured text.
fn init_db(conn: &Connection) -> rusqlite::Result<()> {
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
        );",
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
struct Preset {
    id: i64,
    name: String,
    focus_min: i64,
    break_min: i64,
}

#[tauri::command]
fn list_presets(db: State<Db>) -> Result<Vec<Preset>, String> {
    let conn = db.0.lock().unwrap();
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
fn add_preset(db: State<Db>, focus_min: i64, break_min: i64) -> Result<(), String> {
    if focus_min < 1 || break_min < 1 {
        return Err("focus and break must be at least 1 minute".into());
    }
    db.0.lock()
        .unwrap()
        .execute(
            "INSERT OR IGNORE INTO preset (name, focus_min, break_min) VALUES (?1, ?2, ?3)",
            (format!("{focus_min}/{break_min}"), focus_min, break_min),
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn delete_preset(db: State<Db>, id: i64) -> Result<(), String> {
    db.0.lock()
        .unwrap()
        .execute("DELETE FROM preset WHERE id = ?1", [id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// A finished focus block, as reported by backend/timer.py. Times are epoch seconds.
#[derive(Deserialize)]
struct Session {
    started_at: f64,
    ended_at: f64,
    planned_min: f64,
    break_min: f64,
    label: Option<String>,
}

fn save_session(conn: &Connection, s: &Session) -> rusqlite::Result<()> {
    let iso = |secs: f64| DateTime::from_timestamp(secs as i64, 0).map(|t| t.to_rfc3339());
    conn.execute(
        "INSERT INTO session (started_at, ended_at, planned_min, break_min, label) VALUES (?1, ?2, ?3, ?4, ?5)",
        (iso(s.started_at), iso(s.ended_at), s.planned_min, s.break_min, &s.label),
    )?;
    Ok(())
}

/// What backend/streaks.py needs: study minutes per local day, today's date, the threshold.
// ponytail: a block that runs past midnight counts wholly toward the day it started.
fn streak_input(conn: &Connection) -> rusqlite::Result<Value> {
    let mut minutes = serde_json::Map::new();
    let mut stmt = conn.prepare(
        "SELECT date(started_at, 'localtime'), sum(unixepoch(ended_at) - unixepoch(started_at)) / 60.0
         FROM session WHERE ended_at IS NOT NULL GROUP BY 1",
    )?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        minutes.insert(row.get(0)?, json!(row.get::<_, f64>(1)?));
    }
    let today: String = conn.query_row("SELECT date('now', 'localtime')", [], |r| r.get(0))?;
    let threshold: Option<f64> = conn
        .query_row("SELECT value FROM setting WHERE key = 'streak_threshold_min'", [], |r| r.get(0))
        .optional()?;
    Ok(json!({ "minutes": minutes, "today": today, "threshold": threshold.unwrap_or(25.0) }))
}

/// Everything the panel shows about the day: today's focus blocks plus the streak input
/// (minutes per day, for the 7-day strip).
fn day_data(conn: &Connection) -> rusqlite::Result<Value> {
    let mut stmt = conn.prepare(
        "SELECT unixepoch(started_at), unixepoch(ended_at) - unixepoch(started_at), planned_min
         FROM session
         WHERE ended_at IS NOT NULL AND date(started_at, 'localtime') = date('now', 'localtime')
         ORDER BY started_at",
    )?;
    let sessions = stmt
        .query_map([], |r| {
            Ok(json!({
                "started": r.get::<_, i64>(0)?,
                "seconds": r.get::<_, i64>(1)?,
                "planned_min": r.get::<_, f64>(2)?,
            }))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut data = streak_input(conn)?;
    data["sessions"] = json!(sessions);
    Ok(data)
}

#[tauri::command]
fn day(db: State<Db>) -> Result<Value, String> {
    day_data(&db.0.lock().unwrap()).map_err(|e| e.to_string())
}

// ---- Python sidecar: one JSON object per line over stdin/stdout ----

struct Sidecar {
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    _child: Child, // exits on its own when our end of stdin closes
}

#[derive(Default)]
struct Backend(Mutex<Option<Sidecar>>);

fn spawn_sidecar() -> std::io::Result<Sidecar> {
    // ponytail: dev-only paths into the source tree. For a distributable build,
    // freeze backend/ with PyInstaller and ship it via tauri.conf.json `bundle.externalBin`.
    // Run as a module from the project root so `from backend.x import ...` resolves.
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut child = Command::new(root.join("backend/.venv/bin/python"))
        .args(["-m", "backend.main"])
        .current_dir(&root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    Ok(Sidecar {
        stdin: child.stdin.take().unwrap(),
        stdout: BufReader::new(child.stdout.take().unwrap()),
        _child: child,
    })
}

fn call(sidecar: &mut Sidecar, method: &str, params: Option<Value>) -> Result<Value, String> {
    writeln!(sidecar.stdin, "{}", json!({ "method": method, "params": params }))
        .map_err(|e| e.to_string())?;
    let mut line = String::new();
    sidecar.stdout.read_line(&mut line).map_err(|e| e.to_string())?;
    let mut res: Value =
        serde_json::from_str(&line).map_err(|e| format!("bad sidecar reply {line:?}: {e}"))?;
    match res.get("error") {
        Some(err) => Err(err.to_string()),
        None => Ok(res["ok"].take()),
    }
}

// ponytail: one sidecar behind one lock, so a long job (graph building) would stall the
// timer tick. Give long jobs their own process when Phase 5 adds them.
fn sidecar_call(backend: &Backend, method: &str, params: Option<Value>) -> Result<Value, String> {
    let mut guard = backend.0.lock().unwrap();
    if guard.is_none() {
        *guard = Some(spawn_sidecar().map_err(|e| format!("can't start python backend: {e}"))?);
    }
    let res = call(guard.as_mut().unwrap(), method, params);
    if res.is_err() {
        *guard = None; // respawn on next call in case the process died
    }
    res
}

// async: runs off the main thread, since sidecar jobs can be slow
#[tauri::command(async)]
fn backend_call(
    backend: State<Backend>,
    method: String,
    params: Option<Value>,
) -> Result<Value, String> {
    sidecar_call(&backend, &method, params)
}

// ---- Session timer: logic in backend/timer.py, storage and fan-out to the windows here ----

fn timer_action(app: &AppHandle, params: Value) -> Result<Value, String> {
    let mut state = sidecar_call(&app.state::<Backend>(), "timer", Some(params))?;
    let sessions: Vec<Session> =
        serde_json::from_value(state["sessions"].take()).map_err(|e| e.to_string())?;
    if !sessions.is_empty() {
        {
            let db = app.state::<Db>();
            let conn = db.0.lock().unwrap();
            for s in &sessions {
                save_session(&conn, s).map_err(|e| e.to_string())?;
            }
        }
        streak_state(app)?;
    }
    app.emit("timer", &state).map_err(|e| e.to_string())?;
    Ok(state)
}

/// `params` is `{ action: "start_focus" | "skip_break" | "resume" | "stop" | "tick", ...args }`.
#[tauri::command(async)]
fn timer(app: AppHandle, params: Value) -> Result<Value, String> {
    timer_action(&app, params)
}

// ---- Streak: logic in backend/streaks.py ----

/// Computes the streak and broadcasts it to every window as a "streak" event.
fn streak_state(app: &AppHandle) -> Result<Value, String> {
    let input = {
        let db = app.state::<Db>();
        let conn = db.0.lock().unwrap();
        streak_input(&conn).map_err(|e| e.to_string())?
    };
    let state = sidecar_call(&app.state::<Backend>(), "streak", Some(input))?;
    app.emit("streak", &state).map_err(|e| e.to_string())?;
    Ok(state)
}

#[tauri::command(async)]
fn streak(app: AppHandle) -> Result<Value, String> {
    streak_state(&app)
}

#[tauri::command(async)]
fn set_streak_threshold(app: AppHandle, minutes: f64) -> Result<Value, String> {
    if !(minutes >= 1.0 && minutes <= 24.0 * 60.0) {
        return Err("threshold must be between 1 minute and 24 hours".into());
    }
    app.state::<Db>()
        .0
        .lock()
        .unwrap()
        .execute(
            "INSERT OR REPLACE INTO setting (key, value) VALUES ('streak_threshold_min', ?1)",
            [minutes],
        )
        .map_err(|e| e.to_string())?;
    streak_state(&app)
}

// ---- Tasks: quick-add parsing in backend/tasks.py ----

/// A typed line after backend/tasks.py has parsed it.
#[derive(Serialize, Deserialize)]
struct NewTask {
    title: String,
    course: Option<String>,
    due_date: Option<String>,
    estimate_min: Option<i64>,
}

fn insert_task(conn: &Connection, t: &NewTask) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO task (title, course, due_date, estimate_min) VALUES (?1, ?2, ?3, ?4)",
        (&t.title, &t.course, &t.due_date, t.estimate_min),
    )?;
    Ok(conn.last_insert_rowid())
}

/// `text` is one quick-add line, such as "STAT 310 HW3 Fri 2h". Returns the new task with its id.
#[tauri::command(async)]
fn add_task(db: State<Db>, backend: State<Backend>, text: String) -> Result<Value, String> {
    let today: String = db
        .0
        .lock()
        .unwrap()
        .query_row("SELECT date('now', 'localtime')", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    let parsed = sidecar_call(&backend, "quick_add", Some(json!({ "text": text, "today": today })))?;
    let task: NewTask = serde_json::from_value(parsed).map_err(|e| e.to_string())?;
    let id = insert_task(&db.0.lock().unwrap(), &task).map_err(|e| e.to_string())?;
    let mut task = json!(task);
    task["id"] = json!(id);
    Ok(task)
}

/// A row of the panel's task list. `due` is a local "YYYY-MM-DD", whichever due column is set.
#[derive(Serialize)]
struct Task {
    id: i64,
    title: String,
    course: Option<String>,
    due: Option<String>,
    estimate_min: Option<i64>,
    done: bool,
    source: Option<String>, // where it was imported from, such as "canvas"
    removed: bool,          // imported, and no longer in its source's feed
}

/// Open tasks by due date (undated last), then the ones checked off today.
fn tasks(conn: &Connection) -> rusqlite::Result<Vec<Task>> {
    conn.prepare(
        "SELECT id, title, course, coalesce(due_date, date(due_at, 'localtime')) AS due, estimate_min, status = 'done',
                (SELECT min(source) FROM task_source_link WHERE task_id = task.id),
                EXISTS (SELECT 1 FROM task_source_link WHERE task_id = task.id AND removed_at IS NOT NULL)
         FROM task
         WHERE hidden_at IS NULL AND (status = 'todo' OR date(done_at, 'localtime') = date('now', 'localtime'))
         ORDER BY status = 'done', due IS NULL, due, id",
    )?
    .query_map([], |r| {
        Ok(Task {
            id: r.get(0)?,
            title: r.get(1)?,
            course: r.get(2)?,
            due: r.get(3)?,
            estimate_min: r.get(4)?,
            done: r.get(5)?,
            source: r.get(6)?,
            removed: r.get(7)?,
        })
    })?
    .collect()
}

fn set_done(conn: &Connection, id: i64, done: bool) -> rusqlite::Result<usize> {
    conn.execute(
        "UPDATE task SET status = iif(?2, 'done', 'todo'),
                         done_at = iif(?2, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), NULL)
         WHERE id = ?1",
        (id, done),
    )
}

#[tauri::command]
fn list_tasks(db: State<Db>) -> Result<Vec<Task>, String> {
    tasks(&db.0.lock().unwrap()).map_err(|e| e.to_string())
}

#[tauri::command]
fn set_task_done(db: State<Db>, id: i64, done: bool) -> Result<(), String> {
    match set_done(&db.0.lock().unwrap(), id, done) {
        Ok(0) => Err(format!("no task with id {id}")),
        Ok(_) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

/// Deletes a task the user typed. An imported one is only hidden: deleting it would drop its source
/// link, and the next sync would import it again as new.
fn remove_task(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    let hidden = conn.execute(
        "UPDATE task SET hidden_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
         WHERE id = ?1 AND EXISTS (SELECT 1 FROM task_source_link WHERE task_id = ?1)",
        [id],
    )?;
    if hidden == 0 {
        conn.execute("DELETE FROM task WHERE id = ?1", [id])?;
    }
    Ok(())
}

#[tauri::command]
fn delete_task(db: State<Db>, id: i64) -> Result<(), String> {
    remove_task(&db.0.lock().unwrap(), id).map_err(|e| e.to_string())
}

// ---- Imports: feeds parsed by backend/feeds.py ----

/// One item of a parsed feed, as backend/feeds.py returns it.
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum FeedItem {
    Task { uid: String, title: String, course: Option<String>, due_at: Option<String>, due_date: Option<String> },
    Event { uid: String, title: String, start_at: String, end_at: String, all_day: bool },
}

/// Stores one whole feed from `source`. `now` is a UTC "YYYY-MM-DDTHH:MM:SSZ".
/// A task already imported keeps its row and its done status; only what the source owns is updated.
/// A task missing from the feed is flagged, not deleted, and the flag clears if it comes back.
/// A task first seen after its due day is not imported: the feed cannot say whether it was handed in.
fn sync_feed(conn: &Connection, source: &str, items: &[FeedItem], now: &str) -> rusqlite::Result<()> {
    let tx = conn.unchecked_transaction()?;
    // events have no state of their own, so the feed's copy replaces ours
    tx.execute("DELETE FROM calendar_event WHERE source = ?1", [source])?;
    for item in items {
        match item {
            FeedItem::Task { uid, title, course, due_at, due_date } => {
                let linked: Option<i64> = tx
                    .query_row(
                        "SELECT task_id FROM task_source_link WHERE source = ?1 AND source_uid = ?2",
                        (source, uid),
                        |r| r.get(0),
                    )
                    .optional()?;
                match linked {
                    Some(id) => {
                        tx.execute(
                            "UPDATE task SET title = ?2, course = ?3, due_at = ?4, due_date = ?5 WHERE id = ?1",
                            (id, title, course, due_at, due_date),
                        )?;
                        tx.execute(
                            "UPDATE task_source_link SET last_seen_at = ?3, removed_at = NULL
                             WHERE source = ?1 AND source_uid = ?2",
                            (source, uid, now),
                        )?;
                    }
                    None => {
                        let due_day = due_date.as_deref().or(due_at.as_deref()).and_then(|d| d.get(..10));
                        if matches!((due_day, now.get(..10)), (Some(due), Some(today)) if due < today) {
                            continue;
                        }
                        tx.execute(
                            "INSERT INTO task (title, course, due_at, due_date) VALUES (?1, ?2, ?3, ?4)",
                            (title, course, due_at, due_date),
                        )?;
                        tx.execute(
                            "INSERT INTO task_source_link (task_id, source, source_uid, last_seen_at)
                             VALUES (?1, ?2, ?3, ?4)",
                            (tx.last_insert_rowid(), source, uid, now),
                        )?;
                    }
                }
            }
            FeedItem::Event { uid, title, start_at, end_at, all_day } => {
                tx.execute(
                    "INSERT OR REPLACE INTO calendar_event (source, uid, title, start_at, end_at, all_day)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    (source, uid, title, start_at, end_at, all_day),
                )?;
            }
        }
    }
    // Not seen this time: removed at the source. Past items are left alone, since a feed drops them on its own.
    tx.execute(
        "UPDATE task_source_link SET removed_at = ?2
         WHERE source = ?1 AND removed_at IS NULL AND last_seen_at < ?2
           AND task_id IN (SELECT id FROM task WHERE coalesce(due_date, substr(due_at, 1, 10)) >= substr(?2, 1, 10))",
        (source, now),
    )?;
    tx.commit()
}

const SOURCES: [&str; 2] = ["canvas", "google"];

/// Held for a whole sync, so two never overlap and a feed is not changed in the middle of one.
#[derive(Default)]
struct Syncing(Mutex<()>);

fn setting(conn: &Connection, key: &str) -> rusqlite::Result<Option<String>> {
    conn.query_row("SELECT value FROM setting WHERE key = ?1", [key], |r| r.get(0)).optional()
}

/// What the `setting` table holds in place of a feed address: the address itself is in the Keychain (PR-4).
const IN_KEYCHAIN: &str = "keychain";

/// A source and its feed address, or why the address could not be read.
type FeedUrl = (&'static str, Result<String, String>);

/// Each saved feed address, read from the Keychain.
fn feed_urls(conn: &Connection, service: &str) -> Result<Vec<FeedUrl>, String> {
    let saved = saved_feeds(conn, "feed_url_").map_err(|e| e.to_string())?;
    Ok(saved
        .into_iter()
        .map(|(source, value)| {
            if value != IN_KEYCHAIN {
                return (source, Ok(value)); // saved before the Keychain was used, and not moved yet
            }
            let url = keychain_get(service, &format!("feed_url_{source}"))
                .and_then(|url| url.ok_or_else(|| "the address is missing from the Keychain; paste it again".to_string()));
            (source, url)
        })
        .collect())
}

/// Moves addresses an older build left in the `setting` table into the Keychain. Run at launch.
/// One that can't be moved stays where it is and still works; the next launch tries again.
fn move_feed_urls_to_keychain(conn: &Connection, service: &str) -> rusqlite::Result<()> {
    for (source, url) in saved_feeds(conn, "feed_url_")? {
        if url != IN_KEYCHAIN && keychain_set(service, &format!("feed_url_{source}"), &url).is_ok() {
            conn.execute("UPDATE setting SET value = ?2 WHERE key = ?1", (format!("feed_url_{source}"), IN_KEYCHAIN))?;
        }
    }
    Ok(())
}

/// The .ics files the user uploaded, as text. A source has an address or a file, never both.
fn feed_files(conn: &Connection) -> rusqlite::Result<Vec<(&'static str, String)>> {
    saved_feeds(conn, "feed_file_")
}

fn saved_feeds(conn: &Connection, prefix: &str) -> rusqlite::Result<Vec<(&'static str, String)>> {
    let mut saved = Vec::new();
    for source in SOURCES {
        if let Some(value) = setting(conn, &format!("{prefix}{source}"))? {
            saved.push((source, value));
        }
    }
    Ok(saved)
}

/// Saves a feed address in the Keychain, or forgets the feed when `url` is empty. Imported tasks stay either way.
fn save_feed_url(conn: &Connection, service: &str, source: &str, url: &str) -> Result<(), String> {
    if !SOURCES.contains(&source) {
        return Err(format!("unknown feed {source}"));
    }
    let lower = url.to_lowercase();
    if !(url.is_empty() || lower.starts_with("https://") || lower.starts_with("webcal://")) {
        return Err("a feed address must start with https://".into());
    }
    let account = format!("feed_url_{source}");
    if url.is_empty() {
        keychain_delete(service, &account);
        return replace_feed(conn, source, "feed_url_", "").map_err(|e| e.to_string());
    }
    keychain_set(service, &account, url)?;
    replace_feed(conn, source, "feed_url_", IN_KEYCHAIN).map_err(|e| e.to_string())
}

const MAX_ICS_BYTES: usize = 5_000_000;

/// Saves an uploaded .ics file's text in place of an address. It is parsed again on every sync,
/// so repeating events keep rolling forward, but it only changes when the user uploads a new one.
fn save_feed_file(conn: &Connection, source: &str, ics: &str) -> Result<(), String> {
    if !SOURCES.contains(&source) {
        return Err(format!("unknown feed {source}"));
    }
    if ics.trim().is_empty() {
        return Err("that file is empty".into());
    }
    if ics.len() > MAX_ICS_BYTES {
        return Err("that file is larger than 5 MB".into());
    }
    replace_feed(conn, source, "feed_file_", ics).map_err(|e| e.to_string())
}

/// Clears whatever the source had (address, file, account marker, last status), then saves `value` unless it is empty.
fn replace_feed(conn: &Connection, source: &str, prefix: &str, value: &str) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM setting WHERE key IN (?1, ?2, ?3, ?4)",
        (format!("feed_url_{source}"), format!("feed_file_{source}"), format!("feed_account_{source}"), format!("feed_status_{source}")),
    )?;
    if value.is_empty() {
        conn.execute("DELETE FROM calendar_event WHERE source = ?1", [source])?;
    } else {
        conn.execute("INSERT INTO setting (key, value) VALUES (?1, ?2)", (format!("{prefix}{source}"), value))?;
    }
    Ok(())
}

/// What Settings shows per feed: whether an address, a file or an account is saved (never its contents) and how the last sync went.
fn feed_list(conn: &Connection) -> rusqlite::Result<Vec<Value>> {
    SOURCES
        .iter()
        .map(|source| {
            let status = setting(conn, &format!("feed_status_{source}"))?;
            let has = |prefix: &str| conn.query_row("SELECT count(*) FROM setting WHERE key = ?1", [format!("{prefix}{source}")], |r| r.get::<_, bool>(0));
            Ok(json!({
                "source": source,
                "set": has("feed_url_")? || has("feed_file_")? || has("feed_account_")?,
                "file": has("feed_file_")?,
                "account": has("feed_account_")?,
                "status": status.and_then(|s| serde_json::from_str::<Value>(&s).ok()),
            }))
        })
        .collect()
}

// ---- Google by sign-in (docs/google_calendar_oauth.md) ----

/// Where pet.db lives; google_client.json sits beside it.
struct DataDir(PathBuf);

const GOOGLE_TOKEN: &str = "google_refresh_token";

/// Secrets (the Google refresh token, the feed addresses) live in the macOS Keychain, reached through the
/// `security` tool that ships with macOS. `service` is the app identifier, so the dev and everyday apps
/// keep their own; `account` names the secret.
/// Our app is unsigned, so a keychain library would ask for the login password after every rebuild.
/// ponytail: any program running as the user can ask `security` for a secret; sign the app and use a
/// keychain library to tie them to this app alone.
fn keychain(args: &[&str]) -> Result<std::process::Output, String> {
    Command::new("security").args(args).output().map_err(|_| "could not reach the Keychain".to_string())
}

fn keychain_get(service: &str, account: &str) -> Result<Option<String>, String> {
    let out = keychain(&["find-generic-password", "-s", service, "-a", account, "-w"])?;
    match out.status.code() {
        Some(0) => Ok(Some(String::from_utf8_lossy(&out.stdout).trim().to_string())),
        Some(44) => Ok(None), // errSecItemNotFound
        _ => Err("could not reach the Keychain".into()),
    }
}

fn keychain_set(service: &str, account: &str, secret: &str) -> Result<(), String> {
    // the names go into a command line for `security -i`, so nothing that could end or split it gets through;
    // the secret goes in as hex, so it can hold anything (an address has ? and & in it)
    let plain = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c));
    if !plain(service) || !plain(account) || secret.is_empty() {
        return Err("could not reach the Keychain".into());
    }
    let hex: String = secret.bytes().map(|b| format!("{b:02x}")).collect();
    // on stdin, so the secret never shows in `ps`
    let mut child = Command::new("security")
        .arg("-i")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "could not reach the Keychain".to_string())?;
    let line = format!("add-generic-password -U -s {service} -a {account} -X {hex}\n");
    child.stdin.take().unwrap().write_all(line.as_bytes()).map_err(|_| "could not reach the Keychain".to_string())?;
    child.wait().map_err(|_| "could not reach the Keychain".to_string())?;
    // `security -i` exits 0 even when a command in it fails, so read the secret back
    if keychain_get(service, account)?.as_deref() == Some(secret) {
        Ok(())
    } else {
        Err("could not reach the Keychain".into())
    }
}

fn keychain_delete(service: &str, account: &str) {
    let _ = keychain(&["delete-generic-password", "-s", service, "-a", account]);
}

/// The OAuth client from the Cloud console's download: `{ "client_id", "client_secret" }`.
fn google_client(dir: &Path) -> Result<Value, String> {
    let path = dir.join("google_client.json");
    let missing = || format!("put the Google client file at {}", path.display());
    let file: Value = serde_json::from_str(&std::fs::read_to_string(&path).map_err(|_| missing())?).map_err(|_| missing())?;
    match (file["installed"]["client_id"].as_str(), file["installed"]["client_secret"].as_str()) {
        (Some(id), Some(secret)) => Ok(json!({ "client_id": id, "client_secret": secret })),
        _ => Err(format!("{} is not a \"Desktop app\" client file", path.display())),
    }
}

/// What the fetcher needs to read the signed-in account's calendar.
fn google_account(app: &AppHandle) -> Result<Value, String> {
    let mut account = google_client(&app.state::<DataDir>().0)?;
    let token = keychain_get(&app.config().identifier, GOOGLE_TOKEN)?.ok_or("Google sign-in has expired; sign in again")?;
    account["refresh_token"] = json!(token);
    Ok(account)
}

/// Runs backend/fetcher.py once: the only process that touches the network (PR-6).
/// `request` is `{ "feeds": { source: address }, "files": { source: ics text }, "accounts": { source: client and token } }`,
/// or `{ "connect": { source: client } }` to sign in. It goes in on stdin, so no secret shows in `ps`.
fn run_fetcher(request: &Value) -> Result<Value, String> {
    // ponytail: dev-only path into the source tree, like spawn_sidecar
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut child = Command::new(root.join("backend/.venv/bin/python"))
        .args(["-m", "backend.fetcher"])
        .current_dir(&root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("can't start the task fetcher: {e}"))?;
    // dropping stdin closes it, which is how the fetcher knows the request is complete
    child.stdin.take().unwrap().write_all(request.to_string().as_bytes()).map_err(|e| e.to_string())?;
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        // the last line of a Python traceback names the error, such as a missing package
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(format!("the task fetcher failed: {}", stderr.lines().last().unwrap_or("no output")));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("bad fetcher reply: {e}"))
}

/// Stores one source's reply from the fetcher and returns the status Settings shows.
/// A failed download stores nothing: treating it as an empty feed would flag every upcoming task as removed.
fn store_feed(conn: &Connection, source: &str, reply: &Value, now: &str) -> Value {
    let stored = match reply.get("ok") {
        Some(items) => serde_json::from_value::<Vec<FeedItem>>(items.clone())
            .map_err(|e| format!("bad fetcher reply: {e}"))
            .and_then(|items| sync_feed(conn, source, &items, now).map_err(|e| e.to_string())),
        None => Err(reply["error"].as_str().unwrap_or("the fetcher sent nothing for this feed").to_string()),
    };
    if let Err(error) = stored {
        return json!({ "at": now, "error": error });
    }
    let count = |sql: &str| conn.query_row(sql, [source], |r| r.get::<_, i64>(0)).unwrap_or(0);
    json!({
        "at": now,
        "tasks": count("SELECT count(*) FROM task_source_link WHERE source = ?1"),
        "events": count("SELECT count(*) FROM calendar_event WHERE source = ?1"),
    })
}

/// Downloads every saved feed and stores it, then tells the windows with a "tasks" event.
/// The database is not locked while the fetcher is on the network.
fn sync_feeds(app: &AppHandle) -> Result<(), String> {
    let syncing = app.state::<Syncing>();
    let _one_at_a_time = syncing.0.lock().unwrap();
    let db = app.state::<Db>();
    let (urls, files, signed_in) = {
        let conn = db.0.lock().unwrap();
        (
            feed_urls(&conn, &app.config().identifier)?,
            feed_files(&conn).map_err(|e| e.to_string())?,
            setting(&conn, "feed_account_google").map_err(|e| e.to_string())?.is_some(),
        )
    };
    // a feed whose secret can't be read (no Keychain item, no client file) fails by itself; the others still sync
    let mut failed: Vec<(&str, String)> = Vec::new();
    let mut feeds = serde_json::Map::new();
    for (source, url) in urls {
        match url {
            Ok(url) => drop(feeds.insert(source.into(), json!(url))),
            Err(error) => failed.push((source, error)),
        }
    }
    let mut accounts = serde_json::Map::new();
    if signed_in {
        match google_account(app) {
            Ok(account) => drop(accounts.insert("google".into(), account)),
            Err(error) => failed.push(("google", error)),
        }
    }
    let files: serde_json::Map<String, Value> = files.into_iter().map(|(s, v)| (s.to_string(), json!(v))).collect();
    let asked: Vec<String> = feeds.keys().chain(files.keys()).chain(accounts.keys()).cloned().collect();
    if !(asked.is_empty() && failed.is_empty()) {
        let reply = if asked.is_empty() {
            Ok(json!({}))
        } else {
            run_fetcher(&json!({ "feeds": feeds, "files": files, "accounts": accounts }))
        };
        let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let conn = db.0.lock().unwrap();
        let mut statuses: Vec<(String, Value)> = failed.into_iter().map(|(s, error)| (s.to_string(), json!({ "at": now, "error": error }))).collect();
        for source in asked {
            let status = match &reply {
                Ok(reply) => store_feed(&conn, &source, &reply[&source], &now),
                Err(error) => json!({ "at": now, "error": error }),
            };
            statuses.push((source, status));
        }
        for (source, status) in statuses {
            conn.execute(
                "INSERT OR REPLACE INTO setting (key, value) VALUES (?1, ?2)",
                (format!("feed_status_{source}"), status.to_string()),
            )
            .map_err(|e| e.to_string())?;
        }
    }
    app.emit("tasks", ()).map_err(|e| e.to_string())
}

#[tauri::command]
fn feeds(db: State<Db>) -> Result<Vec<Value>, String> {
    feed_list(&db.0.lock().unwrap()).map_err(|e| e.to_string())
}

/// Saves the address for "canvas" or "google" and syncs straight away. An empty `url` forgets the feed.
#[tauri::command(async)]
fn set_feed(app: AppHandle, source: String, url: String) -> Result<(), String> {
    {
        let syncing = app.state::<Syncing>();
        let _one_at_a_time = syncing.0.lock().unwrap();
        let db = app.state::<Db>();
        let conn = db.0.lock().unwrap();
        save_feed_url(&conn, &app.config().identifier, &source, url.trim())?;
    }
    if source == "google" {
        keychain_delete(&app.config().identifier, GOOGLE_TOKEN); // an address, or none, takes the account's place
    }
    sync_feeds(&app)
}

/// Saves the text of an uploaded .ics file for "canvas" or "google" and syncs straight away.
#[tauri::command(async)]
fn set_feed_file(app: AppHandle, source: String, ics: String) -> Result<(), String> {
    {
        let syncing = app.state::<Syncing>();
        let _one_at_a_time = syncing.0.lock().unwrap();
        let db = app.state::<Db>();
        let conn = db.0.lock().unwrap();
        save_feed_file(&conn, &source, &ics)?;
    }
    // the file takes the place of an address or an account, so their secrets go
    keychain_delete(&app.config().identifier, &format!("feed_url_{source}"));
    if source == "google" {
        keychain_delete(&app.config().identifier, GOOGLE_TOKEN);
    }
    sync_feeds(&app)
}

/// Signs in to Google through the browser, keeps the refresh token in the Keychain, and syncs.
/// No lock is held while the browser is open, so a sign-in nobody finishes does not block the half-hourly sync.
#[tauri::command(async)]
fn connect_google(app: AppHandle) -> Result<(), String> {
    let client = google_client(&app.state::<DataDir>().0)?;
    let reply = run_fetcher(&json!({ "connect": { "google": client } }))?;
    let token = reply["google"]["ok"]["refresh_token"]
        .as_str()
        .ok_or_else(|| reply["google"]["error"].as_str().unwrap_or("Sign-in was not completed").to_string())?;
    keychain_set(&app.config().identifier, GOOGLE_TOKEN, token)?;
    keychain_delete(&app.config().identifier, "feed_url_google"); // the account takes the address's place
    {
        let syncing = app.state::<Syncing>();
        let _one_at_a_time = syncing.0.lock().unwrap();
        let db = app.state::<Db>();
        let conn = db.0.lock().unwrap();
        replace_feed(&conn, "google", "feed_account_", "1").map_err(|e| e.to_string())?;
    }
    sync_feeds(&app)
}

#[tauri::command(async)]
fn sync_now(app: AppHandle) -> Result<(), String> {
    sync_feeds(&app)
}

/// Today's calendar events for the panel's timeline: all-day ones first, then by start time.
fn events_today(conn: &Connection) -> rusqlite::Result<Vec<Value>> {
    // ponytail: a timed event that started yesterday and runs past midnight is not shown today
    conn.prepare(
        "SELECT title, start_at, end_at, all_day, source FROM calendar_event
         WHERE (all_day AND start_at <= date('now', 'localtime')
                AND (end_at > date('now', 'localtime') OR start_at = date('now', 'localtime')))
            OR (NOT all_day AND date(start_at, 'localtime') = date('now', 'localtime'))
         ORDER BY all_day DESC, start_at, title",
    )?
    .query_map([], |r| {
        Ok(json!({
            "title": r.get::<_, String>(0)?,
            "start_at": r.get::<_, String>(1)?,
            "end_at": r.get::<_, String>(2)?,
            "all_day": r.get::<_, bool>(3)?,
            "source": r.get::<_, String>(4)?,
        }))
    })?
    .collect()
}

#[tauri::command]
fn list_events(db: State<Db>) -> Result<Vec<Value>, String> {
    events_today(&db.0.lock().unwrap()).map_err(|e| e.to_string())
}

// ---- Windows ----

#[cfg(target_os = "macos")]
fn move_to_active_space(window: &tauri::WebviewWindow) {
    use objc2_app_kit::{NSWindow, NSWindowCollectionBehavior};
    if let Ok(ptr) = window.ns_window() {
        let ns_window = unsafe { &*(ptr as *const NSWindow) };
        let behavior = ns_window.collectionBehavior();
        ns_window.setCollectionBehavior(behavior | NSWindowCollectionBehavior::MoveToActiveSpace);
    }
}

#[tauri::command]
fn toggle_panel(app: AppHandle) -> tauri::Result<()> {
    let panel = app.get_webview_window("panel").expect("panel window in tauri.conf.json");
    if panel.is_visible()? {
        panel.hide()
    } else {
        panel.show()?;
        panel.set_focus()
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(Backend::default())
        .manage(Syncing::default())
        .setup(|app| {
            // dev builds keep pet.db in the project root, where it is easy to inspect;
            // release builds use the OS app data dir
            let dir = if cfg!(debug_assertions) {
                Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
            } else {
                app.path().app_data_dir()?
            };
            std::fs::create_dir_all(&dir)?;
            let conn = Connection::open(dir.join("pet.db"))?;
            init_db(&conn)?;
            move_feed_urls_to_keychain(&conn, &app.config().identifier)?;
            app.manage(Db(Mutex::new(conn)));
            app.manage(DataDir(dir));

            // drive the timer once a second; every window hears the new state as a "timer" event
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                for second in 1u64.. {
                    std::thread::sleep(std::time::Duration::from_secs(1));
                    if let Err(e) = timer_action(&handle, json!({ "action": "tick" })) {
                        eprintln!("timer tick failed: {e}");
                    }
                    // once a minute, so "today" rolls over at midnight while the pet stays open
                    if second % 60 == 0 {
                        if let Err(e) = streak_state(&handle) {
                            eprintln!("streak refresh failed: {e}");
                        }
                    }
                }
            });

            // import the saved feeds at launch, then every half hour
            // ponytail: thread::sleep does not count time asleep, so after the lid opens the next sync
            // can be up to 30 minutes away. "Sync now" in Settings covers it; sync on wake if that hurts.
            let handle = app.handle().clone();
            std::thread::spawn(move || loop {
                if let Err(e) = sync_feeds(&handle) {
                    eprintln!("feed sync failed: {e}");
                }
                std::thread::sleep(std::time::Duration::from_secs(30 * 60));
            });

            // P-1: one shortcut hides/shows the pet
            #[cfg(desktop)]
            app.handle().plugin(
                tauri_plugin_global_shortcut::Builder::new()
                    .with_shortcuts(["alt+shift+p"])?
                    .with_handler(|app, _shortcut, event| {
                        if event.state != tauri_plugin_global_shortcut::ShortcutState::Pressed {
                            return;
                        }
                        if let Some(pet) = app.get_webview_window("pet") {
                            let _ = if pet.is_visible().unwrap_or(false) {
                                pet.hide()
                            } else {
                                pet.show()
                            };
                        }
                    })
                    .build(),
            )?;

            #[cfg(target_os = "macos")]
            if let Some(panel) = app.get_webview_window("panel") {
                move_to_active_space(&panel);
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // closing the panel only hides it, so the pet can reopen it
            if window.label() == "panel" {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            list_presets,
            add_preset,
            delete_preset,
            day,
            backend_call,
            timer,
            streak,
            set_streak_threshold,
            add_task,
            list_tasks,
            set_task_done,
            delete_task,
            feeds,
            set_feed,
            set_feed_file,
            connect_google,
            sync_now,
            list_events,
            toggle_panel
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_seeds_presets_once() {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        init_db(&conn).unwrap();
        let n: i64 = conn.query_row("SELECT count(*) FROM preset", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 2);

        let s = Session { started_at: 0.0, ended_at: 1500.0, planned_min: 25.0, break_min: 5.0, label: None };
        save_session(&conn, &s).unwrap();
        let ended: String = conn.query_row("SELECT ended_at FROM session", [], |r| r.get(0)).unwrap();
        assert_eq!(ended, "1970-01-01T00:25:00+00:00");
    }

    #[test]
    fn task_tables_block_duplicates_and_follow_deletes() {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        init_db(&conn).unwrap();
        conn.execute("INSERT INTO task (title) VALUES ('HW3')", []).unwrap();
        let link = "INSERT INTO task_source_link (task_id, source, source_uid, last_seen_at) VALUES (1, 'canvas', 'a1', '2026-01-01T00:00:00Z')";
        conn.execute(link, []).unwrap();
        assert!(conn.execute(link, []).is_err());
        assert!(conn.execute("UPDATE task SET status = 'nope'", []).is_err());

        conn.execute("INSERT INTO session (started_at, planned_min, break_min, task_id) VALUES ('2026-01-01T00:00:00Z', 25, 5, 1)", []).unwrap();
        conn.execute("DELETE FROM task", []).unwrap();
        let links: i64 = conn.query_row("SELECT count(*) FROM task_source_link", [], |r| r.get(0)).unwrap();
        let task_id: Option<i64> = conn.query_row("SELECT task_id FROM session", [], |r| r.get(0)).unwrap();
        assert_eq!((links, task_id), (0, None));
    }

    #[test]
    fn task_list_orders_by_due_and_keeps_todays_done() {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        let add = |title: &str, due: Option<&str>| {
            let t = NewTask { title: title.into(), course: None, due_date: due.map(Into::into), estimate_min: None };
            insert_task(&conn, &t).unwrap()
        };
        let (undated, late, soon) = (add("undated", None), add("late", Some("2026-12-01")), add("soon", Some("2026-11-01")));
        let order = || tasks(&conn).unwrap().iter().map(|t| (t.id, t.done)).collect::<Vec<_>>();
        assert_eq!(order(), [(soon, false), (late, false), (undated, false)]);

        assert_eq!(set_done(&conn, soon, true).unwrap(), 1);
        assert_eq!(order(), [(late, false), (undated, false), (soon, true)]);
        // done on an earlier day: off the list
        conn.execute("UPDATE task SET done_at = '2020-01-01T00:00:00Z' WHERE id = ?1", [soon]).unwrap();
        assert_eq!(order(), [(late, false), (undated, false)]);

        set_done(&conn, soon, false).unwrap();
        assert_eq!(order()[0], (soon, false));
        assert_eq!(set_done(&conn, 99, true).unwrap(), 0);
    }

    #[test]
    fn feed_sync_updates_in_place_and_flags_removed() {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        // the shape backend/feeds.py returns
        let feed = |v: Value| serde_json::from_value::<Vec<FeedItem>>(v).unwrap();
        let hw = |title: &str, due_at: &str| {
            json!({ "kind": "task", "source": "canvas", "uid": "event-assignment-1", "title": title,
                    "course": "STATISTC 315", "due_at": due_at, "due_date": null })
        };
        let all_day = |uid: &str, due: &str| {
            json!({ "kind": "task", "source": "canvas", "uid": uid, "title": uid, "course": null, "due_at": null, "due_date": due })
        };
        let event = |title: &str| {
            json!({ "kind": "event", "source": "canvas", "uid": "event-calendar-event-1", "title": title,
                    "start_at": "2026-10-06T14:00:00Z", "end_at": "2026-10-06T15:00:00Z", "all_day": false })
        };
        // "long ago" was due before the first sync, so it is never imported
        let first = feed(json!([hw("Homework 1", "2026-10-09T20:00:00Z"), all_day("quiz", "2026-10-12"), all_day("old", "2026-09-08"),
                                all_day("long ago", "2026-08-01"), event("Office hours")]));
        sync_feed(&conn, "canvas", &first, "2026-09-01T12:00:00Z").unwrap();
        sync_feed(&conn, "canvas", &first, "2026-10-04T12:30:00Z").unwrap();
        let count = |table: &str| conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get::<_, i64>(0)).unwrap();
        assert_eq!((count("task"), count("task_source_link"), count("calendar_event")), (3, 3, 1));

        // checked off here, then edited at the source; "quiz" and "old" leave the feed
        set_done(&conn, 1, true).unwrap();
        let second = feed(json!([hw("Homework 1 (revised)", "2026-10-16T20:00:00Z"), event("Office hours, moved")]));
        sync_feed(&conn, "canvas", &second, "2026-10-04T13:00:00Z").unwrap();
        let row: (String, String, String) = conn
            .query_row("SELECT title, due_at, status FROM task WHERE id = 1", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap();
        assert_eq!(row, ("Homework 1 (revised)".into(), "2026-10-16T20:00:00Z".into(), "done".into()));
        let removed = |uid: &str| {
            conn.query_row("SELECT removed_at FROM task_source_link WHERE source_uid = ?1", [uid], |r| r.get::<_, Option<String>>(0))
                .unwrap()
        };
        assert_eq!(removed("event-assignment-1"), None);
        assert_eq!(removed("quiz"), Some("2026-10-04T13:00:00Z".into()));
        assert_eq!(removed("old"), None); // already past: the feed aged it out
        let title: String = conn.query_row("SELECT title FROM calendar_event", [], |r| r.get(0)).unwrap();
        assert_eq!((count("task"), title), (3, "Office hours, moved".into()));
        let listed = tasks(&conn).unwrap();
        let quiz = listed.iter().find(|t| t.title == "quiz").unwrap();
        assert_eq!((quiz.source.as_deref(), quiz.removed), (Some("canvas"), true));

        // back in the feed: the flag clears
        sync_feed(&conn, "canvas", &first, "2026-10-04T14:00:00Z").unwrap();
        assert_eq!((removed("quiz"), count("task")), (None, 3));

        // another source's feed leaves these alone
        sync_feed(&conn, "google", &[], "2026-10-04T15:00:00Z").unwrap();
        assert_eq!((removed("quiz"), count("calendar_event")), (None, 1));
    }

    #[test]
    fn failed_download_stores_nothing_and_deleted_import_stays_gone() {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        let now = "2026-10-04T12:00:00Z";
        let ok = json!({ "ok": [
            { "kind": "task", "source": "canvas", "uid": "event-assignment-1", "title": "Quiz", "course": null, "due_at": null, "due_date": "2026-10-12" }
        ] });
        assert_eq!(store_feed(&conn, "canvas", &ok, now), json!({ "at": now, "tasks": 1, "events": 0 }));

        // a failed download is reported and flags nothing as removed
        let failed = store_feed(&conn, "canvas", &json!({ "error": "the server answered 404" }), "2026-10-04T13:00:00Z");
        assert_eq!(failed["error"], "the server answered 404");
        assert_eq!(store_feed(&conn, "canvas", &Value::Null, now)["error"], "the fetcher sent nothing for this feed");
        assert!(!tasks(&conn).unwrap()[0].removed);

        // deleting an imported task hides it, and the next sync does not bring it back
        remove_task(&conn, 1).unwrap();
        store_feed(&conn, "canvas", &ok, "2026-10-04T14:00:00Z");
        let rows: i64 = conn.query_row("SELECT count(*) FROM task", [], |r| r.get(0)).unwrap();
        assert_eq!((tasks(&conn).unwrap().len(), rows), (0, 1));
        // a typed task is really deleted
        let typed = insert_task(&conn, &NewTask { title: "typed".into(), course: None, due_date: None, estimate_min: None }).unwrap();
        remove_task(&conn, typed).unwrap();
        assert_eq!(conn.query_row("SELECT count(*) FROM task", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
    }

    #[test]
    fn feed_addresses_and_todays_events() {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        // addresses go to the real login Keychain, under a service name only this test uses
        let kc = "com.hiennguyen.pet.test-feeds";
        let urls = |conn: &Connection| feed_urls(conn, kc).unwrap();
        let secret = "https://example.com/secret.ics?user=a&key=b%20c";
        assert!(save_feed_url(&conn, kc, "canvas", "file:///etc/passwd").is_err());
        assert!(save_feed_url(&conn, kc, "nope", "https://example.com/a.ics").is_err());
        save_feed_url(&conn, kc, "google", secret).unwrap();
        assert_eq!(urls(&conn), [("google", Ok(secret.to_string()))]);
        // the database holds a marker, never the address
        assert_eq!(setting(&conn, "feed_url_google").unwrap().as_deref(), Some(IN_KEYCHAIN));

        // an address an older build left in the database still works, and is moved at launch
        conn.execute("INSERT INTO setting (key, value) VALUES ('feed_url_canvas', 'https://example.com/old.ics')", []).unwrap();
        assert_eq!(urls(&conn)[0], ("canvas", Ok("https://example.com/old.ics".to_string())));
        move_feed_urls_to_keychain(&conn, kc).unwrap();
        assert_eq!(setting(&conn, "feed_url_canvas").unwrap().as_deref(), Some(IN_KEYCHAIN));
        assert_eq!(urls(&conn)[0], ("canvas", Ok("https://example.com/old.ics".to_string())));
        // a marker with nothing behind it is that feed's error, not a crash
        keychain_delete(kc, "feed_url_canvas");
        assert_eq!(urls(&conn)[0], ("canvas", Err("the address is missing from the Keychain; paste it again".to_string())));
        save_feed_url(&conn, kc, "canvas", "").unwrap();
        // Settings is told a feed is saved, never its address
        let list = json!(feed_list(&conn).unwrap());
        assert_eq!(
            list,
            json!([
                { "source": "canvas", "set": false, "file": false, "account": false, "status": null },
                { "source": "google", "set": true, "file": false, "account": false, "status": null }
            ])
        );
        // an uploaded file takes the address's place, and an address takes it back
        assert!(save_feed_file(&conn, "google", "  ").is_err());
        assert!(save_feed_file(&conn, "google", &"x".repeat(MAX_ICS_BYTES + 1)).is_err());
        save_feed_file(&conn, "google", "BEGIN:VCALENDAR").unwrap();
        assert_eq!((urls(&conn).len(), feed_files(&conn).unwrap()), (0, vec![("google", "BEGIN:VCALENDAR".to_string())]));
        assert_eq!(json!(feed_list(&conn).unwrap())[1], json!({ "source": "google", "set": true, "file": true, "account": false, "status": null }));
        // signing in takes the file's place: a source holds one of address, file or account
        replace_feed(&conn, "google", "feed_account_", "1").unwrap();
        assert_eq!(json!(feed_list(&conn).unwrap())[1], json!({ "source": "google", "set": true, "file": false, "account": true, "status": null }));
        assert_eq!((urls(&conn).len(), feed_files(&conn).unwrap().len()), (0, 0));
        save_feed_url(&conn, kc, "google", secret).unwrap();
        assert_eq!((urls(&conn).len(), feed_files(&conn).unwrap().len()), (1, 0));
        assert_eq!(json!(feed_list(&conn).unwrap())[1]["account"], false);

        // starts now, so it lands on today whatever the local time is
        let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let today: String = conn.query_row("SELECT date('now', 'localtime')", [], |r| r.get(0)).unwrap();
        let add = |uid: &str, start: &str, end: &str, all_day: bool| {
            conn.execute(
                "INSERT INTO calendar_event (source, uid, title, start_at, end_at, all_day) VALUES ('google', ?1, ?1, ?2, ?3, ?4)",
                (uid, start, end, all_day),
            )
            .unwrap();
        };
        add("lecture", &now, &now, false);
        add("holiday", &today, &today, true);
        add("last year", "2025-01-01T10:00:00Z", "2025-01-01T11:00:00Z", false);
        add("old all-day", "2025-01-01", "2025-01-01", true);
        let titles: Vec<Value> = events_today(&conn).unwrap().iter().map(|e| e["title"].clone()).collect();
        assert_eq!(titles, ["holiday", "lecture"]);

        // forgetting a feed drops its events
        save_feed_url(&conn, kc, "google", "").unwrap();
        let events: i64 = conn.query_row("SELECT count(*) FROM calendar_event", [], |r| r.get(0)).unwrap();
        assert_eq!((urls(&conn).len(), events), (0, 0));
        assert_eq!(keychain_get(kc, "feed_url_google"), Ok(None)); // and the Keychain item with it
        // disconnecting clears an account too
        replace_feed(&conn, "google", "feed_account_", "1").unwrap();
        save_feed_url(&conn, kc, "google", "").unwrap();
        assert_eq!(json!(feed_list(&conn).unwrap())[1], json!({ "source": "google", "set": false, "file": false, "account": false, "status": null }));

        // an uploaded file goes through the same fetcher, with no download
        let google = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../backend/tests/google.ics")).unwrap();
        let reply = run_fetcher(&json!({ "files": { "google": google, "canvas": "not a calendar" } })).unwrap();
        assert_eq!(reply["canvas"]["error"], "that is not a calendar this app can read");
        // the fixture's dates may be outside the window by now, so only the shape is checked
        assert!(store_feed(&conn, "google", &reply["google"], &now).get("error").is_none());
    }

    /// The whole import against a real feed, into a throwaway database:
    ///   CANVAS_ICS_URL=https://... cargo test -- --ignored
    #[test]
    #[ignore = "needs the network and CANVAS_ICS_URL"]
    fn real_canvas_feed_round_trip() {
        let url = std::env::var("CANVAS_ICS_URL").expect("set CANVAS_ICS_URL to a Canvas calendar feed address");
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let reply = run_fetcher(&json!({ "feeds": { "canvas": url, "google": "https://example.com/missing.ics" } })).unwrap();
        assert!(reply["google"]["error"].is_string());
        for _ in 0..2 {
            let status = store_feed(&conn, "canvas", &reply["canvas"], &now);
            assert_eq!(status.get("error"), None);
            println!("{status}, {} of them listed", tasks(&conn).unwrap().len());
        }
    }

    #[test]
    fn streak_counts_a_day_with_enough_study() {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        // starts now, so it lands on today whatever the local time is
        let now = chrono::Utc::now().timestamp() as f64;
        let s = Session { started_at: now, ended_at: now + 30.0 * 60.0, planned_min: 30.0, break_min: 5.0, label: None };
        save_session(&conn, &s).unwrap();

        let day = day_data(&conn).unwrap();
        assert_eq!(day["sessions"][0]["seconds"], 1800);
        assert_eq!(day["minutes"][day["today"].as_str().unwrap()], 30.0);

        let mut sidecar = spawn_sidecar().unwrap();
        let state = call(&mut sidecar, "streak", Some(streak_input(&conn).unwrap())).unwrap();
        assert_eq!((&state["days"], &state["today_minutes"]), (&json!(1), &json!(30)));

        conn.execute("INSERT INTO setting VALUES ('streak_threshold_min', 45)", []).unwrap();
        let state = call(&mut sidecar, "streak", Some(streak_input(&conn).unwrap())).unwrap();
        assert_eq!(state["days"], 0);
    }

    #[test]
    fn sidecar_ping() {
        let mut sidecar = spawn_sidecar().unwrap();
        assert_eq!(call(&mut sidecar, "ping", None).unwrap()["pong"], true);
        assert!(call(&mut sidecar, "nope", None).is_err());

        let line = json!({ "text": "STAT 310 HW3 2026-10-23 2h", "today": "2026-10-20" });
        let task: NewTask = serde_json::from_value(call(&mut sidecar, "quick_add", Some(line)).unwrap()).unwrap();
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        assert_eq!(insert_task(&conn, &task).unwrap(), 1);
        let row: (String, String, String, i64, String) = conn
            .query_row("SELECT title, course, due_date, estimate_min, status FROM task", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })
            .unwrap();
        assert_eq!(row, ("HW3".into(), "STAT 310".into(), "2026-10-23".into(), 120, "todo".into()));
        assert!(call(&mut sidecar, "quick_add", Some(json!({ "text": "fri", "today": "2026-10-20" }))).is_err());

        let start = json!({ "action": "start_focus", "focus_min": 25, "break_min": 5 });
        assert_eq!(call(&mut sidecar, "timer", Some(start)).unwrap()["phase"], "focus");
        let mut stopped = call(&mut sidecar, "timer", Some(json!({ "action": "stop" }))).unwrap();
        assert_eq!(stopped["phase"], "idle");
        let sessions: Vec<Session> = serde_json::from_value(stopped["sessions"].take()).unwrap();
        assert_eq!(sessions.len(), 1);
    }

    /// Writes to the real login Keychain under a service name of its own, then removes it.
    #[test]
    #[cfg(target_os = "macos")]
    fn keychain_round_trip_and_client_file() {
        let service = "com.hiennguyen.pet.test";
        let get = || keychain_get(service, GOOGLE_TOKEN);
        keychain_delete(service, GOOGLE_TOKEN);
        assert_eq!(get(), Ok(None));
        keychain_set(service, GOOGLE_TOKEN, "1//0g-Abc_def.123").unwrap();
        keychain_set(service, GOOGLE_TOKEN, "1//second").unwrap(); // a new sign-in replaces the old token
        assert_eq!(get(), Ok(Some("1//second".into())));
        // a secret may hold anything a command line would choke on
        let awkward = r#"https://x.example/a.ics?b=1&c="d" $HOME;e"#;
        keychain_set(service, GOOGLE_TOKEN, awkward).unwrap();
        assert_eq!(get(), Ok(Some(awkward.into())));
        // a name may not: nothing that could add a second command to the line
        for bad in ["", "a b", "a\nadd-generic-password", "a\"b", "a;b"] {
            assert!(keychain_set(service, bad, "x").is_err(), "{bad:?}");
            assert!(keychain_set(bad, GOOGLE_TOKEN, "x").is_err(), "{bad:?}");
        }
        assert!(keychain_set(service, GOOGLE_TOKEN, "").is_err());
        assert_eq!(get(), Ok(Some(awkward.into())));
        keychain_delete(service, GOOGLE_TOKEN);
        assert_eq!(get(), Ok(None));

        let dir = std::env::temp_dir().join("pet-google-client-test");
        std::fs::create_dir_all(&dir).unwrap();
        let _ = std::fs::remove_file(dir.join("google_client.json"));
        assert!(google_client(&dir).unwrap_err().starts_with("put the Google client file at "));
        std::fs::write(dir.join("google_client.json"), r#"{"web":{"client_id":"id"}}"#).unwrap();
        assert!(google_client(&dir).unwrap_err().ends_with("is not a \"Desktop app\" client file"));
        std::fs::write(dir.join("google_client.json"), r#"{"installed":{"client_id":"id","client_secret":"s","project_id":"p"}}"#).unwrap();
        assert_eq!(google_client(&dir).unwrap(), json!({ "client_id": "id", "client_secret": "s" }));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
