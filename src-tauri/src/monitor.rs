use std::sync::atomic::Ordering;

use rusqlite::{Connection, OptionalExtension};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager, State};

use crate::browser::{GAP_SECS, PAUSED};
use crate::db::{lock, Db};

// ---- Front app (C-1, C-3): which app is in front, and for how long ----
//
// The name and the time only, which is what the spec allows for a Neutral app; a Private app keeps only the time. The window title waits
// for the privacy gate (PR-1) and the onboarding (PR-10): reading it needs the Accessibility permission,
// and a title can hold a chat's name or a document's.

const SAMPLE_SECS: u64 = 5;

/// The app in front, as its name and bundle ID. None for the pet itself and for the lock screen.
pub(crate) fn front_app() -> Option<(String, Option<String>)> {
    use objc2_app_kit::{NSRunningApplication, NSWorkspace};
    // this thread has no pool of its own, and AppKit would leak a little on every sample
    objc2::rc::autoreleasepool(|_| {
        let front = NSWorkspace::sharedWorkspace().frontmostApplication()?;
        if front == NSRunningApplication::currentApplication() {
            return None;
        }
        let bundle = front.bundleIdentifier().map(|b| b.to_string());
        if matches!(bundle.as_deref(), Some("com.apple.loginwindow" | "com.apple.ScreenSaver.Engine")) {
            return None;
        }
        Some((front.localizedName()?.to_string(), bundle))
    })
}

// ---- Privacy gate (PR-1, C-2): every sample passes through it before anything is stored ----

/// Private by default: password managers, messaging and mail.
// ponytail: a fixed list by bundle ID, so an app that is not on it is still named, and a private
// window in an ordinary app (incognito, a password field) is not seen at all. The user's own answers
// come with "ask once and remember" and should be checked before this list.
const PRIVATE_APPS: &[&str] = &[
    "com.1password.1password",
    "com.agilebits.onepassword7",
    "com.bitwarden.desktop",
    "com.lastpass.LastPass",
    "org.keepassxc.keepassxc",
    "com.apple.keychainaccess",
    "com.apple.Passwords",
    "com.apple.MobileSMS",
    "com.apple.FaceTime",
    "net.whatsapp.WhatsApp",
    "ru.keepcoder.Telegram",
    "com.tdesktop.Telegram",
    "org.whispersystems.signal-desktop",
    "com.hnc.Discord",
    "com.tinyspeck.slackmacgap",
    "com.facebook.archon",
    "com.tencent.xinWeChat",
    "com.vng.zalo",
    "jp.naver.line.mac",
    "com.viber.osx",
    "com.apple.mail",
    "com.microsoft.Outlook",
    "com.readdle.smartemail-Mac",
    "com.mimestream.Mimestream",
    "org.mozilla.thunderbird",
];

/// What may be kept of the app in front: a Private one loses its name and bundle ID, leaving only the time.
pub(crate) fn gate((name, bundle): (String, Option<String>)) -> (String, Option<String>) {
    match &bundle {
        Some(id) if PRIVATE_APPS.iter().any(|p| p.eq_ignore_ascii_case(id)) => ("Private".into(), None),
        _ => (name, bundle),
    }
}

/// Adds one sample at `now` (epoch seconds): the open span grows, or a new one starts where it ended.
pub(crate) fn record_app(conn: &Connection, app: &str, bundle_id: Option<&str>, now: i64) -> rusqlite::Result<()> {
    let last: Option<(i64, String, i64)> = conn
        .query_row("SELECT id, app, unixepoch(ended_at) FROM app_span ORDER BY id DESC LIMIT 1", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .optional()?;
    let started = match last {
        Some((id, last_app, ended)) if now - ended <= GAP_SECS => {
            if last_app == app {
                conn.execute(
                    "UPDATE app_span SET ended_at = strftime('%Y-%m-%dT%H:%M:%SZ', ?1, 'unixepoch') WHERE id = ?2",
                    (now, id),
                )?;
                return Ok(());
            }
            ended
        }
        _ => now,
    };
    conn.execute(
        "INSERT INTO app_span (started_at, ended_at, app, bundle_id)
         VALUES (strftime('%Y-%m-%dT%H:%M:%SZ', ?1, 'unixepoch'), strftime('%Y-%m-%dT%H:%M:%SZ', ?2, 'unixepoch'), ?3, ?4)",
        (started, now, app, bundle_id),
    )?;
    Ok(())
}

/// Samples the front app every 5 s, except during a break (C-4).
// ponytail: an app left in front with nobody at the keyboard keeps counting until the screen locks or
// the laptop sleeps. Stop after some minutes without input (CGEventSourceSecondsSinceLastEventType)
// once the drift ladder needs honest numbers; a lecture video has no input either, so pick the limit then.
pub(crate) fn start(app: AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_secs(SAMPLE_SECS));
        if PAUSED.load(Ordering::Relaxed) {
            continue;
        }
        let Some((name, bundle)) = front_app().map(gate) else { continue };
        let db = app.state::<Db>();
        let conn = lock(&db.0);
        // the user's own Private apps, by the same key backend/modes.py uses
        let saved = crate::modes::is_private(&conn, bundle.as_deref().unwrap_or(&name)).and_then(|private| {
            let (name, bundle) = if private { ("Private", None) } else { (name.as_str(), bundle.as_deref()) };
            record_app(&conn, name, bundle, chrono::Utc::now().timestamp())
        });
        if let Err(e) = saved {
            eprintln!("app sample failed: {e}");
        }
    });
}

/// Today's seconds per app, longest first.
pub(crate) fn app_time_today(conn: &Connection) -> rusqlite::Result<Vec<Value>> {
    conn.prepare(
        "SELECT app, sum(unixepoch(ended_at) - unixepoch(started_at)) FROM app_span
         WHERE date(started_at, 'localtime') = date('now', 'localtime')
         GROUP BY app ORDER BY 2 DESC",
    )?
    .query_map([], |r| Ok(json!({ "app": r.get::<_, String>(0)?, "seconds": r.get::<_, i64>(1)? })))?
    .collect()
}

#[tauri::command]
pub(crate) fn app_time(db: State<Db>) -> Result<Vec<Value>, String> {
    app_time_today(&lock(&db.0)).map_err(|e| e.to_string())
}

// ---- Accessibility permission (PR-10): asked for only from Settings, where it is explained ----

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> u8;
    fn AXIsProcessTrustedWithOptions(options: core_foundation::dictionary::CFDictionaryRef) -> u8;
}

/// Whether macOS lets the pet read window titles. With `ask`, macOS shows its own prompt when it
/// does not, and adds the pet to the Accessibility list in System Settings.
#[tauri::command]
pub(crate) fn accessibility(ask: bool) -> bool {
    use core_foundation::{base::TCFType, boolean::CFBoolean, dictionary::CFDictionary, string::CFString};
    if !ask {
        return unsafe { AXIsProcessTrusted() } != 0;
    }
    // the value of kAXTrustedCheckOptionPrompt
    let prompt = CFString::from_static_string("AXTrustedCheckOptionPrompt");
    let options = CFDictionary::from_CFType_pairs(&[(prompt, CFBoolean::true_value())]);
    unsafe { AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef()) != 0 }
}
