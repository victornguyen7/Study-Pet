use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager};

use crate::db::{lock, save_session, setting, streak_input, Db, Session};
use crate::sidecar::{sidecar_call, Backend};

// ---- Session timer: logic in backend/timer.py, storage and fan-out to the windows here ----

pub(crate) fn timer_action(app: &AppHandle, params: Value) -> Result<Value, String> {
    let mut state = sidecar_call(&app.state::<Backend>(), "timer", Some(params))?;
    let sessions: Vec<Session> =
        serde_json::from_value(state["sessions"].take()).map_err(|e| e.to_string())?;
    if !sessions.is_empty() {
        {
            let db = app.state::<Db>();
            let conn = lock(&db.0);
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
pub(crate) fn timer(app: AppHandle, params: Value) -> Result<Value, String> {
    // starting a focus block takes back "I'm relaxing", which would otherwise keep the keeper off
    if params["action"] == "start_focus" {
        let db = app.state::<Db>();
        let conn = lock(&db.0);
        if crate::modes::declared(&conn).map_err(|e| e.to_string())?.as_deref() == Some("relax") {
            crate::modes::declare(&conn, None).map_err(|e| e.to_string())?;
        }
    }
    timer_action(&app, params)
}

// ---- Sounds: a macOS system sound when a break starts and when the pet calls the user back ----

pub(crate) fn play_sound(app: &AppHandle, name: &str) {
    let off = setting(&lock(&app.state::<Db>().0), "sounds").ok().flatten().as_deref() == Some("0");
    if off {
        return;
    }
    let file = format!("/System/Library/Sounds/{name}.aiff");
    // its own thread: afplay runs for as long as the sound, and the timer must keep ticking
    std::thread::spawn(move || {
        if let Err(e) = std::process::Command::new("afplay").arg(file).status() {
            eprintln!("sound failed: {e}");
        }
    });
}

/// Whether sounds are on. Pass `on` to change it.
#[tauri::command(async)]
pub(crate) fn sounds(app: AppHandle, on: Option<bool>) -> Result<bool, String> {
    let db = app.state::<Db>();
    let conn = lock(&db.0);
    if let Some(on) = on {
        conn.execute("INSERT OR REPLACE INTO setting (key, value) VALUES ('sounds', ?1)", [if on { "1" } else { "0" }])
            .map_err(|e| e.to_string())?;
    }
    Ok(setting(&conn, "sounds").map_err(|e| e.to_string())?.as_deref() != Some("0"))
}

// ---- Streak: logic in backend/streaks.py ----

/// Computes the streak and broadcasts it to every window as a "streak" event.
pub(crate) fn streak_state(app: &AppHandle) -> Result<Value, String> {
    let input = {
        let db = app.state::<Db>();
        let conn = lock(&db.0);
        streak_input(&conn).map_err(|e| e.to_string())?
    };
    let state = sidecar_call(&app.state::<Backend>(), "streak", Some(input))?;
    app.emit("streak", &state).map_err(|e| e.to_string())?;
    Ok(state)
}

#[tauri::command(async)]
pub(crate) fn streak(app: AppHandle) -> Result<Value, String> {
    streak_state(&app)
}

#[tauri::command(async)]
pub(crate) fn set_streak_threshold(app: AppHandle, minutes: f64) -> Result<Value, String> {
    if !(minutes >= 1.0 && minutes <= 24.0 * 60.0) {
        return Err("threshold must be between 1 minute and 24 hours".into());
    }
    lock(&app.state::<Db>().0)
        .execute(
            "INSERT OR REPLACE INTO setting (key, value) VALUES ('streak_threshold_min', ?1)",
            [minutes],
        )
        .map_err(|e| e.to_string())?;
    streak_state(&app)
}
