use rusqlite::{Connection, OptionalExtension};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::db::{lock, Db};
use crate::sidecar::{sidecar_call, Backend};

// ---- Modes: the rules are in backend/modes.py; today's spans and the user's declarations come from here ----

/// A declaration ends at the local midnight after it was made, so "Relax" left on in the evening does
/// not label the next morning. Every reader calls this first, since the app may have been shut overnight.
fn expire(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE mode_declaration SET ended_at = strftime('%Y-%m-%dT%H:%M:%SZ', date(started_at, 'localtime', '+1 day'), 'utc')
         WHERE ended_at IS NULL AND date(started_at, 'localtime') < date('now', 'localtime')",
        [],
    )?;
    Ok(())
}

/// Ends the declaration in force, and starts a new one unless `mode` is None ("decide for me").
pub(crate) fn declare(conn: &Connection, mode: Option<&str>) -> rusqlite::Result<()> {
    expire(conn)?;
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
    expire(conn)?;
    conn.query_row("SELECT mode FROM mode_declaration WHERE ended_at IS NULL", [], |r| r.get(0)).optional()
}

/// Rows that started on one local day: `?1` is "YYYY-MM-DD", or NULL for today. A string that is no
/// day matches nothing.
pub(crate) const ON_DAY: &str = "date(started_at, 'localtime') = coalesce(?1, date('now', 'localtime'))";

/// What backend/modes.py label_day needs for one local day (today when `day` is None), with times as
/// epoch seconds.
pub(crate) fn mode_input(conn: &Connection, day: Option<&str>) -> rusqlite::Result<Value> {
    // SQLite writes each list as JSON text
    let list = |fields: &str, from: &str| -> rusqlite::Result<Value> {
        let text: String = conn.query_row(
            &format!("SELECT json_group_array(json_object({fields})) FROM (SELECT * FROM {from} ORDER BY rowid)"),
            [day],
            |r| r.get(0),
        )?;
        Ok(serde_json::from_str(&text).unwrap_or_default())
    };
    expire(conn)?;
    let span = "'start', unixepoch(started_at), 'end', unixepoch(ended_at)";
    Ok(json!({
        "apps": list(&format!("{span}, 'app', app, 'bundle_id', bundle_id"), &format!("app_span WHERE {ON_DAY}"))?,
        "pages": list(&format!("{span}, 'domain', domain"), &format!("activity_span WHERE {ON_DAY}"))?,
        "sessions": list(span, &format!("session WHERE ended_at IS NOT NULL AND {ON_DAY}"))?,
        "rules": rule_map(conn)?,
        // a declaration ends by the midnight after it was made (expire), so it belongs to the day it
        // started on; one still in force runs until now
        "declarations": list(
            "'start', unixepoch(started_at), 'end', unixepoch(coalesce(ended_at, 'now')), 'mode', mode",
            &format!("mode_declaration WHERE {ON_DAY}"),
        )?,
    }))
}

/// The answered rules as { key: mode }, the shape backend/modes.py takes.
pub(crate) fn rule_map(conn: &Connection) -> rusqlite::Result<Value> {
    conn.query_row("SELECT json_group_object(key, mode) FROM app_rule WHERE mode IS NOT NULL", [], |r| r.get::<_, String>(0))
        .map(|text| serde_json::from_str(&text).unwrap_or_default())
}

/// One local day ("YYYY-MM-DD"; today when `day` is None): its labeled stretches, the seconds per
/// mode, what to ask about, and the time per app and site, with every site by name (R-5).
#[tauri::command(async)]
pub(crate) fn modes_day(app: AppHandle, day: Option<String>) -> Result<Value, String> {
    let input = mode_input(&lock(&app.state::<Db>().0), day.as_deref()).map_err(|e| e.to_string())?;
    let mut labeled = sidecar_call(&app.state::<Backend>(), "modes", Some(input))?;
    name_sites(&mut labeled);
    Ok(labeled)
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
    // every finished day is labeled again with the new answer (days_to_total in db.rs)
    conn.execute("DELETE FROM day_total", [])?;
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

/// Forgets an answer, so the pet may ask again. The saved day totals go, as in set_rule.
pub(crate) fn forget_rule(conn: &Connection, key: &str) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM day_total", [])?;
    conn.execute("DELETE FROM app_rule WHERE key = ?1", [key])?;
    Ok(())
}

pub(crate) fn rules(conn: &Connection) -> rusqlite::Result<Vec<Value>> {
    let mut rows: Vec<(String, String, String)> = conn
        .prepare("SELECT key, name, mode FROM app_rule WHERE mode IS NOT NULL")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<rusqlite::Result<_>>()?;
    for (key, name, _) in &mut rows {
        // ponytail: a site answered before sites had names was stored under its address, and nothing in
        // the row says it is a site. An app with no bundle ID whose name has a dot in it is renamed too.
        if name.to_lowercase() == *key && key.contains('.') {
            *name = site_name(name);
        }
    }
    rows.sort_by_key(|(_, name, _)| name.to_lowercase());
    Ok(rows.into_iter().map(|(key, name, mode)| json!({ "key": key, "name": name, "mode": mode })).collect())
}

