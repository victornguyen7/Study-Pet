use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};

use rusqlite::{Connection, OptionalExtension};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager, State};

use crate::db::{lock, Db};

// ---- Pages from the Chrome extension (C-1): address, title and time, for pages the user allowed ----
//
// The extension (extension/) asks once per site and sends a sample every 5 s for an allowed
// page in front. Chrome starts a native-messaging host for it; that host is a two-line script that
// hands the bytes to the socket below, so nothing is recorded unless this app is running.

/// Two samples further apart than this are not joined: Chrome lost focus, the laptop slept, or a break came between.
pub(crate) const GAP_SECS: i64 = 15;
/// C-4: set by the timer thread during a break; samples are dropped while it is set.
pub(crate) static PAUSED: AtomicBool = AtomicBool::new(false);

/// PR-1: when the extension last said that what is in front in Chrome is private (epoch seconds), or 0.
/// It says so for an incognito window, a page showing a password field, and whenever no window it can
/// see has the keyboard, which is also true while Chrome is behind another app.
static PRIVATE_AT: AtomicI64 = AtomicI64::new(0);

/// The allowed page in front in Chrome, as (when it was sent, site, address). The drift check reads it.
static FRONT_PAGE: std::sync::Mutex<Option<(i64, String, String)>> = std::sync::Mutex::new(None);

/// The site and address of the page in front in Chrome, if the extension named one in the last two samples.
pub(crate) fn front_page(now: i64) -> Option<(String, String)> {
    lock(&FRONT_PAGE).clone().filter(|(at, ..)| now - at <= 10).map(|(_, domain, url)| (domain, url))
}

/// PR-3: until when (epoch seconds) the pet looks away. Nothing is noted before then.
static AWAY_UNTIL: AtomicI64 = AtomicI64::new(0);
const AWAY_SECS: i64 = 15 * 60;

/// C-4: nothing is noted during a break, or while the pet has been told to look away.
pub(crate) fn paused(now: i64) -> bool {
    PAUSED.load(Ordering::Relaxed) || now < AWAY_UNTIL.load(Ordering::Relaxed)
}

/// The look-away shortcut: 15 minutes from `now`, or back to watching if it was already looking away.
/// Returns until when, 0 when watching again.
pub(crate) fn look_away(now: i64) -> i64 {
    let until = if now < AWAY_UNTIL.load(Ordering::Relaxed) { 0 } else { now + AWAY_SECS };
    AWAY_UNTIL.store(until, Ordering::Relaxed);
    until
}

/// The app's end of the link to the extension, for the one thing the app says after hello.
static EXTENSION: std::sync::Mutex<Option<UnixStream>> = std::sync::Mutex::new(None);

/// Ladder step 4: asks the extension to cover every page on `site` for `minutes`.
// ponytail: one link is kept, so with two Chrome profiles connected only the last one hears it.
pub(crate) fn block(site: &str, minutes: i64) {
    let frame = json!({ "block": site, "minutes": minutes }).to_string();
    if let Some(stream) = lock(&EXTENSION).as_mut() {
        let _ = stream.write_all(&(frame.len() as u32).to_ne_bytes()).and_then(|_| stream.write_all(frame.as_bytes()));
    }
}

/// PR-11: when the extension last said a tab in Chrome is playing sound, or 0. Never which tab.
static AUDIO_AT: AtomicI64 = AtomicI64::new(0);

/// Whether audio is on in Chrome, by the extension's last two samples.
pub(crate) fn audio_on(now: i64) -> bool {
    now - AUDIO_AT.load(Ordering::Relaxed) <= 10
}

pub(crate) fn note_private(private: bool, now: i64) {
    PRIVATE_AT.store(if private { now } else { 0 }, Ordering::Relaxed);
}

/// Whether Chrome, when it is the app in front, is showing something private. An answer older than
/// two samples is not trusted: the extension may be gone.
pub(crate) fn chrome_private(now: i64) -> bool {
    now - PRIVATE_AT.load(Ordering::Relaxed) <= 10
}

/// Fixed by the `key` in extension/manifest.json.
const EXTENSION_ID: &str = "jenjlnnminflmafgghiodlmpokbkeble";

/// An http(s) address as its site and itself, without the #fragment.
fn clean(url: &str) -> Option<(String, String)> {
    if url.len() > 2000 {
        return None;
    }
    let mut url = tauri::Url::parse(url).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    url.set_fragment(None);
    Some((url.host_str()?.to_string(), url.into()))
}

