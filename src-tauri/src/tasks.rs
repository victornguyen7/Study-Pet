use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::State;

use crate::db::{lock, setting, Db};
use crate::sidecar::{sidecar_call, Backend};

// ---- Tasks: quick-add parsing in backend/tasks.py ----

/// A typed line after backend/tasks.py has parsed it.
#[derive(Serialize, Deserialize)]
pub(crate) struct NewTask {
    pub(crate) title: String,
    pub(crate) course: Option<String>,
    pub(crate) due_date: Option<String>,
    pub(crate) estimate_min: Option<i64>,
}

pub(crate) fn insert_task(conn: &Connection, t: &NewTask) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO task (title, course, due_date, estimate_min) VALUES (?1, ?2, ?3, ?4)",
        (&t.title, &t.course, &t.due_date, t.estimate_min),
    )?;
    Ok(conn.last_insert_rowid())
}

/// `text` is one quick-add line, such as "STAT 310 HW3 Fri 2h". Returns the new task with its id.
#[tauri::command(async)]
pub(crate) fn add_task(db: State<Db>, backend: State<Backend>, text: String) -> Result<Value, String> {
    let today: String = lock(&db.0)
        .query_row("SELECT date('now', 'localtime')", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    let parsed = sidecar_call(&backend, "quick_add", Some(json!({ "text": text, "today": today })))?;
    let task: NewTask = serde_json::from_value(parsed).map_err(|e| e.to_string())?;
    let id = insert_task(&lock(&db.0), &task).map_err(|e| e.to_string())?;
    let mut task = json!(task);
    task["id"] = json!(id);
    Ok(task)
}

/// A row of the panel's task list. `due` is a local "YYYY-MM-DD", whichever due column is set.
#[derive(Serialize)]
pub(crate) struct Task {
    pub(crate) id: i64,
    pub(crate) title: String,
    pub(crate) course: Option<String>,
    pub(crate) due: Option<String>,
    pub(crate) estimate_min: Option<i64>,
    pub(crate) done: bool,
    pub(crate) source: Option<String>, // where it was imported from, such as "canvas"
    pub(crate) removed: bool,          // imported, and no longer in its source's feed
    pub(crate) spent_min: f64,         // focus time in sessions tied to it (S-2)
    pub(crate) same_as: Option<SameAs>, // on a typed task: the imported task that looks like the same piece of work
    pub(crate) left_min: Option<f64>,  // minutes until it is due; a task due on a day is due at the end of that day
    pub(crate) work_min: f64,          // the work still to do, as far as the estimate and the time spent can tell
    pub(crate) soon: bool,             // open, not yet due, and the time left is under twice the work left
    pub(crate) ask_submitted: bool,    // imported, open and past due: the feed can't say whether it was handed in, so the pet asks
    #[serde(skip)]
    pub(crate) reminded_today: bool,
    #[serde(skip)]
    pub(crate) asked_since_due: bool,
}

const ASSUMED_ESTIMATE_MIN: f64 = 60.0; // for a task with no estimate, which is most imported ones
const LEAST_WORK_MIN: f64 = 15.0; // a task that has used up its estimate but is not done still needs something
const MARGIN: f64 = 2.0;

/// The work a task still needs: its estimate less the focus time already spent on it.
pub(crate) fn work_left(estimate_min: Option<i64>, spent_min: f64) -> f64 {
    (estimate_min.map_or(ASSUMED_ESTIMATE_MIN, |m| m as f64) - spent_min).max(LEAST_WORK_MIN)
}

/// Spec: reminders are "timed by due date against the estimate, not by fixed alarms".
pub(crate) fn due_soon(left_min: Option<f64>, work_min: f64) -> bool {
    left_min.is_some_and(|left| left > 0.0 && left < MARGIN * work_min)
}

/// "2 h 30 min", "45 min".
pub(crate) fn minutes_text(minutes: f64) -> String {
    let m = minutes.round().max(1.0) as i64;
    match (m / 60, m % 60) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m} min"),
    }
}

