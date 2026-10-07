use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use rusqlite::{Connection, OptionalExtension};
use serde::Deserialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::db::{lock, setting, DataDir, Db, Syncing};
use crate::keychain::{keychain_delete, keychain_get, keychain_set};

// ---- Imports: feeds parsed by backend/feeds.py ----

/// One item of a parsed feed, as backend/feeds.py returns it.
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub(crate) enum FeedItem {
    Task { uid: String, title: String, course: Option<String>, due_at: Option<String>, due_date: Option<String> },
    Event { uid: String, title: String, start_at: String, end_at: String, all_day: bool },
}

/// Stores one whole feed from `source`. `now` is a UTC "YYYY-MM-DDTHH:MM:SSZ".
/// A task already imported keeps its row and its done status; only what the source owns is updated.
/// A task missing from the feed is flagged, not deleted, and the flag clears if it comes back.
/// A task first seen after its due day is not imported: the feed cannot say whether it was handed in.
/// Days are compared in the laptop's zone, so an evening sync does not treat today's work as yesterday's.
pub(crate) fn sync_feed(conn: &Connection, source: &str, items: &[FeedItem], now: &str) -> rusqlite::Result<()> {
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
                        let past: bool = tx.query_row(
                            "SELECT coalesce(coalesce(?1, date(?2, 'localtime')) < date(?3, 'localtime'), 0)",
                            (due_date, due_at, now),
                            |r| r.get(0),
                        )?;
                        if past {
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
           AND task_id IN (SELECT id FROM task WHERE coalesce(due_date, date(due_at, 'localtime')) >= date(?2, 'localtime'))",
        (source, now),
    )?;
    tx.commit()
}

const SOURCES: [&str; 2] = ["canvas", "google"];

/// What the `setting` table holds in place of a feed address: the address itself is in the Keychain (PR-4).
pub(crate) const IN_KEYCHAIN: &str = "keychain";