/// Adds one sample at `now` (epoch seconds): the open span grows, or a new one starts where it ended.
/// False when the address is not a web page.
pub(crate) fn record_page(conn: &Connection, url: &str, title: &str, now: i64) -> rusqlite::Result<bool> {
    let Some((domain, url)) = clean(url) else { return Ok(false) };
    // PR-1: a site the user made Private is not kept, whatever the extension was told
    if crate::modes::is_private(conn, &domain)? {
        return Ok(false);
    }
    let title: String = title.chars().take(300).collect();
    let last: Option<(i64, String, i64)> = conn
        .query_row("SELECT id, url, unixepoch(ended_at) FROM activity_span ORDER BY id DESC LIMIT 1", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .optional()?;
    let insert = "INSERT INTO activity_span (started_at, ended_at, domain, url, title)
                  VALUES (strftime('%Y-%m-%dT%H:%M:%SZ', ?1, 'unixepoch'), strftime('%Y-%m-%dT%H:%M:%SZ', ?2, 'unixepoch'), ?3, ?4, ?5)";
    match last {
        Some((id, last_url, ended)) if now - ended <= GAP_SECS => {
            if last_url == url {
                conn.execute(
                    "UPDATE activity_span SET ended_at = strftime('%Y-%m-%dT%H:%M:%SZ', ?1, 'unixepoch'), title = ?2 WHERE id = ?3",
                    (now, title, id),
                )?;
            } else {
                conn.execute(insert, (ended, now, domain, url, title))?;
            }
        }
        _ => {
            conn.execute(insert, (now, now, domain, url, title))?;
        }
    }
    Ok(true)
}

/// Reads native-messaging frames (a 4-byte length, then JSON) until the other end closes or sends
/// something that is not ours.
pub(crate) fn read_frames(mut stream: impl Read, mut each: impl FnMut(Value)) {
    loop {
        let mut len = [0u8; 4];
        if stream.read_exact(&mut len).is_err() {
            return;
        }
        // an address and a title; anything bigger did not come from the extension
        let len = u32::from_ne_bytes(len) as usize;
        if len > 64 * 1024 {
            return;
        }
        let mut frame = vec![0; len];
        if stream.read_exact(&mut frame).is_err() {
            return;
        }
        match serde_json::from_slice(&frame) {
            Ok(message) => each(message),
            Err(_) => return,
        }
    }
}

fn serve(app: &AppHandle, mut stream: UnixStream) {
    // tells the extension the pet is really here; a failed start of the host looks the same as silence
    let hello = br#"{"hello":true}"#;
    if stream.write_all(&(hello.len() as u32).to_ne_bytes()).and_then(|_| stream.write_all(hello)).is_err() {
        return;
    }
    *lock(&EXTENSION) = stream.try_clone().ok();
    read_frames(stream, |message| {
        let now = chrono::Utc::now().timestamp();
        let private = message["private"] == true;
        note_private(private, now);
        AUDIO_AT.store(if message["audio"] == true { now } else { 0 }, Ordering::Relaxed);
        *lock(&FRONT_PAGE) = None; // until this sample names a page that may be kept
        if private || paused(now) {
            return;
        }
        let (Some(url), Some(title)) = (message["url"].as_str(), message["title"].as_str()) else { return };
        let saved = record_page(&lock(&app.state::<Db>().0), url, title, now);
        if let (Ok(true), Some((domain, url))) = (&saved, clean(url)) {
            *lock(&FRONT_PAGE) = Some((now, domain, url));
        }
        if let Err(e) = saved {
            eprintln!("page sample failed: {e}");
        }
    });
}

/// Opens the socket in the data folder and tells Chrome where the host is.
// ponytail: any program running as the user can write to the socket and add made-up pages, the same
// reach it has into the Keychain.
// The host is named after the app's identifier, so the dev and the everyday app each have their own
// and neither overwrites the other's; HOSTS in extension/background.js lists both, the everyday app first.
pub(crate) fn start(app: AppHandle, dir: &Path, home: &Path) -> std::io::Result<()> {
    let dir = dir.canonicalize()?;
    let name = app.config().identifier.clone();
    let socket = dir.join("ext.sock");
    let _ = fs::remove_file(&socket); // left by the last run
    let listener = UnixListener::bind(&socket)?;
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600))?;
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let app = app.clone();
            std::thread::spawn(move || serve(&app, stream));
        }
    });

    let chrome = home.join("Library/Application Support/Google/Chrome");
    if !chrome.is_dir() {
        return Ok(());
    }
    // the host: nc ends when the socket is gone, which is how the extension learns the pet has quit
    let host = dir.join("chrome-host.sh");
    fs::write(&host, format!("#!/bin/sh\nexec /usr/bin/nc -U '{}'\n", socket.display()))?;
    fs::set_permissions(&host, fs::Permissions::from_mode(0o755))?;
    let hosts = chrome.join("NativeMessagingHosts");
    fs::create_dir_all(&hosts)?;
    let manifest = json!({
        "name": name,
        "description": "Study Pet",
        "path": host,
        "type": "stdio",
        "allowed_origins": [format!("chrome-extension://{EXTENSION_ID}/")],
    });
    fs::write(hosts.join(format!("{name}.json")), manifest.to_string())
}

/// Today's seconds per site, longest first.
pub(crate) fn site_time_today(conn: &Connection) -> rusqlite::Result<Vec<Value>> {
    conn.prepare(
        "SELECT domain, sum(unixepoch(ended_at) - unixepoch(started_at)) FROM activity_span
         WHERE date(started_at, 'localtime') = date('now', 'localtime')
         GROUP BY domain ORDER BY 2 DESC",
    )?
    .query_map([], |r| Ok(json!({ "domain": r.get::<_, String>(0)?, "seconds": r.get::<_, i64>(1)? })))?
    .collect()
}

#[tauri::command]
pub(crate) fn site_time(db: State<Db>) -> Result<Vec<Value>, String> {
    site_time_today(&lock(&db.0)).map_err(|e| e.to_string())
}
