
use rusqlite::{Connection, OptionalExtension};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::browser::{idle_secs, nobody_there, paused, GAP_SECS};
use crate::db::{lock, Db};
use crate::sidecar::{sidecar_call, Backend};

// ---- Front app (C-1, C-3): which app is in front, and for how long ----
//
// The name and the time only, which is what the spec allows for a Neutral app; a Private app keeps only the time. The window title waits
// for the privacy gate (PR-1) and the onboarding (PR-10): reading it needs the Accessibility permission,
// and a title can hold a chat's name or a document's.

const SAMPLE_SECS: u64 = 5;
/// Ladder step 4: how long a site stays blocked.
const BLOCK_MIN: i64 = 5;

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

const CHROME: &str = "com.google.Chrome";

/// Whether the keyboard is in a password field, in any app. False without the Accessibility permission.
// ponytail: the field with the keyboard only, so a password field that is on screen but not being
// typed in is missed, and so is one in an app that tells macOS nothing about its fields.
pub(crate) fn password_field_focused() -> bool {
    use core_foundation::base::{CFType, CFTypeRef, TCFType};
    use core_foundation::string::{CFString, CFStringRef};
    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXUIElementCreateSystemWide() -> CFTypeRef;
        fn AXUIElementSetMessagingTimeout(element: CFTypeRef, seconds: f32) -> i32;
        fn AXUIElementCopyAttributeValue(element: CFTypeRef, attribute: CFStringRef, value: *mut CFTypeRef) -> i32;
    }
    // the value of an attribute, or None on any error (0 is kAXErrorSuccess)
    let copy = |element: &CFType, attribute: &'static str| -> Option<CFType> {
        let mut value: CFTypeRef = std::ptr::null();
        let name = CFString::from_static_string(attribute);
        let error = unsafe { AXUIElementCopyAttributeValue(element.as_CFTypeRef(), name.as_concrete_TypeRef(), &mut value) };
        (error == 0 && !value.is_null()).then(|| unsafe { CFType::wrap_under_create_rule(value) })
    };
    let system = unsafe { CFType::wrap_under_create_rule(AXUIElementCreateSystemWide()) };
    // an app that has hung must not hold the monitor up for macOS's default of 6 s
    unsafe { AXUIElementSetMessagingTimeout(system.as_CFTypeRef(), 1.0) };
    copy(&system, "AXFocusedUIElement")
        .and_then(|focused| copy(&focused, "AXSubrole"))
        .and_then(|subrole| subrole.downcast::<CFString>())
        .is_some_and(|subrole| subrole == "AXSecureTextField")
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

// ---- Background audio (PR-11): that sound is on, from any app, and never what is playing ----

/// Whether any program is using the sound output in use (speakers, headphones).
// ponytail: "using", not "audible". A call, or a player that keeps the output open while paused,
// counts as sound, and so does a muted laptop. Nothing says music from speech either.
pub(crate) fn audio_running() -> bool {
    use std::ffi::c_void;
    #[link(name = "CoreAudio", kind = "framework")]
    extern "C" {
        fn AudioObjectGetPropertyData(object: u32, address: *const [u32; 3], qualifier_size: u32, qualifier: *const c_void, size: *mut u32, data: *mut c_void) -> i32;
    }
    // an AudioObjectPropertyAddress: selector, scope ('glob'), element (main)
    let read = |object: u32, selector: &[u8; 4]| -> Option<u32> {
        let address = [u32::from_be_bytes(*selector), u32::from_be_bytes(*b"glob"), 0];
        let (mut value, mut size) = (0u32, 4u32);
        let error = unsafe { AudioObjectGetPropertyData(object, &address, 0, std::ptr::null(), &mut size, (&mut value as *mut u32).cast()) };
        (error == 0).then_some(value)
    };
    // kAudioObjectSystemObject's default output device, then its "is running somewhere"
    read(1, b"dOut").and_then(|device| read(device, b"gone")).is_some_and(|running| running != 0)
}

/// Adds one sample of sound at `now` (epoch seconds): the open span grows, or a new one starts.
pub(crate) fn record_audio(conn: &Connection, now: i64) -> rusqlite::Result<()> {
    let grown = conn.execute(
        "UPDATE audio_span SET ended_at = strftime('%Y-%m-%dT%H:%M:%SZ', ?1, 'unixepoch')
         WHERE id = (SELECT max(id) FROM audio_span) AND ?1 - unixepoch(ended_at) <= ?2",
        (now, GAP_SECS),
    )?;
    if grown == 0 {
        conn.execute(
            "INSERT INTO audio_span (started_at, ended_at) VALUES (strftime('%Y-%m-%dT%H:%M:%SZ', ?1, 'unixepoch'), strftime('%Y-%m-%dT%H:%M:%SZ', ?1, 'unixepoch'))",
            [now],
        )?;
    }
    Ok(())
}

