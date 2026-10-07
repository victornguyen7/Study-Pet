use std::path::Path;
use std::sync::Mutex;

use rusqlite::Connection;
use serde_json::json;
use tauri::{Emitter, Manager, WindowEvent};

mod sidecar;
use sidecar::Backend;

mod db;
use db::{init_db, lock, DataDir, Db, Syncing};

mod keychain;

mod windows;

#[cfg(unix)]
mod browser;

#[cfg(target_os = "macos")]
mod monitor;

mod modes;

mod session;
use session::{play_sound, streak_state, timer_action};

mod tasks;
use tasks::take_reminder;

mod feeds;
use feeds::{move_feed_urls_to_keychain, sync_feeds};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(Backend::default())
        .manage(Syncing::default())
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
            move_feed_urls_to_keychain(&conn, &app.config().identifier)?;
            app.manage(Db(Mutex::new(conn)));
            // pages from the Chrome extension; the pet works without them
            #[cfg(unix)]
            if let Err(e) = browser::start(app.handle().clone(), &dir, &app.path().home_dir()?) {
                eprintln!("browser extension link failed: {e}");
            }
            app.manage(DataDir(dir));
            #[cfg(target_os = "macos")]
            monitor::start(app.handle().clone());

            // drive the timer once a second; every window hears the new state as a "timer" event
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                let mut rang = json!(null); // the [phase, calls] a sound was last played for
                for second in 1u64.. {
                    std::thread::sleep(std::time::Duration::from_secs(1));
                    let state = timer_action(&handle, json!({ "action": "tick" }));
                    if let Err(e) = &state {
                        eprintln!("timer tick failed: {e}");
                    }
                    // C-4: pages from the extension and the front app are not noted during a break
                    #[cfg(unix)]
                    browser::PAUSED.store(
                        state.as_ref().is_ok_and(|s| s["phase"] == "break"),
                        std::sync::atomic::Ordering::Relaxed,
                    );
                    // a break that starts, and each time the pet calls the user back from one
                    if let Ok(s) = &state {
                        let key = json!([s["phase"], s["calls"]]);
                        if key != rang {
                            match s["phase"].as_str() {
                                Some("break") => play_sound(&handle, "Glass"),
                                Some("callback") => play_sound(&handle, "Ping"),
                                _ => {}
                            }
                            rang = key;
                        }
                    }
                    // once a minute, so "today" rolls over at midnight while the pet stays open
                    if second % 60 == 0 {
                        // a task that has become due soon is mentioned by the pet, but never during a focus
                        // block: it waits, unmarked, and the first check after the block ends picks it up
                        if state.is_ok_and(|s| s["phase"] != "focus") {
                            let text = take_reminder(&lock(&handle.state::<Db>().0));
                            match text {
                                Ok(Some(text)) => drop(handle.emit("reminder", text)),
                                // nothing due: an app or site the pet cannot place is asked about, once each
                                Ok(None) => match modes::question(&handle) {
                                    Ok(Some(text)) => drop(handle.emit("reminder", text)),
                                    Ok(None) => {}
                                    Err(e) => eprintln!("mode question failed: {e}"),
                                },
                                Err(e) => eprintln!("reminder check failed: {e}"),
                            }
                        }
                        if let Err(e) = streak_state(&handle) {
                            eprintln!("streak refresh failed: {e}");
                        }
                    }
                }
            });

            // import the saved feeds at launch, then every half hour
            // ponytail: thread::sleep does not count time asleep, so after the lid opens the next sync
            // can be up to 30 minutes away. "Sync now" in Settings covers it; sync on wake if that hurts.
            let handle = app.handle().clone();
            std::thread::spawn(move || loop {
                if let Err(e) = sync_feeds(&handle) {
                    eprintln!("feed sync failed: {e}");
                }
                std::thread::sleep(std::time::Duration::from_secs(30 * 60));
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

            #[cfg(target_os = "macos")]
            if let Some(panel) = app.get_webview_window("panel") {
                windows::move_to_active_space(&panel);
            }
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
            db::list_presets,
            db::add_preset,
            db::delete_preset,
            db::day,
            session::timer,
            session::streak,
            session::set_streak_threshold,
            session::sounds,
            tasks::add_task,
            tasks::list_tasks,
            tasks::set_task_done,
            tasks::delete_task,
            tasks::merge_task,
            tasks::list_courses,
            tasks::set_course,
            feeds::feeds,
            feeds::set_feed,
            feeds::set_feed_file,
            feeds::connect_google,
            feeds::sync_now,
            feeds::list_events,
            windows::toggle_panel,
            modes::modes_today,
            modes::declared_mode,
            modes::learned,
            #[cfg(unix)]
            browser::site_time,
            #[cfg(target_os = "macos")]
            monitor::app_time,
            #[cfg(target_os = "macos")]
            monitor::accessibility
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use db::{day_data, save_session, setting, streak_input, Session};
    use sidecar::{call, sidecar_call, spawn_sidecar};
    use feeds::*;
    use keychain::{keychain_delete, keychain_get, keychain_set};
    use tasks::*;

    #[test]
    fn db_seeds_presets_once() {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        init_db(&conn).unwrap();
        let n: i64 = conn.query_row("SELECT count(*) FROM preset", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 2);

        let s = Session { started_at: 0.0, ended_at: 1500.0, planned_min: 25.0, break_min: 5.0, label: None, task_id: None };
        save_session(&conn, &s).unwrap();
        let ended: String = conn.query_row("SELECT ended_at FROM session", [], |r| r.get(0)).unwrap();
        assert_eq!(ended, "1970-01-01T00:25:00+00:00");
    }

    #[test]
    fn pages_join_into_spans_and_only_web_addresses_are_kept() {
        use browser::{read_frames, record_page, site_time_today};
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        let now: i64 = conn.query_row("SELECT unixepoch('now')", [], |r| r.get(0)).unwrap();
        let t = now - 1000;
        // 10 s on one page (a #fragment is the same page), 5 s on another, then a sample after a gap
        assert!(record_page(&conn, "https://canvas.edu/a", "A", t).unwrap());
        assert!(record_page(&conn, "https://canvas.edu/a#top", "A", t + 5).unwrap());
        assert!(record_page(&conn, "https://canvas.edu/a", "A, renamed", t + 10).unwrap());
        assert!(record_page(&conn, "https://docs.rs/b?q=1", "B", t + 15).unwrap());
        assert!(record_page(&conn, "https://docs.rs/b?q=1", "B", t + 600).unwrap());
        for other in ["chrome://settings", "file:///etc/passwd", "not an address"] {
            assert!(!record_page(&conn, other, "", t + 600).unwrap());
        }
        let rows: Vec<(String, String)> = conn
            .prepare("SELECT url, title FROM activity_span ORDER BY id").unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap()
            .collect::<rusqlite::Result<_>>().unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0], ("https://canvas.edu/a".into(), "A, renamed".into()));
        // run just after local midnight, the spans fall on yesterday and today's list is empty
        let today: bool = conn.query_row("SELECT date(?1, 'unixepoch', 'localtime') = date('now', 'localtime')", [t], |r| r.get(0)).unwrap();
        if today {
            assert_eq!(site_time_today(&conn).unwrap(), [json!({ "domain": "canvas.edu", "seconds": 10 }), json!({ "domain": "docs.rs", "seconds": 5 })]);
        }

        // frames as Chrome sends them; an oversized length ends the reading
        let frame = |body: &str| [&(body.len() as u32).to_ne_bytes()[..], body.as_bytes()].concat();
        let bytes = [frame(r#"{"url":"u"}"#), frame("{}"), u32::MAX.to_ne_bytes().to_vec(), frame("{}")].concat();
        let mut seen = vec![];
        read_frames(&bytes[..], |m| seen.push(m));
        assert_eq!(seen, [json!({ "url": "u" }), json!({})]);
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn front_app_samples_join_into_spans() {
        use monitor::{app_time_today, gate, record_app};
        // PR-1: a Private app is stored without its name or bundle ID, whatever the case of the ID
        let app = |name: &str, id: Option<&str>| gate((name.to_string(), id.map(Into::into)));
        assert_eq!(app("Messages", Some("com.apple.MobileSMS")), ("Private".into(), None));
        assert_eq!(app("Mail", Some("com.apple.Mail")), ("Private".into(), None));
        assert_eq!(app("Preview", Some("com.apple.Preview")), ("Preview".into(), Some("com.apple.Preview".into())));
        assert_eq!(app("a.out", None), ("a.out".into(), None));
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        let now: i64 = conn.query_row("SELECT unixepoch('now')", [], |r| r.get(0)).unwrap();
        let t = now - 1000;
        // 10 s in one app, 5 s in the next (counted from where the first ended), then a sample after a gap
        for (app, at) in [("Preview", 0), ("Preview", 5), ("Preview", 10), ("Code", 15), ("Code", 600)] {
            record_app(&conn, app, Some("com.example"), t + at).unwrap();
        }
        let spans: Vec<(String, i64)> = conn
            .prepare("SELECT app, unixepoch(ended_at) - unixepoch(started_at) FROM app_span ORDER BY id").unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap()
            .collect::<rusqlite::Result<_>>().unwrap();
        assert_eq!(spans, [("Preview".into(), 10), ("Code".into(), 5), ("Code".into(), 0)]);
        // run just after local midnight, the spans fall on yesterday and today's list is empty
        let today: bool = conn.query_row("SELECT date(?1, 'unixepoch', 'localtime') = date('now', 'localtime')", [t], |r| r.get(0)).unwrap();
        if today {
            assert_eq!(app_time_today(&conn).unwrap(), [json!({ "app": "Preview", "seconds": 10 }), json!({ "app": "Code", "seconds": 5 })]);
        }
    }

    /// Asks macOS for the app really in front, from a thread as the monitor does:
    ///   cargo test real_front_app -- --ignored --nocapture
    #[test]
    #[cfg(target_os = "macos")]
    #[ignore = "needs a desktop session with an app in front"]
    fn real_front_app() {
        let front = std::thread::spawn(monitor::front_app).join().unwrap();
        println!("accessibility allowed: {}", monitor::accessibility(false)); // only checks, never asks
        println!("{front:?}");
        assert!(front.is_some_and(|(name, _)| !name.is_empty()));
    }

    #[test]
    fn todays_spans_are_labeled_by_the_python_rules() {
        use modes::{declare, declared, is_private, mode_input, rules, set_rule, take_question};
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        assert_eq!(mode_input(&conn).unwrap(), json!({ "apps": [], "pages": [], "sessions": [], "declarations": [], "rules": {} }));
        // 20 min ago: 10 min on a course site in Chrome, then 5 min in an app nothing knows
        let now: i64 = conn.query_row("SELECT unixepoch('now')", [], |r| r.get(0)).unwrap();
        let t = now - 1200;
        let at = |secs: i64| chrono::DateTime::from_timestamp(secs, 0).unwrap().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        conn.execute("INSERT INTO app_span (started_at, ended_at, app, bundle_id) VALUES (?1, ?2, 'Google Chrome', 'com.google.Chrome'), (?2, ?3, 'Preview', NULL)", (at(t), at(t + 600), at(t + 900))).unwrap();
        conn.execute("INSERT INTO activity_span (started_at, ended_at, domain, url, title) VALUES (?1, ?2, 'canvas.school.edu', 'https://canvas.school.edu/', 'Canvas')", (at(t), at(t + 600))).unwrap();

        // only one declaration is in force at a time, and a mode that does not exist is refused
        declare(&conn, Some("relax")).unwrap();
        declare(&conn, Some("study")).unwrap();
        assert!(declare(&conn, Some("nap")).is_err());
        assert_eq!(declared(&conn).unwrap().as_deref(), Some("study"));
        let input = mode_input(&conn).unwrap();
        assert_eq!((input["apps"][1]["bundle_id"].clone(), input["declarations"].as_array().unwrap().len()), (Value::Null, 2));

        // run just after local midnight, the spans fall on yesterday and there is nothing to label
        let today: bool = conn.query_row("SELECT date(?1, 'unixepoch', 'localtime') = date('now', 'localtime')", [t], |r| r.get(0)).unwrap();
        if today {
            let mut sidecar = spawn_sidecar().unwrap();
            let day = call(&mut sidecar, "modes", Some(input)).unwrap();
            // the 5 unknown minutes take the label they follow
            assert_eq!(day["totals"], json!({ "study": 900 }));
            assert_eq!(day["spans"][0]["source"], "rule");
        }
        declare(&conn, None).unwrap();
        assert_eq!(declared(&conn).unwrap(), None);

        // PR-2: each unknown is brought up once, and stays out of the rules until it is answered
        let unknown = json!([{ "key": "preview", "name": "Preview", "seconds": 300 }, { "key": "canvas.school.edu", "name": "canvas.school.edu", "seconds": 200 }]);
        assert_eq!(take_question(&conn, &unknown).unwrap().as_deref(), Some("What is Preview for? Open the panel and tell me."));
        assert_eq!(take_question(&conn, &unknown).unwrap().as_deref(), Some("What is canvas.school.edu for? Open the panel and tell me."));
        assert_eq!(take_question(&conn, &unknown).unwrap(), None);
        assert_eq!((rules(&conn).unwrap().len(), mode_input(&conn).unwrap()["rules"].clone()), (0, json!({})));
        set_rule(&conn, "Preview", "Preview", "study").unwrap();
        set_rule(&conn, "preview", "Preview", "work").unwrap(); // a second answer replaces the first
        assert!(set_rule(&conn, "preview", "Preview", "nap").is_err());
        assert_eq!(mode_input(&conn).unwrap()["rules"], json!({ "preview": "work" }));
        assert_eq!(rules(&conn).unwrap(), [json!({ "key": "preview", "name": "Preview", "mode": "work" })]);
        // Private takes the name off what is already stored, and the gate keeps new samples out
        set_rule(&conn, "preview", "Preview", "private").unwrap();
        set_rule(&conn, "canvas.school.edu", "canvas.school.edu", "private").unwrap();
        let apps: Vec<String> = conn.prepare("SELECT app FROM app_span ORDER BY id").unwrap().query_map([], |r| r.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap();
        let pages: i64 = conn.query_row("SELECT count(*) FROM activity_span", [], |r| r.get(0)).unwrap();
        assert_eq!((apps, pages), (vec!["Google Chrome".to_string(), "Private".to_string()], 0));
        assert!(is_private(&conn, "Preview").unwrap() && !is_private(&conn, "com.google.Chrome").unwrap());
        assert!(!browser::record_page(&conn, "https://canvas.school.edu/a", "A", now).unwrap());
    }

    #[test]
    fn task_tables_block_duplicates_and_follow_deletes() {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        init_db(&conn).unwrap();
        conn.execute("INSERT INTO task (title) VALUES ('HW3')", []).unwrap();
        let link = "INSERT INTO task_source_link (task_id, source, source_uid, last_seen_at) VALUES (1, 'canvas', 'a1', '2026-01-01T00:00:00Z')";
        conn.execute(link, []).unwrap();
        assert!(conn.execute(link, []).is_err());
        assert!(conn.execute("UPDATE task SET status = 'nope'", []).is_err());

        conn.execute("INSERT INTO session (started_at, planned_min, break_min, task_id) VALUES ('2026-01-01T00:00:00Z', 25, 5, 1)", []).unwrap();
        conn.execute("DELETE FROM task", []).unwrap();
        let links: i64 = conn.query_row("SELECT count(*) FROM task_source_link", [], |r| r.get(0)).unwrap();
        let task_id: Option<i64> = conn.query_row("SELECT task_id FROM session", [], |r| r.get(0)).unwrap();
        assert_eq!((links, task_id), (0, None));
    }

    #[test]
    fn task_list_orders_by_due_and_keeps_todays_done() {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        let add = |title: &str, due: Option<&str>| {
            let t = NewTask { title: title.into(), course: None, due_date: due.map(Into::into), estimate_min: None };
            insert_task(&conn, &t).unwrap()
        };
        let (undated, late, soon) = (add("undated", None), add("late", Some("2026-12-01")), add("soon", Some("2026-11-01")));
        let order = || tasks(&conn).unwrap().iter().map(|t| (t.id, t.done)).collect::<Vec<_>>();
        assert_eq!(order(), [(soon, false), (late, false), (undated, false)]);

        assert_eq!(set_done(&conn, soon, true).unwrap(), 1);
        assert_eq!(order(), [(late, false), (undated, false), (soon, true)]);
        // done on an earlier day: off the list
        conn.execute("UPDATE task SET done_at = '2020-01-01T00:00:00Z' WHERE id = ?1", [soon]).unwrap();
        assert_eq!(order(), [(late, false), (undated, false)]);

        set_done(&conn, soon, false).unwrap();
        assert_eq!(order()[0], (soon, false));
        assert_eq!(set_done(&conn, 99, true).unwrap(), 0);
    }

    #[test]
    fn feed_sync_updates_in_place_and_flags_removed() {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        // the shape backend/feeds.py returns
        let feed = |v: Value| serde_json::from_value::<Vec<FeedItem>>(v).unwrap();
        let hw = |title: &str, due_at: &str| {
            json!({ "kind": "task", "source": "canvas", "uid": "event-assignment-1", "title": title,
                    "course": "STATISTC 315", "due_at": due_at, "due_date": null })
        };
        let all_day = |uid: &str, due: &str| {
            json!({ "kind": "task", "source": "canvas", "uid": uid, "title": uid, "course": null, "due_at": null, "due_date": due })
        };
        let event = |title: &str| {
            json!({ "kind": "event", "source": "canvas", "uid": "event-calendar-event-1", "title": title,
                    "start_at": "2026-10-06T14:00:00Z", "end_at": "2026-10-06T15:00:00Z", "all_day": false })
        };
        // "long ago" was due before the first sync, so it is never imported
        let first = feed(json!([hw("Homework 1", "2026-10-09T20:00:00Z"), all_day("quiz", "2026-10-12"), all_day("old", "2026-09-08"),
                                all_day("long ago", "2026-08-01"), event("Office hours")]));
        sync_feed(&conn, "canvas", &first, "2026-09-01T12:00:00Z").unwrap();
        sync_feed(&conn, "canvas", &first, "2026-10-04T12:30:00Z").unwrap();
        let count = |table: &str| conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get::<_, i64>(0)).unwrap();
        assert_eq!((count("task"), count("task_source_link"), count("calendar_event")), (3, 3, 1));

        // checked off here, then edited at the source; "quiz" and "old" leave the feed
        set_done(&conn, 1, true).unwrap();
        let second = feed(json!([hw("Homework 1 (revised)", "2026-10-16T20:00:00Z"), event("Office hours, moved")]));
        sync_feed(&conn, "canvas", &second, "2026-10-04T13:00:00Z").unwrap();
        let row: (String, String, String) = conn
            .query_row("SELECT title, due_at, status FROM task WHERE id = 1", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap();
        assert_eq!(row, ("Homework 1 (revised)".into(), "2026-10-16T20:00:00Z".into(), "done".into()));
        let removed = |uid: &str| {
            conn.query_row("SELECT removed_at FROM task_source_link WHERE source_uid = ?1", [uid], |r| r.get::<_, Option<String>>(0))
                .unwrap()
        };
        assert_eq!(removed("event-assignment-1"), None);
        assert_eq!(removed("quiz"), Some("2026-10-04T13:00:00Z".into()));
        assert_eq!(removed("old"), None); // already past: the feed aged it out
        let title: String = conn.query_row("SELECT title FROM calendar_event", [], |r| r.get(0)).unwrap();
        assert_eq!((count("task"), title), (3, "Office hours, moved".into()));
        let listed = tasks(&conn).unwrap();
        let quiz = listed.iter().find(|t| t.title == "quiz").unwrap();
        assert_eq!((quiz.source.as_deref(), quiz.removed), (Some("canvas"), true));

        // back in the feed: the flag clears
        sync_feed(&conn, "canvas", &first, "2026-10-04T14:00:00Z").unwrap();
        assert_eq!((removed("quiz"), count("task")), (None, 3));

        // another source's feed leaves these alone
        sync_feed(&conn, "google", &[], "2026-10-04T15:00:00Z").unwrap();
        assert_eq!((removed("quiz"), count("calendar_event")), (None, 1));
    }

    #[test]
    fn feed_sync_compares_local_days() {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        // the first and last minute of the local day, as the UTC stamps a feed and a sync carry;
        // away from UTC the two fall on different UTC days
        let at = |time: &str| -> String {
            conn.query_row("SELECT strftime('%Y-%m-%dT%H:%M:%SZ', date('now', 'localtime') || ?1, 'utc')", [time], |r| r.get(0)).unwrap()
        };
        let (morning, night) = (at(" 00:01:00"), at(" 23:59:00"));
        let feed: Vec<FeedItem> = serde_json::from_value(json!([
            { "kind": "task", "source": "canvas", "uid": "today", "title": "Due this morning", "course": null, "due_at": morning, "due_date": null }
        ]))
        .unwrap();
        // due earlier today: still imported in the evening
        sync_feed(&conn, "canvas", &feed, &night).unwrap();
        assert_eq!(tasks(&conn).unwrap().len(), 1);
        // and flagged when it leaves the feed that same day
        sync_feed(&conn, "canvas", &[], &at(" 23:59:30")).unwrap();
        assert!(tasks(&conn).unwrap()[0].removed);
    }

    #[test]
    fn failed_sync_keeps_the_last_good_time() {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        let status = || json!(feed_list(&conn).unwrap())[0]["status"].clone();
        save_feed_status(&conn, "canvas", json!({ "at": "T0", "error": "offline" })).unwrap();
        assert_eq!(status().get("ok_at"), None); // never synced
        save_feed_status(&conn, "canvas", json!({ "at": "T1", "tasks": 1, "events": 0 })).unwrap();
        save_feed_status(&conn, "canvas", json!({ "at": "T2", "error": "offline" })).unwrap();
        save_feed_status(&conn, "canvas", json!({ "at": "T3", "error": "offline" })).unwrap();
        assert_eq!(status(), json!({ "at": "T3", "error": "offline", "ok_at": "T1" }));
    }

    #[test]
    fn failed_download_stores_nothing_and_deleted_import_stays_gone() {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        let now = "2026-10-04T12:00:00Z";
        let ok = json!({ "ok": [
            { "kind": "task", "source": "canvas", "uid": "event-assignment-1", "title": "Quiz", "course": null, "due_at": null, "due_date": "2026-10-12" }
        ] });
        assert_eq!(store_feed(&conn, "canvas", &ok, now), json!({ "at": now, "tasks": 1, "events": 0 }));

        // a failed download is reported and flags nothing as removed
        let failed = store_feed(&conn, "canvas", &json!({ "error": "the server answered 404" }), "2026-10-04T13:00:00Z");
        assert_eq!(failed["error"], "the server answered 404");
        assert_eq!(store_feed(&conn, "canvas", &Value::Null, now)["error"], "the fetcher sent nothing for this feed");
        assert!(!tasks(&conn).unwrap()[0].removed);

        // deleting an imported task hides it, and the next sync does not bring it back
        remove_task(&conn, 1).unwrap();
        store_feed(&conn, "canvas", &ok, "2026-10-04T14:00:00Z");
        let rows: i64 = conn.query_row("SELECT count(*) FROM task", [], |r| r.get(0)).unwrap();
        assert_eq!((tasks(&conn).unwrap().len(), rows), (0, 1));
        // a typed task is really deleted
        let typed = insert_task(&conn, &NewTask { title: "typed".into(), course: None, due_date: None, estimate_min: None }).unwrap();
        remove_task(&conn, typed).unwrap();
        assert_eq!(conn.query_row("SELECT count(*) FROM task", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
    }

    #[test]
    fn feed_addresses_and_todays_events() {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        // addresses go to the real login Keychain, under a service name only this test uses
        let kc = "com.hiennguyen.pet.test-feeds";
        let urls = |conn: &Connection| feed_urls(conn, kc).unwrap();
        let secret = "https://example.com/secret.ics?user=a&key=b%20c";
        assert!(save_feed_url(&conn, kc, "canvas", "file:///etc/passwd").is_err());
        assert!(save_feed_url(&conn, kc, "nope", "https://example.com/a.ics").is_err());
        save_feed_url(&conn, kc, "google", secret).unwrap();
        assert_eq!(urls(&conn), [("google", Ok(secret.to_string()))]);
        // the database holds a marker, never the address
        assert_eq!(setting(&conn, "feed_url_google").unwrap().as_deref(), Some(IN_KEYCHAIN));

        // an address an older build left in the database still works, and is moved at launch
        conn.execute("INSERT INTO setting (key, value) VALUES ('feed_url_canvas', 'https://example.com/old.ics')", []).unwrap();
        assert_eq!(urls(&conn)[0], ("canvas", Ok("https://example.com/old.ics".to_string())));
        move_feed_urls_to_keychain(&conn, kc).unwrap();
        assert_eq!(setting(&conn, "feed_url_canvas").unwrap().as_deref(), Some(IN_KEYCHAIN));
        assert_eq!(urls(&conn)[0], ("canvas", Ok("https://example.com/old.ics".to_string())));
        // a marker with nothing behind it is that feed's error, not a crash
        keychain_delete(kc, "feed_url_canvas");
        assert_eq!(urls(&conn)[0], ("canvas", Err("the address is missing from the Keychain; paste it again".to_string())));
        save_feed_url(&conn, kc, "canvas", "").unwrap();
        // Settings is told a feed is saved, never its address
        let list = json!(feed_list(&conn).unwrap());
        assert_eq!(
            list,
            json!([
                { "source": "canvas", "set": false, "file": false, "account": false, "status": null },
                { "source": "google", "set": true, "file": false, "account": false, "status": null }
            ])
        );
        // an uploaded file takes the address's place, and an address takes it back
        assert!(save_feed_file(&conn, "google", "  ").is_err());
        assert!(save_feed_file(&conn, "google", &"x".repeat(MAX_ICS_BYTES + 1)).is_err());
        save_feed_file(&conn, "google", "BEGIN:VCALENDAR").unwrap();
        assert_eq!((urls(&conn).len(), feed_files(&conn).unwrap()), (0, vec![("google", "BEGIN:VCALENDAR".to_string())]));
        assert_eq!(json!(feed_list(&conn).unwrap())[1], json!({ "source": "google", "set": true, "file": true, "account": false, "status": null }));
        // signing in takes the file's place: a source holds one of address, file or account
        replace_feed(&conn, "google", "feed_account_", "1").unwrap();
        assert_eq!(json!(feed_list(&conn).unwrap())[1], json!({ "source": "google", "set": true, "file": false, "account": true, "status": null }));
        assert_eq!((urls(&conn).len(), feed_files(&conn).unwrap().len()), (0, 0));
        save_feed_url(&conn, kc, "google", secret).unwrap();
        assert_eq!((urls(&conn).len(), feed_files(&conn).unwrap().len()), (1, 0));
        assert_eq!(json!(feed_list(&conn).unwrap())[1]["account"], false);

        // starts now, so it lands on today whatever the local time is
        let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let today: String = conn.query_row("SELECT date('now', 'localtime')", [], |r| r.get(0)).unwrap();
        let add = |uid: &str, start: &str, end: &str, all_day: bool| {
            conn.execute(
                "INSERT INTO calendar_event (source, uid, title, start_at, end_at, all_day) VALUES ('google', ?1, ?1, ?2, ?3, ?4)",
                (uid, start, end, all_day),
            )
            .unwrap();
        };
        add("lecture", &now, &now, false);
        add("holiday", &today, &today, true);
        add("last year", "2025-01-01T10:00:00Z", "2025-01-01T11:00:00Z", false);
        add("old all-day", "2025-01-01", "2025-01-01", true);
        let titles: Vec<Value> = events_today(&conn).unwrap().iter().map(|e| e["title"].clone()).collect();
        assert_eq!(titles, ["holiday", "lecture"]);

        // forgetting a feed drops its events
        save_feed_url(&conn, kc, "google", "").unwrap();
        let events: i64 = conn.query_row("SELECT count(*) FROM calendar_event", [], |r| r.get(0)).unwrap();
        assert_eq!((urls(&conn).len(), events), (0, 0));
        assert_eq!(keychain_get(kc, "feed_url_google"), Ok(None)); // and the Keychain item with it
        // disconnecting clears an account too
        replace_feed(&conn, "google", "feed_account_", "1").unwrap();
        save_feed_url(&conn, kc, "google", "").unwrap();
        assert_eq!(json!(feed_list(&conn).unwrap())[1], json!({ "source": "google", "set": false, "file": false, "account": false, "status": null }));

        // an uploaded file goes through the same fetcher, with no download
        let google = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../backend/tests/google.ics")).unwrap();
        let reply = run_fetcher(&json!({ "files": { "google": google, "canvas": "not a calendar" } })).unwrap();
        assert_eq!(reply["canvas"]["error"], "that is not a calendar this app can read");
        // the fixture's dates may be outside the window by now, so only the shape is checked
        assert!(store_feed(&conn, "google", &reply["google"], &now).get("error").is_none());
    }

    /// The whole import against a real feed, into a throwaway database:
    ///   CANVAS_ICS_URL=https://... cargo test -- --ignored
    #[test]
    #[ignore = "needs the network and CANVAS_ICS_URL"]
    fn real_canvas_feed_round_trip() {
        let url = std::env::var("CANVAS_ICS_URL").expect("set CANVAS_ICS_URL to a Canvas calendar feed address");
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let reply = run_fetcher(&json!({ "feeds": { "canvas": url, "google": "https://example.com/missing.ics" } })).unwrap();
        assert!(reply["google"]["error"].is_string());
        for _ in 0..2 {
            let status = store_feed(&conn, "canvas", &reply["canvas"], &now);
            assert_eq!(status.get("error"), None);
            println!("{status}, {} of them listed", tasks(&conn).unwrap().len());
        }
    }

    #[test]
    fn streak_counts_a_day_with_enough_study() {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        // starts now, so it lands on today whatever the local time is
        let now = chrono::Utc::now().timestamp() as f64;
        let s = Session { started_at: now, ended_at: now + 30.0 * 60.0, planned_min: 30.0, break_min: 5.0, label: None, task_id: None };
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
    fn a_lock_survives_a_panic_while_it_was_held() {
        let db = std::sync::Arc::new(Mutex::new(1));
        let held = db.clone();
        let _ = std::thread::spawn(move || {
            let _guard = held.lock().unwrap();
            panic!("on purpose");
        })
        .join();
        assert!(db.lock().is_err());
        assert_eq!(*lock(&db), 1);
    }

    #[test]
    fn sidecar_ping() {
        let mut sidecar = spawn_sidecar().unwrap();
        assert_eq!(call(&mut sidecar, "ping", None).unwrap()["pong"], true);
        assert!(call(&mut sidecar, "nope", None).is_err());

        // an error a method raises leaves the process, and the block in progress, alone
        let backend = Backend::default();
        let start = json!({ "action": "start_focus", "focus_min": 25, "break_min": 5 });
        sidecar_call(&backend, "timer", Some(start)).unwrap();
        assert!(sidecar_call(&backend, "quick_add", Some(json!({ "text": "", "today": "2026-10-20" }))).is_err());
        assert_eq!(sidecar_call(&backend, "timer", Some(json!({ "action": "tick" }))).unwrap()["phase"], "focus");

        let line = json!({ "text": "STAT 310 HW3 2026-10-23 2h", "today": "2026-10-20" });
        let task: NewTask = serde_json::from_value(call(&mut sidecar, "quick_add", Some(line)).unwrap()).unwrap();
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        assert_eq!(insert_task(&conn, &task).unwrap(), 1);
        let row: (String, String, String, i64, String) = conn
            .query_row("SELECT title, course, due_date, estimate_min, status FROM task", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })
            .unwrap();
        assert_eq!(row, ("HW3".into(), "STAT 310".into(), "2026-10-23".into(), 120, "todo".into()));
        assert!(call(&mut sidecar, "quick_add", Some(json!({ "text": "fri", "today": "2026-10-20" }))).is_err());

        let start = json!({ "action": "start_focus", "focus_min": 25, "break_min": 5 });
        assert_eq!(call(&mut sidecar, "timer", Some(start)).unwrap()["phase"], "focus");
        let mut stopped = call(&mut sidecar, "timer", Some(json!({ "action": "stop" }))).unwrap();
        assert_eq!(stopped["phase"], "idle");
        let sessions: Vec<Session> = serde_json::from_value(stopped["sessions"].take()).unwrap();
        assert_eq!(sessions.len(), 1);
    }

    /// Writes to the real login Keychain under a service name of its own, then removes it.
    #[test]
    #[cfg(target_os = "macos")]
    fn keychain_round_trip_and_client_file() {
        let service = "com.hiennguyen.pet.test";
        let get = || keychain_get(service, GOOGLE_TOKEN);
        keychain_delete(service, GOOGLE_TOKEN);
        assert_eq!(get(), Ok(None));
        keychain_set(service, GOOGLE_TOKEN, "1//0g-Abc_def.123").unwrap();
        keychain_set(service, GOOGLE_TOKEN, "1//second").unwrap(); // a new sign-in replaces the old token
        assert_eq!(get(), Ok(Some("1//second".into())));
        // a secret may hold anything a command line would choke on
        let awkward = r#"https://x.example/a.ics?b=1&c="d" $HOME;e"#;
        keychain_set(service, GOOGLE_TOKEN, awkward).unwrap();
        assert_eq!(get(), Ok(Some(awkward.into())));
        // a name may not: nothing that could add a second command to the line
        for bad in ["", "a b", "a\nadd-generic-password", "a\"b", "a;b"] {
            assert!(keychain_set(service, bad, "x").is_err(), "{bad:?}");
            assert!(keychain_set(bad, GOOGLE_TOKEN, "x").is_err(), "{bad:?}");
        }
        assert!(keychain_set(service, GOOGLE_TOKEN, "").is_err());
        assert_eq!(get(), Ok(Some(awkward.into())));
        keychain_delete(service, GOOGLE_TOKEN);
        assert_eq!(get(), Ok(None));

        let dir = std::env::temp_dir().join("pet-google-client-test");
        std::fs::create_dir_all(&dir).unwrap();
        let _ = std::fs::remove_file(dir.join("google_client.json"));
        assert!(google_client(&dir).unwrap_err().starts_with("put the Google client file at "));
        std::fs::write(dir.join("google_client.json"), r#"{"web":{"client_id":"id"}}"#).unwrap();
        assert!(google_client(&dir).unwrap_err().ends_with("is not a \"Desktop app\" client file"));
        std::fs::write(dir.join("google_client.json"), r#"{"installed":{"client_id":"id","client_secret":"s","project_id":"p"}}"#).unwrap();
        assert_eq!(google_client(&dir).unwrap(), json!({ "client_id": "id", "client_secret": "s" }));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn sessions_tie_to_tasks_typed_merges_into_imported_and_courses_hide() {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        let typed = |title: &str, course: Option<&str>, due: Option<&str>, estimate: Option<i64>| {
            let t = NewTask { title: title.into(), course: course.map(Into::into), due_date: due.map(Into::into), estimate_min: estimate };
            insert_task(&conn, &t).unwrap()
        };
        let feed: Vec<FeedItem> = serde_json::from_value(json!([
            { "kind": "task", "uid": "event-assignment-1", "title": "Homework 3", "course": "STATISTC 310", "due_at": null, "due_date": "2030-01-11" },
            { "kind": "task", "uid": "event-assignment-2", "title": "Lab 2", "course": "BIOLOGY 151", "due_at": null, "due_date": "2030-01-11" },
            { "kind": "task", "uid": "event-assignment-3", "title": "Quiz A", "course": "COMPSCI 230", "due_at": null, "due_date": "2030-01-12" },
            { "kind": "task", "uid": "event-assignment-4", "title": "Quiz B", "course": "COMPSCI 230", "due_at": null, "due_date": "2030-01-12" }
        ]))
        .unwrap();
        sync_feed(&conn, "canvas", &feed, "2030-01-01T00:00:00Z").unwrap();
        let hw = typed("HW3", Some("STAT 310"), Some("2030-01-11"), Some(120));
        let other_day = typed("HW4", Some("STAT 310"), Some("2030-01-18"), None);
        let no_course = typed("read chapter", None, Some("2030-01-11"), None);
        let two_matches = typed("quiz", Some("CS 230"), Some("2030-01-12"), None);
        let find = |id: i64| tasks(&conn).unwrap().into_iter().find(|t| t.id == id);
        let canvas_hw = tasks(&conn).unwrap().into_iter().find(|t| t.title == "Homework 3").unwrap().id;

        // S-2: a block tied to a task counts toward it; one tied to a task that is gone is kept, untied
        let block = |task_id: Option<i64>| Session { started_at: 0.0, ended_at: 45.0 * 60.0, planned_min: 50.0, break_min: 10.0, label: None, task_id };
        save_session(&conn, &block(Some(hw))).unwrap();
        save_session(&conn, &block(Some(9999))).unwrap();
        save_session(&conn, &block(None)).unwrap();
        assert_eq!(find(hw).unwrap().spent_min.round(), 45.0);
        assert_eq!(conn.query_row("SELECT count(*) FROM session WHERE task_id IS NULL", [], |r| r.get::<_, i64>(0)).unwrap(), 2);

        // the task id makes the whole trip: into the Python timer with the start, back out with the finished block
        let backend = Backend::default();
        let start = json!({ "action": "start_focus", "focus_min": 25, "break_min": 5, "label": "HW3", "task_id": hw });
        assert_eq!(sidecar_call(&backend, "timer", Some(start)).unwrap()["task_id"], hw);
        let mut stopped = sidecar_call(&backend, "timer", Some(json!({ "action": "stop" }))).unwrap();
        let finished: Vec<Session> = serde_json::from_value(stopped["sessions"].take()).unwrap();
        assert_eq!(finished.iter().map(|s| s.task_id).collect::<Vec<_>>(), [Some(hw)]);

        // same due day and course number: suggested, and only then
        assert_eq!(find(hw).unwrap().same_as.map(|s| (s.id, s.title, s.source)), Some((canvas_hw, "Homework 3".into(), "canvas".into())));
        for id in [other_day, no_course, two_matches, canvas_hw] {
            assert!(find(id).unwrap().same_as.is_none(), "{id}");
        }

        // only a typed task merges into an imported one
        assert!(merge_tasks(&conn, canvas_hw, hw).is_err());
        assert!(merge_tasks(&conn, hw, other_day).is_err());
        merge_tasks(&conn, hw, canvas_hw).unwrap();
        assert!(find(hw).is_none());
        let merged = find(canvas_hw).unwrap();
        assert_eq!((merged.title.as_str(), merged.estimate_min, merged.spent_min.round(), merged.source.as_deref()), ("Homework 3", Some(120), 45.0, Some("canvas")));
        // the next sync updates the merged task in place and does not import a second copy
        let before = tasks(&conn).unwrap().len();
        sync_feed(&conn, "canvas", &feed, "2030-01-02T00:00:00Z").unwrap();
        assert_eq!((tasks(&conn).unwrap().len(), find(canvas_hw).unwrap().estimate_min), (before, Some(120)));

        // reminders: due date against the work left, each task once a day
        assert!(!due_soon(None, 60.0) && !due_soon(Some(-5.0), 60.0) && !due_soon(Some(120.0), 60.0) && due_soon(Some(119.0), 60.0));
        assert_eq!((work_left(None, 0.0), work_left(Some(120), 45.0), work_left(Some(30), 50.0)), (60.0, 75.0, 15.0));
        assert_eq!((minutes_text(150.0), minutes_text(45.4), minutes_text(120.0), minutes_text(0.2)), ("2 h 30 min".into(), "45 min".into(), "2 h".into(), "1 min".into()));
        assert_eq!(take_reminder(&conn).unwrap(), None); // everything so far is due in 2030
        let due_in = |title: &str, minutes: i64, estimate: Option<i64>| {
            let id = typed(title, None, None, estimate);
            let at = (chrono::Utc::now() + chrono::Duration::minutes(minutes)).format("%Y-%m-%dT%H:%M:%SZ").to_string();
            conn.execute("UPDATE task SET due_at = ?2 WHERE id = ?1", (id, at)).unwrap();
            id
        };
        let essay = due_in("Essay", 150, Some(120)); // 2 h 30 min left for 2 h of work
        let later = due_in("Reading", 150, None); // 2 h 30 min left for an assumed hour: not yet
        let passed = due_in("Missed", -10, Some(120));
        let done = due_in("Done already", 30, None);
        set_done(&conn, done, true).unwrap();
        assert_eq!([essay, later, passed, done].map(|id| find(id).unwrap().soon), [true, false, false, false]);
        assert_eq!(take_reminder(&conn).unwrap().as_deref(), Some("“Essay” is due in 2 h 30 min and needs about 2 h. Start now?"));
        assert_eq!(take_reminder(&conn).unwrap(), None); // said once today
        assert!(find(essay).unwrap().soon); // the mark on its row stays
        // time spent on the task shrinks the work left, and two at once are counted
        conn.execute("UPDATE task SET reminded_at = '2020-01-01T00:00:00Z'", []).unwrap();
        save_session(&conn, &block(Some(essay))).unwrap();
        due_in("A very long title that would not fit in the pet's little speech bubble", 20, None);
        assert_eq!(
            take_reminder(&conn).unwrap().as_deref(),
            Some("“A very long title that would not fit in…” is due in 20 min and needs about 1 h. Start now? 1 more is due soon.")
        );
        assert_eq!(find(essay).unwrap().work_min.round(), 75.0);

        // "Did you submit it?": asked once when an imported task's due time has passed; a typed one is never asked about
        let lab = tasks(&conn).unwrap().into_iter().find(|t| t.title == "Lab 2").unwrap().id;
        assert_eq!([lab, passed, essay].map(|id| find(id).unwrap().ask_submitted), [false, false, false]);
        conn.execute("UPDATE task SET due_date = '2021-03-04' WHERE id = ?1", [lab]).unwrap();
        assert!(find(lab).unwrap().ask_submitted);
        assert_eq!(take_reminder(&conn).unwrap().as_deref(), Some("“Lab 2” was due. Did you submit it?"));
        assert_eq!(take_reminder(&conn).unwrap(), None);
        assert!(find(lab).unwrap().ask_submitted); // the question stays on its row until it is answered
        // a new due date that passes is a new question, and a due-soon reminder goes first
        // (the first question is back-dated to just after the old due date, as it would be in real use)
        conn.execute(
            "UPDATE task SET due_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-1 minute'), due_date = NULL, reminded_at = '2021-03-06T00:00:00Z' WHERE id = ?1",
            [lab],
        )
        .unwrap();
        let quiz = tasks(&conn).unwrap().into_iter().find(|t| t.title == "Quiz A").unwrap().id;
        conn.execute("UPDATE task SET due_at = '2021-03-04T10:00:00Z', due_date = NULL WHERE id = ?1", [quiz]).unwrap();
        due_in("Problem set", 30, None);
        assert_eq!(take_reminder(&conn).unwrap().as_deref(), Some("“Problem set” is due in 30 min and needs about 1 h. Start now?"));
        assert_eq!(take_reminder(&conn).unwrap().as_deref(), Some("“Lab 2” was due. Did you submit it? 1 more to check."));
        set_done(&conn, lab, true).unwrap();
        assert!(!find(lab).unwrap().ask_submitted);

        // unticking a course hides its imported tasks only, and ticking it brings them back
        let shown = || json!(courses(&conn).unwrap());
        assert_eq!(shown()[0], json!({ "course": "COMPSCI 230", "open": 2, "hidden": false })); // the lab is done, so BIOLOGY 151 has nothing open
        set_course_shown(&conn, "COMPSCI 230", false).unwrap();
        set_course_shown(&conn, "COMPSCI 230", false).unwrap();
        let typed_230 = typed("my own", Some("COMPSCI 230"), None, None);
        assert_eq!(shown()[0], json!({ "course": "COMPSCI 230", "open": 2, "hidden": true }));
        let titles = || tasks(&conn).unwrap().into_iter().map(|t| t.title).collect::<Vec<_>>();
        assert!(!titles().contains(&"Quiz A".to_string()) && find(typed_230).is_some());
        set_course_shown(&conn, "COMPSCI 230", true).unwrap();
        assert!(titles().contains(&"Quiz A".to_string()) && titles().contains(&"Quiz B".to_string()));
    }
}
