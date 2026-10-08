use rusqlite::{Connection, OptionalExtension};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::db::{lock, Db};
use crate::sidecar::{sidecar_call, Backend};

// ---- Modes: the rules are in backend/modes.py; today's spans and the user's declarations come from here ----

/// Ends the declaration in force, and starts a new one unless `mode` is None ("decide for me").
// ponytail: a declaration lasts until it is changed, even overnight. End it at a new focus block or
// at midnight if "Relax" gets left on.
pub(crate) fn declare(conn: &Connection, mode: Option<&str>) -> rusqlite::Result<()> {
    // the new one first: a mode the table refuses must leave the old declaration in force
    let new = match mode {
        Some(mode) => {
            conn.execute("INSERT INTO mode_declaration (started_at, mode) VALUES (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), ?1)", [mode])?;
            conn.last_insert_rowid()
        }
        None => 0,
    };
    conn.execute(
        "UPDATE mode_declaration SET ended_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE ended_at IS NULL AND id != ?1",
        [new],
    )?;
    Ok(())
}

pub(crate) fn declared(conn: &Connection) -> rusqlite::Result<Option<String>> {
    conn.query_row("SELECT mode FROM mode_declaration WHERE ended_at IS NULL", [], |r| r.get(0)).optional()
}

/// What backend/modes.py label_day needs for today, with times as epoch seconds.
pub(crate) fn mode_input(conn: &Connection) -> rusqlite::Result<Value> {
    // SQLite writes each list as JSON text
    let list = |fields: &str, from: &str| -> rusqlite::Result<Value> {
        let text: String = conn.query_row(
            &format!("SELECT json_group_array(json_object({fields})) FROM (SELECT * FROM {from} ORDER BY rowid)"),
            [],
            |r| r.get(0),
        )?;
        Ok(serde_json::from_str(&text).unwrap_or_default())
    };
    let span = "'start', unixepoch(started_at), 'end', unixepoch(ended_at)";
    let today = "date(started_at, 'localtime') = date('now', 'localtime')";
    Ok(json!({
        "apps": list(&format!("{span}, 'app', app, 'bundle_id', bundle_id"), &format!("app_span WHERE {today}"))?,
        "pages": list(&format!("{span}, 'domain', domain"), &format!("activity_span WHERE {today}"))?,
        "sessions": list(span, &format!("session WHERE ended_at IS NOT NULL AND {today}"))?,
        "rules": rule_map(conn)?,
        // one still in force runs until now
        "declarations": list(
            "'start', unixepoch(started_at), 'end', unixepoch(coalesce(ended_at, 'now')), 'mode', mode",
            "mode_declaration WHERE ended_at IS NULL OR date(ended_at, 'localtime') = date('now', 'localtime')",
        )?,
    }))
}

/// The answered rules as { key: mode }, the shape backend/modes.py takes.
pub(crate) fn rule_map(conn: &Connection) -> rusqlite::Result<Value> {
    conn.query_row("SELECT json_group_object(key, mode) FROM app_rule WHERE mode IS NOT NULL", [], |r| r.get::<_, String>(0))
        .map(|text| serde_json::from_str(&text).unwrap_or_default())
}

/// Today's labeled stretches and the seconds per mode.
#[tauri::command(async)]
pub(crate) fn modes_today(app: AppHandle) -> Result<Value, String> {
    let input = mode_input(&lock(&app.state::<Db>().0)).map_err(|e| e.to_string())?;
    sidecar_call(&app.state::<Backend>(), "modes", Some(input))
}

/// The mode the user has declared, if any. Pass `set` to change it: "study", "work", "relax", or "" for none.
#[tauri::command]
pub(crate) fn declared_mode(db: State<Db>, set: Option<String>) -> Result<Option<String>, String> {
    let conn = lock(&db.0);
    if let Some(mode) = set {
        declare(&conn, Some(mode.as_str()).filter(|m| !m.is_empty())).map_err(|e| e.to_string())?;
    }
    declared(&conn).map_err(|e| e.to_string())
}

// ---- Learned rules (PR-2): what the user said an app or site is for. Asked once, kept in app_rule. ----

/// Whether the user has made this app (by bundle ID, or by name if it has none) or site Private.
pub(crate) fn is_private(conn: &Connection, key: &str) -> rusqlite::Result<bool> {
    conn.query_row("SELECT EXISTS (SELECT 1 FROM app_rule WHERE key = lower(?1) AND mode = 'private')", [key], |r| r.get(0))
}

/// Saves an answer. A Private one also takes the name off what was already recorded: the app's past
/// spans become "Private", and the site's pages are deleted.
pub(crate) fn set_rule(conn: &Connection, key: &str, name: &str, mode: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO app_rule (key, name, mode) VALUES (lower(?1), ?2, ?3) ON CONFLICT (key) DO UPDATE SET mode = ?3",
        (key, name, mode),
    )?;
    if mode == "private" {
        conn.execute("UPDATE app_span SET app = 'Private', bundle_id = NULL WHERE lower(coalesce(bundle_id, app)) = lower(?1)", [key])?;
        conn.execute("DELETE FROM activity_span WHERE lower(domain) = lower(?1)", [key])?;
    }
    Ok(())
}

