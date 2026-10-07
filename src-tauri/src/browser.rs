use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

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

const HOST_NAME: &str = "com.hiennguyen.pet";
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
    read_frames(stream, |message| {
        if PAUSED.load(Ordering::Relaxed) {
            return;
        }
        let (Some(url), Some(title)) = (message["url"].as_str(), message["title"].as_str()) else { return };
        let saved = record_page(&lock(&app.state::<Db>().0), url, title, chrono::Utc::now().timestamp());
        if let Err(e) = saved {
            eprintln!("page sample failed: {e}");
        }
    });
}

/// Opens the socket in the data folder and tells Chrome where the host is.
// ponytail: any program running as the user can write to the socket and add made-up pages, the same
// reach it has into the Keychain. The dev and the everyday app share one host name, so Chrome talks
// to whichever started last; give each its own name if both must record.
pub(crate) fn start(app: AppHandle, dir: &Path, home: &Path) -> std::io::Result<()> {
    let dir = dir.canonicalize()?;
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
        "name": HOST_NAME,
        "description": "Study Pet",
        "path": host,
        "type": "stdio",
        "allowed_origins": [format!("chrome-extension://{EXTENSION_ID}/")],
    });
    fs::write(hosts.join(format!("{HOST_NAME}.json")), manifest.to_string())
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