/// What the pet says next, if anything: about the tasks that have just become due soon (each once a day),
/// or else about the imported tasks whose due time has just passed (each once). The ones spoken about are
/// marked, so the same thing is not said again.
pub(crate) fn take_reminder(conn: &Connection) -> rusqlite::Result<Option<String>> {
    let all = tasks(conn)?;
    let mut soon: Vec<&Task> = all.iter().filter(|t| t.soon && !t.reminded_today).collect();
    let mut asks: Vec<&Task> = all.iter().filter(|t| t.ask_submitted && !t.asked_since_due).collect();
    let nearest = |a: &&Task, b: &&Task| a.left_min.partial_cmp(&b.left_min).unwrap_or(std::cmp::Ordering::Equal);
    soon.sort_by(nearest); // the one due first is the one named
    asks.sort_by(|a, b| nearest(b, a)); // the one that passed most recently
    // ponytail: one kind per bubble. If both are waiting, the question comes at the next check, a minute
    // later, and takes the reminder's place in the bubble.
    let (spoken, text) = match (soon.first(), asks.first()) {
        (Some(first), _) => {
            let more = match soon.len() - 1 {
                0 => String::new(),
                1 => " 1 more is due soon.".into(),
                n => format!(" {n} more are due soon."),
            };
            let (left, work) = (minutes_text(first.left_min.unwrap_or(0.0)), minutes_text(first.work_min));
            let text = format!("“{}” is due in {left} and needs about {work}. Start now?{more}", short_title(&first.title));
            (soon, text)
        }
        (None, Some(first)) => {
            let more = match asks.len() - 1 {
                0 => String::new(),
                n => format!(" {n} more to check."),
            };
            let text = format!("“{}” was due. Did you submit it?{more}", short_title(&first.title));
            (asks, text)
        }
        (None, None) => return Ok(None),
    };
    for task in spoken {
        conn.execute("UPDATE task SET reminded_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = ?1", [task.id])?;
    }
    Ok(Some(text))
}

/// The bubble is small, so a long title is cut.
pub(crate) fn short_title(title: &str) -> String {
    if title.chars().count() > 40 {
        title.chars().take(39).chain(['…']).collect()
    } else {
        title.into()
    }
}

#[derive(Serialize, Clone)]
pub(crate) struct SameAs {
    pub(crate) id: i64,
    pub(crate) title: String,
    pub(crate) source: String,
}

/// "STAT 310" and "STATISTC 310" are the same course to a student: compare the number alone.
pub(crate) fn course_number(course: &str) -> String {
    course.chars().skip_while(|c| !c.is_ascii_digit()).take_while(|c| c.is_ascii_digit()).collect()
}