pub(crate) fn rules(conn: &Connection) -> rusqlite::Result<Vec<Value>> {
    conn.prepare("SELECT key, name, mode FROM app_rule WHERE mode IS NOT NULL ORDER BY name COLLATE NOCASE")?
        .query_map([], |r| Ok(json!({ "key": r.get::<_, String>(0)?, "name": r.get::<_, String>(1)?, "mode": r.get::<_, String>(2)? })))?
        .collect()
}

/// The first of `unknown` (from label_day) the pet has not brought up before, as a line for its
/// bubble. It is marked as asked, so the pet says it once; the panel keeps listing it until answered.
pub(crate) fn take_question(conn: &Connection, unknown: &Value) -> rusqlite::Result<Option<String>> {
    for ask in unknown.as_array().into_iter().flatten() {
        let (Some(key), Some(name)) = (ask["key"].as_str(), ask["name"].as_str()) else { continue };
        let new = conn.execute(
            "INSERT OR IGNORE INTO app_rule (key, name, asked_at) VALUES (lower(?1), ?2, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))",
            (key, name),
        )?;
        if new == 1 {
            return Ok(Some(format!("What is {name} for? Open the panel and tell me.")));
        }
    }
    Ok(None)
}

/// The question to put in the pet's bubble now, if there is a new one.
pub(crate) fn question(app: &AppHandle) -> Result<Option<String>, String> {
    let day = modes_today(app.clone())?;
    take_question(&lock(&app.state::<Db>().0), &day["unknown"]).map_err(|e| e.to_string())
}

/// Every answer the pet remembers. Pass `key`, `name` and `mode` to save one; an empty `mode` forgets it,
/// and the pet may then ask again.
#[tauri::command]
pub(crate) fn learned(db: State<Db>, key: Option<String>, name: Option<String>, mode: Option<String>) -> Result<Vec<Value>, String> {
    let conn = lock(&db.0);
    if let (Some(key), Some(mode)) = (key, mode) {
        if mode.is_empty() {
            conn.execute("DELETE FROM app_rule WHERE key = ?1", [&key]).map(drop)
        } else {
            set_rule(&conn, &key, name.as_deref().unwrap_or(&key), &mode)
        }
        .map_err(|e| e.to_string())?;
    }
    rules(&conn).map_err(|e| e.to_string())
}

// ---- Focus keeper settings: the voice of the pet's lines, and the opt-in block (ladder step 4) ----

/// { personality: "gentle" | "sassy" | "roast", block: bool }. Pass either to change it; every window
/// hears the new settings as a "keeper" event.
#[tauri::command]
pub(crate) fn keeper(app: AppHandle, personality: Option<String>, block: Option<bool>) -> Result<Value, String> {
    let db = app.state::<Db>();
    let conn = lock(&db.0);
    let set = |key: &str, value: &str| conn.execute("INSERT OR REPLACE INTO setting (key, value) VALUES (?1, ?2)", (key, value));
    if let Some(p) = personality.filter(|p| ["gentle", "sassy", "roast"].contains(&p.as_str())) {
        set("personality", &p).map_err(|e| e.to_string())?;
    }
    if let Some(on) = block {
        set("block_drift", if on { "1" } else { "0" }).map_err(|e| e.to_string())?;
    }
    let get = |key: &str| crate::db::setting(&conn, key).map_err(|e| e.to_string());
    let state = json!({
        "personality": get("personality")?.unwrap_or_else(|| "gentle".into()),
        "block": get("block_drift")?.as_deref() == Some("1"),
    });
    app.emit("keeper", &state).map_err(|e| e.to_string())?;
    Ok(state)
}

// ---- S-7: study with no session running, and the pet offers to start one ----

const OFFER_AFTER_SECS: i64 = 5 * 60;
const OFFER_EVERY_SECS: i64 = 30 * 60;
static LAST_OFFER: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);

/// Whether `day` (from label_day) ends in Study that has lasted 5 minutes and is still going at `now`.
pub(crate) fn studying(day: &Value, now: i64) -> bool {
    day["spans"].as_array().and_then(|spans| spans.last()).is_some_and(|last| {
        let (start, end) = (last["start"].as_i64().unwrap_or(0), last["end"].as_i64().unwrap_or(0));
        last["mode"] == "study" && now - end <= 60 && end - start >= OFFER_AFTER_SECS
    })
}

/// The pet's offer, at most once in half an hour. Call it only while no session is running.
pub(crate) fn offer(app: &AppHandle) -> Result<Option<String>, String> {
    let now = chrono::Utc::now().timestamp();
    let last = LAST_OFFER.load(std::sync::atomic::Ordering::Relaxed);
    if now - last < OFFER_EVERY_SECS || !studying(&modes_today(app.clone())?, now) {
        return Ok(None);
    }
    LAST_OFFER.store(now, std::sync::atomic::Ordering::Relaxed);
    Ok(Some("Looks like you're studying. Open the panel to start a focus block?".into()))
}
