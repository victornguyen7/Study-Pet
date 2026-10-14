use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager};

use crate::db::{days_to_total, lock, save_day_total, setting, streak_input, Db};
use crate::sidecar::{sidecar_call, Backend};

// ---- Sounds: a macOS system sound when the pet suggests a break ----

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