// ---- R-5: a site is shown by its name, never by its address ----

// ponytail: a short fixed list; a site also covers its subdomains. Any other site is named after the
// first part of its address, which can be clumsy ("Blog"). Add a site here when its name annoys.
const SITE_NAMES: &[(&str, &str)] = &[
    ("youtube.com", "YouTube"),
    ("music.youtube.com", "YouTube Music"),
    ("google.com", "Google"),
    ("docs.google.com", "Google Docs"),
    ("drive.google.com", "Google Drive"),
    ("mail.google.com", "Gmail"),
    ("calendar.google.com", "Google Calendar"),
    ("classroom.google.com", "Google Classroom"),
    ("instructure.com", "Canvas"),
    ("gradescope.com", "Gradescope"),
    ("piazza.com", "Piazza"),
    ("khanacademy.org", "Khan Academy"),
    ("coursera.org", "Coursera"),
    ("edx.org", "edX"),
    ("quizlet.com", "Quizlet"),
    ("overleaf.com", "Overleaf"),
    ("wolframalpha.com", "WolframAlpha"),
    ("wikipedia.org", "Wikipedia"),
    ("github.com", "GitHub"),
    ("stackoverflow.com", "Stack Overflow"),
    ("chatgpt.com", "ChatGPT"),
    ("claude.ai", "Claude"),
    ("notion.so", "Notion"),
    ("linkedin.com", "LinkedIn"),
    ("indeed.com", "Indeed"),
    ("joinhandshake.com", "Handshake"),
    ("glassdoor.com", "Glassdoor"),
    ("netflix.com", "Netflix"),
    ("twitch.tv", "Twitch"),
    ("tiktok.com", "TikTok"),
    ("instagram.com", "Instagram"),
    ("facebook.com", "Facebook"),
    ("x.com", "X"),
    ("twitter.com", "X"),
    ("reddit.com", "Reddit"),
    ("hulu.com", "Hulu"),
    ("disneyplus.com", "Disney+"),
    ("primevideo.com", "Prime Video"),
    ("open.spotify.com", "Spotify"),
    ("soundcloud.com", "SoundCloud"),
    ("music.apple.com", "Apple Music"),
];

/// "www.youtube.com" -> "YouTube"; a site not on the list -> the first part of its address, capitalised.
pub(crate) fn site_name(domain: &str) -> String {
    let lower = domain.to_lowercase();
    let host = lower.strip_prefix("www.").unwrap_or(&lower);
    let parts: Vec<&str> = host.split('.').collect();
    // "en.wikipedia.org" tries itself, then "wikipedia.org", as category() in backend/modes.py does
    for i in 0..parts.len() {
        let tail = parts[i..].join(".");
        if let Some((_, name)) = SITE_NAMES.iter().find(|(site, _)| *site == tail) {
            return name.to_string();
        }
    }
    // a bare word (localhost) or a number (127.0.0.1) has no name in it
    if parts.len() < 2 || !parts[0].chars().any(char::is_alphabetic) {
        return host.to_string();
    }
    let mut first = parts[0].chars();
    first.next().map(|c| c.to_uppercase().chain(first).collect()).unwrap_or_default()
}

/// Puts names in a reply of backend/modes.py label_day: each site in `unknown` is renamed, and `usage`
/// becomes [{ name, seconds }], longest first, with two addresses of one site added together.
pub(crate) fn name_sites(day: &mut Value) {
    let named = |row: &Value| {
        let name = row["name"].as_str().unwrap_or_default();
        if row["site"] == true { site_name(name) } else { name.to_string() }
    };
    for ask in day["unknown"].as_array_mut().into_iter().flatten() {
        let name = named(ask);
        ask["name"] = json!(name);
    }
    let mut totals: Vec<(String, i64)> = Vec::new();
    for row in day["usage"].as_array().into_iter().flatten() {
        let (name, seconds) = (named(row), row["seconds"].as_i64().unwrap_or(0));
        match totals.iter_mut().find(|(n, _)| *n == name) {
            Some(total) => total.1 += seconds,
            None => totals.push((name, seconds)),
        }
    }
    totals.sort_by_key(|(_, seconds)| -seconds);
    day["usage"] = totals.into_iter().map(|(name, seconds)| json!({ "name": name, "seconds": seconds })).collect();
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
    let day = modes_day(app.clone(), None)?;
    take_question(&lock(&app.state::<Db>().0), &day["unknown"]).map_err(|e| e.to_string())
}

/// Every answer the pet remembers. Pass `key`, `name` and `mode` to save one; an empty `mode` forgets it,
/// and the pet may then ask again.
#[tauri::command]
pub(crate) fn learned(db: State<Db>, key: Option<String>, name: Option<String>, mode: Option<String>) -> Result<Vec<Value>, String> {
    let conn = lock(&db.0);
    if let (Some(key), Some(mode)) = (key, mode) {
        if mode.is_empty() {
            forget_rule(&conn, &key)
        } else {
            set_rule(&conn, &key, name.as_deref().unwrap_or(&key), &mode)
        }
        .map_err(|e| e.to_string())?;
    }
    rules(&conn).map_err(|e| e.to_string())
}

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
