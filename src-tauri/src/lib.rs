use std::io::{BufRead, BufReader, Write};
use std::path::Path;
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
        CREATE TABLE IF NOT EXISTS setting (key TEXT PRIMARY KEY, value);",
    )?;
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version < 1 {
        conn.execute_batch("ALTER TABLE session ADD COLUMN label TEXT; PRAGMA user_version = 1;")?;
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

// ---- Windows ----

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
            app.manage(Db(Mutex::new(conn)));

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

        let start = json!({ "action": "start_focus", "focus_min": 25, "break_min": 5 });
        assert_eq!(call(&mut sidecar, "timer", Some(start)).unwrap()["phase"], "focus");
        let mut stopped = call(&mut sidecar, "timer", Some(json!({ "action": "stop" }))).unwrap();
        assert_eq!(stopped["phase"], "idle");
        let sessions: Vec<Session> = serde_json::from_value(stopped["sessions"].take()).unwrap();
        assert_eq!(sessions.len(), 1);
    }
}