/// Open tasks by due date (undated last), then the ones checked off today.
/// An imported task whose course is unticked in Settings is left out.
pub(crate) fn tasks(conn: &Connection) -> rusqlite::Result<Vec<Task>> {
    let mut tasks: Vec<Task> = conn
        .prepare(
            "SELECT id, title, course, coalesce(due_date, date(due_at, 'localtime')) AS due, estimate_min, status = 'done',
                    (SELECT min(source) FROM task_source_link WHERE task_id = task.id) AS source,
                    EXISTS (SELECT 1 FROM task_source_link WHERE task_id = task.id AND removed_at IS NOT NULL),
                    coalesce((SELECT sum((julianday(ended_at) - julianday(started_at)) * 1440) FROM session WHERE task_id = task.id), 0.0),
                    (julianday(coalesce(due_at, datetime(due_date, '+1 day', 'utc'))) - julianday('now')) * 1440,
                    coalesce(date(reminded_at, 'localtime') = date('now', 'localtime'), 0),
                    coalesce(julianday(reminded_at) >= julianday(coalesce(due_at, datetime(due_date, '+1 day', 'utc'))), 0)
             FROM task
             WHERE hidden_at IS NULL AND (status = 'todo' OR date(done_at, 'localtime') = date('now', 'localtime'))
               AND NOT (EXISTS (SELECT 1 FROM task_source_link WHERE task_id = task.id) AND coalesce(course, '') IN (
                    SELECT value FROM json_each(coalesce((SELECT value FROM setting WHERE key = 'hidden_courses'), '[]'))))
             ORDER BY status = 'done', due IS NULL, due, id",
        )?
        .query_map([], |r| {
            let (estimate_min, done, spent_min): (Option<i64>, bool, f64) = (r.get(4)?, r.get(5)?, r.get(8)?);
            let left_min: Option<f64> = r.get(9)?;
            let work_min = work_left(estimate_min, spent_min);
            let source: Option<String> = r.get(6)?;
            Ok(Task {
                id: r.get(0)?,
                title: r.get(1)?,
                course: r.get(2)?,
                due: r.get(3)?,
                estimate_min,
                done,
                ask_submitted: !done && source.is_some() && left_min.is_some_and(|left| left <= 0.0),
                asked_since_due: r.get(11)?,
                source,
                removed: r.get(7)?,
                spent_min,
                same_as: None,
                left_min,
                work_min,
                soon: !done && due_soon(left_min, work_min),
                reminded_today: r.get(10)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;

    // A typed task and an imported one are suggested as the same work when both are open, due the same day
    // and in a course with the same number. The user confirms; nothing merges by itself.
    // ponytail: two imported matches (two things due that day in one course) means no suggestion at all.
    // Compare titles too if that turns out to be common.
    let key = |t: &Task| match (&t.due, t.course.as_deref().map(course_number)) {
        (Some(due), Some(number)) if !t.done && !number.is_empty() => Some((due.clone(), number)),
        _ => None,
    };
    let imported: Vec<_> = tasks.iter().filter_map(|t| Some((key(t)?, t.id, t.title.clone(), t.source.clone()?))).collect();
    for task in tasks.iter_mut().filter(|t| t.source.is_none()) {
        let Some(k) = key(task) else { continue };
        let mut matches = imported.iter().filter(|(other, ..)| *other == k);
        if let (Some((_, id, title, source)), None) = (matches.next(), matches.next()) {
            task.same_as = Some(SameAs { id: *id, title: title.clone(), source: source.clone() });
        }
    }
    Ok(tasks)
}

/// Makes a typed task and an imported one a single task: the imported row stays, so its source link and the
/// title, course and due date the source owns stay with it, and it takes the typed task's estimate (if it
/// had one) and its focus time. There is no undo.
pub(crate) fn merge_tasks(conn: &Connection, typed: i64, imported: i64) -> rusqlite::Result<()> {
    let tx = conn.unchecked_transaction()?;
    let linked = |id: i64| tx.query_row("SELECT EXISTS (SELECT 1 FROM task_source_link WHERE task_id = ?1)", [id], |r| r.get::<_, bool>(0));
    if linked(typed)? || !linked(imported)? {
        return Err(rusqlite::Error::InvalidQuery); // not a typed task and an imported one
    }
    tx.execute(
        "UPDATE task SET estimate_min = coalesce((SELECT estimate_min FROM task WHERE id = ?1), estimate_min)
         WHERE id = ?2",
        (typed, imported),
    )?;
    tx.execute("UPDATE session SET task_id = ?2 WHERE task_id = ?1", (typed, imported))?;
    if tx.execute("DELETE FROM task WHERE id = ?1", [typed])? == 0 {
        return Err(rusqlite::Error::QueryReturnedNoRows);
    }
    tx.commit()
}

/// The courses open imported tasks belong to, with whether each is shown. Unticking one in Settings hides
/// its imported tasks from the list; nothing is deleted, and ticking it again brings them back.
pub(crate) fn courses(conn: &Connection) -> rusqlite::Result<Vec<Value>> {
    conn.prepare(
        "SELECT course, count(*),
                course IN (SELECT value FROM json_each(coalesce((SELECT value FROM setting WHERE key = 'hidden_courses'), '[]')))
         FROM task
         WHERE course IS NOT NULL AND hidden_at IS NULL AND status = 'todo'
           AND EXISTS (SELECT 1 FROM task_source_link WHERE task_id = task.id)
         GROUP BY course ORDER BY course",
    )?
    .query_map([], |r| Ok(json!({ "course": r.get::<_, String>(0)?, "open": r.get::<_, i64>(1)?, "hidden": r.get::<_, bool>(2)? })))?
    .collect()
}

pub(crate) fn set_course_shown(conn: &Connection, course: &str, shown: bool) -> rusqlite::Result<()> {
    let saved = setting(conn, "hidden_courses")?.unwrap_or_else(|| "[]".into());
    let mut hidden: Vec<String> = serde_json::from_str(&saved).unwrap_or_default();
    hidden.retain(|c| c != course);
    if !shown {
        hidden.push(course.into());
    }
    conn.execute("INSERT OR REPLACE INTO setting (key, value) VALUES ('hidden_courses', ?1)", [json!(hidden).to_string()])?;
    Ok(())
}

pub(crate) fn set_done(conn: &Connection, id: i64, done: bool) -> rusqlite::Result<usize> {
    conn.execute(
        "UPDATE task SET status = iif(?2, 'done', 'todo'),
                         done_at = iif(?2, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), NULL)
         WHERE id = ?1",
        (id, done),
    )
}

#[tauri::command]
pub(crate) fn list_tasks(db: State<Db>) -> Result<Vec<Task>, String> {
    tasks(&lock(&db.0)).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn set_task_done(db: State<Db>, id: i64, done: bool) -> Result<(), String> {
    match set_done(&lock(&db.0), id, done) {
        Ok(0) => Err(format!("no task with id {id}")),
        Ok(_) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

/// Deletes a task the user typed. An imported one is only hidden: deleting it would drop its source
/// link, and the next sync would import it again as new.
pub(crate) fn remove_task(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    let hidden = conn.execute(
        "UPDATE task SET hidden_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
         WHERE id = ?1 AND EXISTS (SELECT 1 FROM task_source_link WHERE task_id = ?1)",
        [id],
    )?;
    if hidden == 0 {
        conn.execute("DELETE FROM task WHERE id = ?1", [id])?;
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn delete_task(db: State<Db>, id: i64) -> Result<(), String> {
    remove_task(&lock(&db.0), id).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn merge_task(db: State<Db>, typed: i64, imported: i64) -> Result<(), String> {
    merge_tasks(&lock(&db.0), typed, imported).map_err(|_| "those two tasks can't be merged".to_string())
}

#[tauri::command]
pub(crate) fn list_courses(db: State<Db>) -> Result<Vec<Value>, String> {
    courses(&lock(&db.0)).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn set_course(db: State<Db>, course: String, shown: bool) -> Result<(), String> {
    set_course_shown(&lock(&db.0), &course, shown).map_err(|e| e.to_string())
}