/// Seconds with sound on during one local day ("YYYY-MM-DD"; today when `day` is None).
pub(crate) fn audio_time_on(conn: &Connection, day: Option<&str>) -> rusqlite::Result<i64> {
    conn.query_row(
        &format!(
            "SELECT coalesce(sum(unixepoch(ended_at) - unixepoch(started_at)), 0) FROM audio_span WHERE {}",
            crate::modes::ON_DAY
        ),
        [day],
        |r| r.get(0),
    )
}

#[tauri::command]
pub(crate) fn audio_time(db: State<Db>, day: Option<String>) -> Result<i64, String> {
    audio_time_on(&lock(&db.0), day.as_deref()).map_err(|e| e.to_string())
}

/// Samples the front app every 5 s, except while the pet looks away and with nobody at the keyboard.
pub(crate) fn start(app: AppHandle) {
    std::thread::spawn(move || {
        let mut drift = Value::Null; // the drift state the windows last heard
        loop {
            std::thread::sleep(std::time::Duration::from_secs(SAMPLE_SECS));
            let now = chrono::Utc::now().timestamp();
            if paused(now) {
                continue;
            }
            // sound is noted whatever is in front, and with nobody at the keyboard: music plays on
            let audio = audio_running() || crate::browser::audio_on(now);
            if audio {
                if let Err(e) = record_audio(&lock(&app.state::<Db>().0), now) {
                    eprintln!("audio sample failed: {e}");
                }
            }
            let Some((name, bundle)) = front_app().map(gate) else { continue };
            let chrome = bundle.as_deref() == Some(CHROME);
            if nobody_there(idle_secs(), chrome && crate::browser::audio_on(now)) {
                continue;
            }
            // a private window in an ordinary app: a password is being typed, or Chrome's front window is
            // one its extension calls private (incognito, or a page with a password field)
            let window = password_field_focused() || (chrome && crate::browser::chrome_private(now));
            let stored = {
                let db = app.state::<Db>();
                let conn = lock(&db.0);
                // the user's own Private apps, by the same key backend/modes.py uses
                crate::modes::is_private(&conn, bundle.as_deref().unwrap_or(&name)).and_then(|private| {
                    let private = private || window;
                    let (name, bundle) = if private { ("Private", None) } else { (name.as_str(), bundle.as_deref()) };
                    record_app(&conn, name, bundle, now)?;
                    // what the drift check needs: the same thing the gate let through, and the user's rules
                    let page = if chrome && !private { crate::browser::front_page(now) } else { None }.unzip();
                    let block = crate::db::setting(&conn, "block_drift")?.as_deref() == Some("1");
                    Ok((block, json!({
                        "app": name, "bundle_id": bundle, "domain": page.0, "url": page.1,
                        "rules": crate::modes::rule_map(&conn)?, "declared": crate::modes::declared(&conn)?,
                        "break_after_min": crate::modes::break_after(&conn)?,
                    })))
                })
            };
            // the keeper and its ladder are in backend/drift.py; the pet hears each change as a "drift"
            // event, which also carries what its face shows: the mode of what is in front, and whether
            // audio is on
            let checked = stored.map_err(|e| e.to_string()).and_then(|(block, front)| {
                let site = front["domain"].as_str().map(str::to_string);
                let after = front["break_after_min"].as_i64().unwrap_or(50);
                let mut state = sidecar_call(&app.state::<Backend>(), "watch", Some(front))?;
                // R-5: the pet's line names the site, never its address
                if let (Some(site), true) = (&site, state["what"].is_string()) {
                    state["what"] = json!(crate::modes::site_name(site));
                }
                // step 4, once as the ladder reaches it, and only if the user opted in: a site can be
                // blocked, an app cannot
                if let (true, Some(site)) = (block && state["step"] == 4 && drift["step"] != 4, site) {
                    crate::browser::block(&site, BLOCK_MIN);
                }
                // the minute loop in lib.rs holds reminders and questions back while the keeper is on
                crate::modes::KEEPER_ON.store(state["on"] == true, std::sync::atomic::Ordering::Relaxed);
                // said once per run of study or work, then taken off so the windows are not told twice
                if state["break"] == true {
                    crate::session::play_sound(&app, "Glass");
                    let _ = app.emit("break", format!("You've been at it for {after} minutes. Take a break?"));
                }
                if let Some(state) = state.as_object_mut() {
                    state.remove("break");
                }
                Ok::<_, String>(state)
            });
            match checked {
                Ok(mut state) => {
                    state["audio"] = json!(audio);
                    if state != drift {
                        let _ = app.emit("drift", &state);
                        drift = state;
                    }
                }
                Err(e) => eprintln!("app sample failed: {e}"),
            }
        }
    });
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