/// A source and its feed address, or why the address could not be read.
type FeedUrl = (&'static str, Result<String, String>);

/// Each saved feed address, read from the Keychain.
pub(crate) fn feed_urls(conn: &Connection, service: &str) -> Result<Vec<FeedUrl>, String> {
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
pub(crate) fn move_feed_urls_to_keychain(conn: &Connection, service: &str) -> rusqlite::Result<()> {
    for (source, url) in saved_feeds(conn, "feed_url_")? {
        if url != IN_KEYCHAIN && keychain_set(service, &format!("feed_url_{source}"), &url).is_ok() {
            conn.execute("UPDATE setting SET value = ?2 WHERE key = ?1", (format!("feed_url_{source}"), IN_KEYCHAIN))?;
        }
    }
    Ok(())
}

/// The .ics files the user uploaded, as text. A source has an address or a file, never both.
pub(crate) fn feed_files(conn: &Connection) -> rusqlite::Result<Vec<(&'static str, String)>> {
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
pub(crate) fn save_feed_url(conn: &Connection, service: &str, source: &str, url: &str) -> Result<(), String> {
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

pub(crate) const MAX_ICS_BYTES: usize = 5_000_000;

/// Saves an uploaded .ics file's text in place of an address. It is parsed again on every sync,
/// so repeating events keep rolling forward, but it only changes when the user uploads a new one.
pub(crate) fn save_feed_file(conn: &Connection, source: &str, ics: &str) -> Result<(), String> {
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
pub(crate) fn replace_feed(conn: &Connection, source: &str, prefix: &str, value: &str) -> rusqlite::Result<()> {
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
pub(crate) fn feed_list(conn: &Connection) -> rusqlite::Result<Vec<Value>> {
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

pub(crate) const GOOGLE_TOKEN: &str = "google_refresh_token";

/// The OAuth client from the Cloud console's download: `{ "client_id", "client_secret" }`.
pub(crate) fn google_client(dir: &Path) -> Result<Value, String> {
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
pub(crate) fn run_fetcher(request: &Value) -> Result<Value, String> {
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
    if out.status.code().is_none() {
        // ended by a signal: the alarm backend/fetcher.py sets on itself, so a stuck download can't hold the sync lock
        return Err("the task fetcher took too long and was stopped".into());
    }
    if !out.status.success() {
        // the last line of a Python traceback names the error, such as a missing package
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(format!("the task fetcher failed: {}", stderr.lines().last().unwrap_or("no output")));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("bad fetcher reply: {e}"))
}

/// Stores one source's reply from the fetcher and returns the status Settings shows.
/// A failed download stores nothing: treating it as an empty feed would flag every upcoming task as removed.
pub(crate) fn store_feed(conn: &Connection, source: &str, reply: &Value, now: &str) -> Value {
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

/// Saves how a sync went. A failure keeps the time of the last good sync as "ok_at",
/// so Today can say how old the list is.
pub(crate) fn save_feed_status(conn: &Connection, source: &str, mut status: Value) -> rusqlite::Result<()> {
    let key = format!("feed_status_{source}");
    if status.get("error").is_some() {
        let before = setting(conn, &key)?.and_then(|s| serde_json::from_str::<Value>(&s).ok());
        if let Some(before) = before {
            status["ok_at"] = before[if before.get("error").is_some() { "ok_at" } else { "at" }].clone();
        }
    }
    conn.execute("INSERT OR REPLACE INTO setting (key, value) VALUES (?1, ?2)", (key, status.to_string()))?;
    Ok(())
}

/// Downloads every saved feed and stores it, then tells the windows with a "tasks" event.
/// The database is not locked while the fetcher is on the network.
pub(crate) fn sync_feeds(app: &AppHandle) -> Result<(), String> {
    let syncing = app.state::<Syncing>();
    let _one_at_a_time = lock(&syncing.0);
    let db = app.state::<Db>();
    let (urls, files, signed_in) = {
        let conn = lock(&db.0);
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
        let conn = lock(&db.0);
        let mut statuses: Vec<(String, Value)> = failed.into_iter().map(|(s, error)| (s.to_string(), json!({ "at": now, "error": error }))).collect();
        for source in asked {
            let status = match &reply {
                Ok(reply) => store_feed(&conn, &source, &reply[&source], &now),
                Err(error) => json!({ "at": now, "error": error }),
            };
            statuses.push((source, status));
        }
        for (source, status) in statuses {
            save_feed_status(&conn, &source, status).map_err(|e| e.to_string())?;
        }
    }
    app.emit("tasks", ()).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn feeds(db: State<Db>) -> Result<Vec<Value>, String> {
    feed_list(&lock(&db.0)).map_err(|e| e.to_string())
}

/// Saves the address for "canvas" or "google" and syncs straight away. An empty `url` forgets the feed.
#[tauri::command(async)]
pub(crate) fn set_feed(app: AppHandle, source: String, url: String) -> Result<(), String> {
    {
        let syncing = app.state::<Syncing>();
        let _one_at_a_time = lock(&syncing.0);
        let db = app.state::<Db>();
        let conn = lock(&db.0);
        save_feed_url(&conn, &app.config().identifier, &source, url.trim())?;
    }
    if source == "google" {
        keychain_delete(&app.config().identifier, GOOGLE_TOKEN); // an address, or none, takes the account's place
    }
    sync_feeds(&app)
}

/// Saves the text of an uploaded .ics file for "canvas" or "google" and syncs straight away.
#[tauri::command(async)]
pub(crate) fn set_feed_file(app: AppHandle, source: String, ics: String) -> Result<(), String> {
    {
        let syncing = app.state::<Syncing>();
        let _one_at_a_time = lock(&syncing.0);
        let db = app.state::<Db>();
        let conn = lock(&db.0);
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
pub(crate) fn connect_google(app: AppHandle) -> Result<(), String> {
    let client = google_client(&app.state::<DataDir>().0)?;
    let reply = run_fetcher(&json!({ "connect": { "google": client } }))?;
    let token = reply["google"]["ok"]["refresh_token"]
        .as_str()
        .ok_or_else(|| reply["google"]["error"].as_str().unwrap_or("Sign-in was not completed").to_string())?;
    keychain_set(&app.config().identifier, GOOGLE_TOKEN, token)?;
    keychain_delete(&app.config().identifier, "feed_url_google"); // the account takes the address's place
    {
        let syncing = app.state::<Syncing>();
        let _one_at_a_time = lock(&syncing.0);
        let db = app.state::<Db>();
        let conn = lock(&db.0);
        replace_feed(&conn, "google", "feed_account_", "1").map_err(|e| e.to_string())?;
    }
    sync_feeds(&app)
}

#[tauri::command(async)]
pub(crate) fn sync_now(app: AppHandle) -> Result<(), String> {
    sync_feeds(&app)
}

/// Today's calendar events for the panel's timeline: all-day ones first, then by start time.
pub(crate) fn events_today(conn: &Connection) -> rusqlite::Result<Vec<Value>> {
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
pub(crate) fn list_events(db: State<Db>) -> Result<Vec<Value>, String> {
    events_today(&lock(&db.0)).map_err(|e| e.to_string())
}
