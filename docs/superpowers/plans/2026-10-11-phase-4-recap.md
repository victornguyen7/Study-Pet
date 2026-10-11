# Phase 4 · Recap Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The Wrap up view shows a focus heatmap and, for today or any earlier day, the hours per mode and one total per app and site, with sites shown by name; "What the pet has learned" folds shut.

**Architecture:** Nothing new is stored. `label_day` in `backend/modes.py` already cuts each browser span around its pages, so it also returns one total per app and site. Rust picks the rows of one local day, sends them to `label_day`, and replaces each site's address with a name before the panel sees it. The panel draws the heatmap from the focus minutes per day that the `day` command already returns.

**Tech Stack:** Python 3 standard library (`backend/`), Rust with `rusqlite` and `serde_json` (`src-tauri/`), plain TypeScript, HTML and CSS (`src/panel/`, `panel.html`). No new package, table or process.

**Spec:** `docs/spec.md`, section "Wrap up and recap" (R-1 to R-6). Task list: `docs/ROADMAP.md`, "Phase 4 · Recap".

## Global Constraints

- No new dependency, database table, migration or process.
- Changes to what `backend/` returns must only **add** keys. The installed everyday app starts Python from this source tree (`KNOWN_ISSUES.md`, item 33), so a renamed or removed key breaks it at once.
- Do not rebuild or reinstall the everyday app in `/Applications`. Run and check with the dev app only (`npm run tauri dev`), which has its own database in the project root.
- A site's address is never shown in the panel or in the pet's bubble: always its name (R-5). The Chrome extension's own popup is out of scope and still lists sites by address.
- Heatmap shades (R-1): none, under 1 h, 1 to 2 h, 2 to 4 h, 4 h or more.
- Local days are `"YYYY-MM-DD"` strings in the laptop's time zone, as `date(..., 'localtime')` gives them in SQLite and `toLocaleDateString("en-CA")` in the panel.
- macOS only. Light and dark mode both have to read well.
- Match the surrounding code: comments explain why in plain words, Rust tests live in the `tests` module of `src-tauri/src/lib.rs`, Python tests in `backend/tests/`.
- Colours below were checked with the dataviz palette validator against the panel's surfaces (`#ffffff` light, `#18323b` dark). Do not change them without running it again.

**Check commands** (the third runs from the project root):

    npm run build
    cd src-tauri && cargo test
    backend/.venv/bin/python -m unittest backend.tests.test_timer backend.tests.test_streaks backend.tests.test_tasks backend.tests.test_feeds backend.tests.test_fetcher backend.tests.test_modes

## Review Focus

1. **An address with no name in it** (`127.0.0.1`, `localhost`, an empty string, a non-ASCII host): it is shown as it is or capitalised, and nothing panics. Pinned in Task 2.
2. **Two addresses of one site** (`www.youtube.com` and `m.youtube.com`): one row with the times added, not two rows named "YouTube". Pinned in Task 2.
3. **An answer saved before sites had names** (an `app_rule` row whose name is the address): the learned section shows the name. Pinned in Task 2.
4. **A day with nothing noted** (any day before 2026-10-07, or a day the laptop was shut): empty lists and a hidden bar, no error. Pinned in Tasks 1 and 3.
5. **A day string that is no day** (`"not a day"`, `""`): an empty recap, never today's numbers under another date. Pinned in Task 3.

## File Structure

| File | Change |
| --- | --- |
| `backend/modes.py` | `label_day` also returns `usage`; each `unknown` row says whether it is a site |
| `backend/tests/test_modes.py` | Tests for both |
| `src-tauri/src/modes.rs` | `site_name`, `name_sites`, `ON_DAY`; `mode_input` takes a day; `modes_today` becomes `modes_day`; `rules` names old site rows |
| `src-tauri/src/monitor.rs` | `audio_time` takes a day; `app_time` removed; the drift line names the site |
| `src-tauri/src/browser.rs` | `site_time` removed |
| `src-tauri/src/lib.rs` | Command list; tests |
| `panel.html` | Wrap up sections; folding learned section |
| `src/panel/main.ts` | Recap, heatmap, day picking, learned count |
| `src/panel/panel.css` | Mode bar, heatmap, folding section |
| `docs/` | `CHANGELOG.md`, `KNOWN_ISSUES.md`, `ARCHITECTURE.md`, `ROADMAP.md` |

**Before Task 1:** the docs for the new plan are committed (`be3e62a`). Only this plan file is not; commit it first:

```bash
git add docs/superpowers
git commit -m "plan for phase 4, the recap"
```

---

### Task 1: One total per app and site, from `label_day`

**Files:**
- Modify: `backend/modes.py` (`label_day`, plus one new function above it)
- Test: `backend/tests/test_modes.py`

**Interfaces:**
- Consumes: `timeline(apps, pages)`, already in `backend/modes.py`.
- Produces: `label_day(...)` returns two things more:
  - `"usage": [{"name": str, "site": bool, "seconds": int}]`, longest first. `name` is the site's address or the app's name, exactly as stored. Rows of zero seconds are left out.
  - each row of `"unknown"` gains `"site": bool`.

- [ ] **Step 1: Write the failing tests**

In `backend/tests/test_modes.py`, add this method to `ModesTest`:

```python
    def test_usage_is_one_total_per_app_and_site(self):
        chrome = "com.google.Chrome"
        day = label_day(
            [app(0, 10, "Google Chrome", chrome), app(10, 15, "Preview"), app(20, 30, "Google Chrome", chrome)],
            # the second page has no length, and is left out
            [page(2, 5, "a.edu"), page(5, 5, "b.edu"), page(22, 30, "a.edu")],
        )
        # the browser's own row is only the time not under one of its pages
        self.assertEqual(day["usage"], [
            {"name": "a.edu", "site": True, "seconds": 11 * MIN},
            {"name": "Google Chrome", "site": False, "seconds": 9 * MIN},
            {"name": "Preview", "site": False, "seconds": 5 * MIN},
        ])
        # so every minute in front is counted once
        self.assertEqual(sum(u["seconds"] for u in day["usage"]), 25 * MIN)
        # a day with nothing noted
        self.assertEqual(label_day([], []), {"spans": [], "totals": {}, "unknown": [], "usage": []})
```

In the existing `test_what_to_ask_about`, replace the expected list with:

```python
        self.assertEqual(day["unknown"], [
            {"key": "com.apple.preview", "name": "Preview", "site": False, "seconds": 12 * MIN},
            {"key": "news.example.com", "name": "news.example.com", "site": True, "seconds": 5 * MIN},
        ])
```

- [ ] **Step 2: Run the tests to see them fail**

Run from the project root: `backend/.venv/bin/python -m unittest backend.tests.test_modes`
Expected: 2 failures, `KeyError: 'usage'` and a list that differs by the missing `'site'` key.

- [ ] **Step 3: Implement**

In `backend/modes.py`, add above `label_day`:

```python
def usage(spans):
    """spans: timeline()'s pieces. -> [{name, site, seconds}], longest first: one total per app and site.
    A page has already been cut out of its browser's span, so no time is counted twice."""
    totals = {}
    for s in spans:
        row = (s.get("domain") or s["app"], bool(s.get("domain")))
        totals[row] = totals.get(row, 0) + s["end"] - s["start"]
    return [
        {"name": name, "site": site, "seconds": seconds}
        for (name, site), seconds in sorted(totals.items(), key=lambda t: -t[1])
        if seconds > 0
    ]
```

In `label_day`, build the timeline once and use it twice. Replace the docstring's return line, the `for` line, the `ask = ...` line and the `return` line so the function reads:

```python
def label_day(apps, pages, sessions=(), declarations=(), rules=None):
    """apps: [{start, end, app, bundle_id}], pages: [{start, end, domain}], sessions: [{start, end}],
    declarations: [{start, end, mode}], rules: {key: mode}. Times are epoch seconds.
    -> {"spans": [{start, end, mode, source, what: the apps and sites in it}], "totals": {mode or "unknown": seconds},
        "unknown": [{key, name, site, seconds}], longest first: what to ask the user about,
        "usage": [{name, site, seconds}], longest first: the time in each app and site}"""
    pieces = timeline(apps, pages)
    labeled, unknown = [], {}
    for span in _cut(pieces, [*sessions, *declarations]):
        declared = _covering(declarations, span)
        mode, source = classify(span, declared and declared["mode"], rules, bool(_covering(sessions, span)))
        what = span.get("domain") or span["app"]
        labeled.append({"start": span["start"], "end": span["end"], "mode": mode, "source": source, "what": [what]})
        # a guess from context is still a guess, so it is asked about too
        if source in (None, "context", "declared") and key(span) not in BROWSERS | (rules or {}).keys() and not category(span):
            ask = unknown.setdefault(key(span), {"key": key(span), "name": what, "site": bool(span.get("domain")), "seconds": 0})
            ask["seconds"] += span["end"] - span["start"]
    spans = smooth(labeled)
    totals = {}
    for s in spans:
        name = s["mode"] or "unknown"
        totals[name] = totals.get(name, 0) + s["end"] - s["start"]
    asks = sorted((u for u in unknown.values() if u["seconds"] >= ASK_SECS), key=lambda u: -u["seconds"])
    return {"spans": spans, "totals": totals, "unknown": asks, "usage": usage(pieces)}
```

- [ ] **Step 4: Run the tests to see them pass**

Run: `backend/.venv/bin/python -m unittest backend.tests.test_modes`
Expected: `OK`, 9 tests.

- [ ] **Step 5: Commit**

```bash
git add backend/modes.py backend/tests/test_modes.py
git commit -m "label_day returns one total per app and site"
```

---

### Task 2: Names for sites (R-5)

**Files:**
- Modify: `src-tauri/src/modes.rs` (new section after the `rules` function; `rules` itself)
- Test: `src-tauri/src/lib.rs` (`tests` module)

**Interfaces:**
- Consumes: the `usage` and `unknown` rows of Task 1: `{name, site, seconds}` and `{key, name, site, seconds}`.
- Produces, all in `crate::modes`:
  - `pub(crate) fn site_name(domain: &str) -> String`
  - `pub(crate) fn name_sites(day: &mut Value)`: in a `label_day` reply, renames every site in `unknown`, and rewrites `usage` as `[{ "name": String, "seconds": i64 }]`, one row per name, longest first.
  - `rules(conn)` returns a name, not an address, for a site.

- [ ] **Step 1: Write the failing test**

In `src-tauri/src/lib.rs`, add to the `tests` module:

```rust
    #[test]
    fn sites_are_shown_by_name() {
        use modes::{name_sites, rules, set_rule, site_name};
        for (domain, name) in [
            ("www.youtube.com", "YouTube"),
            ("m.youtube.com", "YouTube"),
            ("music.youtube.com", "YouTube Music"), // the longest match wins
            ("WWW.GitHub.com", "GitHub"),
            ("en.wikipedia.org", "Wikipedia"),
            ("docs.google.com", "Google Docs"),
            ("netflix.com", "Netflix"), // not "X", though it ends in "x.com"
            // nothing on the list: the first part of the address
            ("canvas.auckland.ac.nz", "Canvas"),
            ("example.com", "Example"),
            ("ünï.example", "Ünï"),
            // no name in it: left as it is
            ("localhost", "localhost"),
            ("127.0.0.1", "127.0.0.1"),
            ("", ""),
        ] {
            assert_eq!(site_name(domain), name, "{domain}");
        }

        let mut day = json!({
            "unknown": [
                { "key": "news.example.com", "name": "news.example.com", "site": true, "seconds": 300 },
                { "key": "com.apple.preview", "name": "Preview", "site": false, "seconds": 200 },
            ],
            "usage": [
                { "name": "www.youtube.com", "site": true, "seconds": 600 },
                { "name": "Google Chrome", "site": false, "seconds": 500 },
                { "name": "m.youtube.com", "site": true, "seconds": 100 },
                { "name": "example.com", "site": false, "seconds": 50 }, // an app named like a site keeps its name
            ],
        });
        name_sites(&mut day);
        assert_eq!((day["unknown"][0]["name"].clone(), day["unknown"][1]["name"].clone()), (json!("News"), json!("Preview")));
        // two addresses of one site are one row
        assert_eq!(
            day["usage"],
            json!([{ "name": "YouTube", "seconds": 700 }, { "name": "Google Chrome", "seconds": 500 }, { "name": "example.com", "seconds": 50 }])
        );
        // a day with nothing noted
        let mut empty = json!({ "unknown": [], "usage": [] });
        name_sites(&mut empty);
        assert_eq!(empty, json!({ "unknown": [], "usage": [] }));

        // an answer saved before sites had names is stored under its address; an app keeps its name
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        set_rule(&conn, "www.reddit.com", "www.reddit.com", "relax").unwrap();
        set_rule(&conn, "com.apple.preview", "Preview", "study").unwrap();
        assert_eq!(
            rules(&conn).unwrap(),
            [
                json!({ "key": "com.apple.preview", "name": "Preview", "mode": "study" }),
                json!({ "key": "www.reddit.com", "name": "Reddit", "mode": "relax" }),
            ]
        );
    }
```

- [ ] **Step 2: Run the test to see it fail**

Run: `cd src-tauri && cargo test sites_are_shown_by_name`
Expected: it does not compile, `unresolved imports modes::name_sites, modes::site_name`.

- [ ] **Step 3: Implement**

In `src-tauri/src/modes.rs`, replace the `rules` function with:

```rust
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
```

- [ ] **Step 4: Run the tests to see them pass**

Run: `cd src-tauri && cargo test`
Expected: every test passes, `sites_are_shown_by_name` among them. A warning that `name_sites` is never used is expected until Task 3.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/modes.rs src-tauri/src/lib.rs
git commit -m "site names: a built-in list, else the first part of the address"
```

---

### Task 3: The recap of any day, with one bar and one list (R-2 backend, R-3, R-4, R-5)

The commands and the panel change together here, because the panel calls the commands by name.

**Files:**
- Modify: `src-tauri/src/modes.rs` (`mode_input`, `modes_today`, `question`, `offer`)
- Modify: `src-tauri/src/monitor.rs` (`audio_time_today`, `audio_time`, `app_time_today`, `app_time`, the drift state in `start`)
- Modify: `src-tauri/src/browser.rs` (`site_time_today`, `site_time`)
- Modify: `src-tauri/src/lib.rs` (the command list in `run`; three tests)
- Modify: `panel.html` (the Wrap up view), `src/panel/main.ts`, `src/panel/panel.css`

**Interfaces:**
- Consumes: `name_sites(day: &mut Value)` and `site_name(domain: &str) -> String` from Task 2; `usage` from Task 1.
- Produces:
  - `pub(crate) const ON_DAY: &str` in `crate::modes`: an SQL test on `started_at` whose `?1` is a local day or NULL for today.
  - `pub(crate) fn mode_input(conn: &Connection, day: Option<&str>) -> rusqlite::Result<Value>`
  - Tauri command `modes_day(day: Option<String>)` in place of `modes_today`. It returns `label_day`'s reply after `name_sites`: `{ spans, totals, unknown: [{key, name, site, seconds}], usage: [{name, seconds}] }`.
  - Tauri command `audio_time(day: Option<String>) -> i64`.
  - The commands `site_time` and `app_time` no longer exist.
  - In the panel: `let recapDay: string | null` (null means today) and `async function loadRecap()`, which Task 4 uses.

- [ ] **Step 1: Change the Rust tests first**

In `src-tauri/src/lib.rs`, test `pages_join_into_spans_and_only_web_addresses_are_kept`: change the import to `use browser::{read_frames, record_page};` and delete these five lines:

```rust
        // run just after local midnight, the spans fall on yesterday and today's list is empty
        let today: bool = conn.query_row("SELECT date(?1, 'unixepoch', 'localtime') = date('now', 'localtime')", [t], |r| r.get(0)).unwrap();
        if today {
            assert_eq!(site_time_today(&conn).unwrap(), [json!({ "domain": "canvas.edu", "seconds": 10 }), json!({ "domain": "docs.rs", "seconds": 5 })]);
        }
```

Test `front_app_samples_join_into_spans`: change the import to `use monitor::{audio_time_on, gate, record_app, record_audio};`, delete the three-line `if today { assert_eq!(app_time_today(...)...) }` block (keep the `let today` line above it), and replace the last `if today { ... }` block with:

```rust
        if today {
            assert_eq!(audio_time_on(&conn, None).unwrap(), 15);
        }
        // a day with nothing noted, and a string that is no day
        assert_eq!((audio_time_on(&conn, Some("2000-01-01")).unwrap(), audio_time_on(&conn, Some("not a day")).unwrap()), (0, 0));
```

Test `todays_spans_are_labeled_by_the_python_rules`: change every `mode_input(&conn)` to `mode_input(&conn, None)` (four places), and after the line that asserts on `input["apps"][1]["bundle_id"]` add:

```rust
        // R-2: any local day can be asked for. One with nothing noted, or a string that is no day, is empty
        let that_day: String = conn.query_row("SELECT date(?1, 'unixepoch', 'localtime')", [t], |r| r.get(0)).unwrap();
        assert_eq!(mode_input(&conn, Some(&that_day)).unwrap()["pages"].as_array().unwrap().len(), 1);
        for day in ["2000-01-01", "not a day", ""] {
            let empty = mode_input(&conn, Some(day)).unwrap();
            assert_eq!((empty["apps"].clone(), empty["pages"].clone(), empty["declarations"].clone()), (json!([]), json!([]), json!([])), "{day:?}");
        }
```

In the same test, inside `if today { ... }` where the sidecar labels the day, add after the `day["spans"][0]["source"]` assertion:

```rust
            // the time per app and site, with the site by name
            let mut day = day;
            modes::name_sites(&mut day);
            assert_eq!(day["usage"], json!([{ "name": "Canvas", "seconds": 600 }, { "name": "Preview", "seconds": 300 }]));
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `cd src-tauri && cargo test`
Expected: it does not compile: `audio_time_on` is not found, and `mode_input` takes 1 argument.

- [ ] **Step 3: Rust, `modes.rs`**

Replace `mode_input` and `modes_today` with:

```rust
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
```

```rust
/// One local day ("YYYY-MM-DD"; today when `day` is None): its labeled stretches, the seconds per
/// mode, what to ask about, and the time per app and site, with every site by name (R-5).
#[tauri::command(async)]
pub(crate) fn modes_day(app: AppHandle, day: Option<String>) -> Result<Value, String> {
    let input = mode_input(&lock(&app.state::<Db>().0), day.as_deref()).map_err(|e| e.to_string())?;
    let mut labeled = sidecar_call(&app.state::<Backend>(), "modes", Some(input))?;
    name_sites(&mut labeled);
    Ok(labeled)
}
```

Leave `rule_map`, which sits between them, as it is. In `question` change `modes_today(app.clone())?` to `modes_day(app.clone(), None)?`, and in `offer` change `modes_today(app.clone())?` to `modes_day(app.clone(), None)?`.

- [ ] **Step 4: Rust, `monitor.rs` and `browser.rs`**

In `monitor.rs`, replace `audio_time_today` and `audio_time` with:

```rust
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
```

Delete `app_time_today` and `app_time` from `monitor.rs`, with their doc comment (`/// Today's seconds per app, longest first.`).

In `monitor.rs`, in `start`, the drift state carries what the pet's line names. Change

```rust
                let state = sidecar_call(&app.state::<Backend>(), "drift", Some(front))?;
```

to

```rust
                let mut state = sidecar_call(&app.state::<Backend>(), "drift", Some(front))?;
                // R-5: the pet's line names the site, never its address
                if let (Some(site), true) = (&site, state["what"].is_string()) {
                    state["what"] = json!(crate::modes::site_name(site));
                }
```

Delete `site_time_today` and `site_time` from `browser.rs`, with their doc comment (`/// Today's seconds per site, longest first.`). Then remove from the `use` lines at the top of `browser.rs` and `monitor.rs` whatever the compiler now reports as unused (in `browser.rs` that is `State`).

- [ ] **Step 5: Rust, the command list**

In `src-tauri/src/lib.rs`, in `generate_handler!`: change `modes::modes_today,` to `modes::modes_day,`, and delete these four lines:

```rust
            #[cfg(unix)]
            browser::site_time,
            #[cfg(target_os = "macos")]
            monitor::app_time,
```

- [ ] **Step 6: Run the Rust tests to see them pass**

Run: `cd src-tauri && cargo test`
Expected: every test passes, with no warnings about unused code.

- [ ] **Step 7: The Wrap up markup**

In `panel.html`, inside `<main id="wrapup" hidden>`, replace the three `<section>` blocks that hold "Time by mode today" with "Stretches", "Sites today" and "Apps today" (keep the "Last 7 days" section above them) with:

```html
      <section>
        <div class="row head">
          <h2 id="recap-title">Today</h2>
          <button class="refresh" aria-label="Refresh the recap">Refresh</button>
        </div>
        <div id="mode-bar" role="img"></div>
        <ol id="balance" class="rows"></ol>
        <p class="muted">
          How the pet labeled the day, from the apps and sites below. “Unknown” is time in something the pet could
          not place. A wrong label? Answer the pet's question in Today, or change the answer in Settings.
        </p>
      </section>
      <section>
        <h2>Apps and sites</h2>
        <ol id="usage" class="rows"></ol>
        <p id="audio-today"></p>
        <p class="muted">
          The time each app and site was in front, checked every 5 seconds: its name and the time only. A site is
          listed only if you said yes to it in the Chrome extension; the rest of the browser's time is under the
          browser. Password managers, chat and mail apps are counted together as “Private”. Not during breaks, and
          only while Study Pet is running. Sound is noted as on or off, never what was playing.
        </p>
      </section>
```

- [ ] **Step 8: The panel code**

In `src/panel/main.ts`, in `loadAsks`, change `("modes_today")` to `("modes_day")`.

Replace everything from `function timeRows(` down to the closing brace of `async function loadSites() { ... }` (this takes out `timeRows`, the two "Mirrors" comments, `ModeDay`, `hhmm`, `modeName`, `loadBalance` and `loadSites`) with:

```ts
function timeRows(list: string, rows: [name: string, seconds: number, className?: string][]) {
  $(list).replaceChildren(
    ...rows.map(([name, seconds, className]) => {
      const li = el("li", "", className);
      li.append(el("span", name), el("span", duration(seconds / 60)));
      return li;
    }),
  );
  if (!rows.length) $(list).append(el("li", "Nothing noted.", "empty"));
}

// Mirrors label_day() in backend/modes.py, after name_sites() in src-tauri/src/modes.rs
type ModeDay = { totals: Record<string, number>; usage: { name: string; seconds: number }[] };
const MODE_ORDER = ["study", "work", "relax", "private", "unknown"];
const modeName = (mode: string) => mode[0].toUpperCase() + mode.slice(1);
// R-2: the day the recap shows, "YYYY-MM-DD"; null is today
let recapDay: string | null = null;

// R-3 and R-4: one bar of the day's modes, and one total per app and site
async function loadRecap() {
  const day = await invoke<ModeDay>("modes_day", { day: recapDay });
  const modes = MODE_ORDER.filter((m) => day.totals[m]);
  const bar = $("mode-bar");
  bar.hidden = !modes.length;
  bar.setAttribute("aria-label", modes.map((m) => `${modeName(m)} ${duration(day.totals[m] / 60)}`).join(", "));
  bar.replaceChildren(
    ...modes.map((m) => {
      const part = el("span", "", `mode-${m}`);
      part.style.flexGrow = String(day.totals[m]);
      part.title = `${modeName(m)}: ${duration(day.totals[m] / 60)}`;
      return part;
    }),
  );
  // the list under the bar is its legend: a swatch, the mode and the time
  timeRows("balance", modes.map((m) => [modeName(m), day.totals[m], `mode-${m}`]));
  // under a minute is a glance, and a long tail of them hides the day
  timeRows("usage", day.usage.filter((u) => u.seconds >= 60).map((u) => [u.name, u.seconds]));
  // PR-11: only that sound was on
  const audio = await invoke<number>("audio_time", { day: recapDay });
  $("audio-today").textContent = audio >= 60 ? `Sound on for ${duration(audio / 60)}.` : "";
}
```

In `loadDay`, change `await loadSites();` to `await loadRecap();`.

- [ ] **Step 9: The styles**

In `src/panel/panel.css`, add to the `:root` block:

```css
  /* the recap's modes; checked against --surface with the dataviz palette validator */
  --study: #2a78d6;
  --work: #eb6834;
  --relax: #1baf7a;
```

and to the `:root` block inside `@media (prefers-color-scheme: dark)`:

```css
    --study: #3987e5;
    --work: #d95926;
    --relax: #199e70;
```

Delete the rule `ol + .head { margin-top: 10px; }` with its comment about Stretches, and add under `/* ---- wrap up ---- */`:

```css
/* ---- recap: one bar of the day's modes; the list under it is the legend ---- */

.mode-study {
  --mode: var(--study);
}

.mode-work {
  --mode: var(--work);
}

.mode-relax {
  --mode: var(--relax);
}

/* not kinds of work: Private and unknown time stay grey */
.mode-private {
  --mode: var(--muted);
}

.mode-unknown {
  --mode: var(--line);
}

#mode-bar {
  display: flex;
  gap: 2px;
  height: 14px;
  margin: 10px 0 4px;
}

#mode-bar span {
  min-width: 3px;
  border-radius: 4px;
  background: var(--mode);
}

#balance li::before {
  content: "";
  flex: none;
  width: 10px;
  height: 10px;
  border-radius: 3px;
  background: var(--mode);
}
```

- [ ] **Step 10: Check and look**

Run: `npm run build`
Expected: the type check and the build pass.

Run: `npm run tauri dev`, open the panel's Wrap up view, and check:
- One bar and a list of modes with a coloured square each; the times match the bar's widths.
- "Apps and sites" has one row per app and site, longest first, no address anywhere, and no "Stretches" list.
- Hovering a part of the bar names its mode and time.
- In Today, a question under "What are these for?" names a site, not its address.
- Start a 1-minute focus block and stay on a Relax site for 2 minutes: the pet's bubble names the site.

- [ ] **Step 11: Commit**

```bash
git add src-tauri/src panel.html src/panel
git commit -m "recap: one bar of modes and one list of apps and sites, for any day"
```

---

### Task 4: The focus heatmap, and picking a day (R-1, R-2)

**Files:**
- Modify: `panel.html` (the "Last 7 days" section), `src/panel/main.ts` (`loadDay`), `src/panel/panel.css` (`#week` rules)

**Interfaces:**
- Consumes: `recapDay` and `loadRecap()` from Task 3; the `Day` type already in `main.ts` (`minutes: Record<string, number>`, `today: string`); the `streak` command, whose answer redraws the whole day through `onStreak` and `loadDay`.
- Produces: nothing later tasks use.

There is no automated test for the panel (`KNOWN_ISSUES.md`, item 24): the check is the type check and the running app.

- [ ] **Step 1: The markup**

In `panel.html`, replace the whole "Last 7 days" `<section>` with:

```html
      <section>
        <div class="row head">
          <h2>Focus</h2>
          <button class="refresh" aria-label="Refresh Focus">Refresh</button>
        </div>
        <div id="heat"></div>
        <p class="muted heat-key">
          Less <span class="l0"></span><span class="l1"></span><span class="l2"></span><span class="l3"></span><span class="l4"></span>
          More · under 1 h, 1 to 2 h, 2 to 4 h, 4 h or more of focus. Click a day to see it below.
        </p>
      </section>
```

In the recap section added in Task 3, change the head to hold a way back:

```html
        <div class="row head">
          <h2 id="recap-title">Today</h2>
          <span class="row">
            <button id="back-today" hidden>Back to today</button>
            <button class="refresh" aria-label="Refresh the recap">Refresh</button>
          </span>
        </div>
```

- [ ] **Step 2: The panel code**

In `src/panel/main.ts`, add above `async function loadDay(`:

```ts
// R-1: about four months of days, a column per week from Monday, shaded by hours of focus
const HEAT_WEEKS = 18;
const heatLevel = (minutes: number) => (minutes <= 0 ? 0 : minutes < 60 ? 1 : minutes < 120 ? 2 : minutes < 240 ? 3 : 4);
// R-2: picking a day asks for the streak again, whose answer redraws the whole view (loadDay)
function pickDay(date: string | null) {
  recapDay = date;
  return run(() => invoke("streak"));
}
$("back-today").onclick = () => pickDay(null);

function showHeatmap(day: Day) {
  // noon avoids daylight-saving edges when stepping by day
  const today = new Date(`${day.today}T12:00:00`);
  const d = new Date(today);
  d.setDate(d.getDate() - ((d.getDay() + 6) % 7) - 7 * (HEAT_WEEKS - 1)); // the Monday 17 weeks back
  const cells: HTMLElement[] = [];
  for (; d <= today; d.setDate(d.getDate() + 1)) {
    const date = d.toLocaleDateString("en-CA"); // "YYYY-MM-DD"
    const minutes = day.minutes[date] ?? 0;
    const label = `${d.toLocaleDateString([], { weekday: "short", day: "numeric", month: "short" })}: ${
      minutes ? duration(minutes) : "no focus"
    }`;
    const cell = el("button", "", `l${heatLevel(minutes)}`);
    cell.title = label;
    cell.setAttribute("aria-label", label);
    cell.setAttribute("aria-pressed", String(date === (recapDay ?? day.today)));
    cell.onclick = () => pickDay(date === day.today ? null : date);
    cells.push(cell);
  }
  $("heat").replaceChildren(...cells);
}
```

In `loadDay`, replace the statement that sets `$("wrap-total").textContent` with:

```ts
  const todayLine = day.sessions.length
    ? `You focused for ${duration(day.minutes[day.today] ?? 0)} across ${plural(day.sessions.length, "block")} today.`
    : "Nothing logged today yet. A focus block of any length counts.";
  // an earlier day picked in the heatmap: its focus, and its name over the recap
  const when =
    recapDay && new Date(`${recapDay}T12:00:00`).toLocaleDateString([], { weekday: "long", day: "numeric", month: "long" });
  const then = recapDay ? (day.minutes[recapDay] ?? 0) : 0;
  $("wrap-total").textContent = !recapDay
    ? todayLine
    : then
      ? `You focused for ${duration(then)} on ${when}.`
      : `No focus logged on ${when}.`;
  $("recap-title").textContent = when || "Today";
  $("back-today").hidden = !recapDay;
```

At the end of `loadDay`, replace everything from the comment `// 7 days ending today; noon avoids daylight-saving edges when stepping by day` to the end of the `$("week").replaceChildren(...)` statement with:

```ts
  showHeatmap(day);
```

- [ ] **Step 3: The styles**

In `src/panel/panel.css`, add to the `:root` block:

```css
  /* the heatmap's shades, light to dark; checked as an ordinal ramp against --surface */
  --heat-1: #86b6ef;
  --heat-2: #5598e7;
  --heat-3: #256abf;
  --heat-4: #104281;
```

and to the `:root` block inside `@media (prefers-color-scheme: dark)` (more focus is brighter on the dark surface):

```css
    --heat-1: #1c5cab;
    --heat-2: #3987e5;
    --heat-3: #6da7ec;
    --heat-4: #b7d3f6;
```

Delete the five `#week` rules (`#week`, `#week li`, `#week .bar`, `#week .counted .bar`, `#week .today`), and add in their place:

```css
/* ---- heatmap: a column per week, a square per day ---- */

#heat {
  display: grid;
  grid-auto-flow: column;
  grid-template-rows: repeat(7, 16px);
  grid-auto-columns: 16px;
  gap: 3px;
}

#heat button {
  padding: 0;
  border: 0;
  border-radius: 4px;
}

#heat [aria-pressed="true"] {
  outline: 2px solid var(--ink);
  outline-offset: 1px;
}

.heat-key span {
  display: inline-block;
  width: 10px;
  height: 10px;
  margin: 0 1px;
  border-radius: 3px;
}

.l0 {
  background: var(--line);
}

.l1 {
  background: var(--heat-1);
}

.l2 {
  background: var(--heat-2);
}

.l3 {
  background: var(--heat-3);
}

.l4 {
  background: var(--heat-4);
}
```

- [ ] **Step 4: Check and look**

Run: `npm run build`
Expected: the type check and the build pass. If `grep -n '"week"\|#week' src/panel/main.ts src/panel/panel.css panel.html` prints anything, a piece of the old strip is left; remove it.

Run: `npm run tauri dev`, open Wrap up, and check:
- 18 columns of squares, the last column ending on today, today outlined.
- Days with focus blocks are shaded; hovering one shows its date and time.
- Clicking an earlier day: the headline names that day, the recap title is its date, the bar and "Apps and sites" show that day, "Back to today" appears, and the clicked square is outlined.
- Clicking a day before 2026-10-07: "Nothing noted." in both lists, no bar, no error line at the bottom.
- "Back to today", and a click on today's square, both return to today.
- Leave an earlier day shown for two minutes: it stays shown (the once-a-minute reload does not jump back).
- Switch macOS between light and dark: the shades and the bar read well in both.

- [ ] **Step 5: Commit**

```bash
git add panel.html src/panel
git commit -m "focus heatmap in Wrap up; a click shows that day's recap"
```

---

### Task 5: Folding "What the pet has learned" (R-6), and the docs

**Files:**
- Modify: `panel.html` (Settings), `src/panel/main.ts` (`showLearned`), `src/panel/panel.css`
- Modify: `docs/CHANGELOG.md`, `docs/KNOWN_ISSUES.md`, `docs/ARCHITECTURE.md`, `docs/ROADMAP.md`

**Interfaces:**
- Consumes: `plural(n, word)` and `showLearned(rules)` already in `main.ts`.
- Produces: nothing.

- [ ] **Step 1: The markup**

In `panel.html`, replace the "What the pet has learned" `<section>` with:

```html
      <section>
        <details>
          <summary><h2>What the pet has learned</h2> <span id="learned-count" class="muted"></span></summary>
          <ul id="learned" class="rows"></ul>
          <p class="muted">Your answers about apps and sites. Forget one and the pet may ask again.</p>
        </details>
      </section>
```

- [ ] **Step 2: The count**

In `src/panel/main.ts`, in `showLearned`, add as the last line of the function:

```ts
  $("learned-count").textContent = plural(rules.length, "answer");
```

- [ ] **Step 3: The styles**

In `src/panel/panel.css`, add after the `.rows .empty` rule:

```css
/* a section that folds shut (Settings, "What the pet has learned") */
summary {
  cursor: pointer;
}

summary h2 {
  display: inline;
}
```

- [ ] **Step 4: Check and look**

Run: `npm run build`
Expected: passes.

Run: `npm run tauri dev`, open Settings, and check: the section is shut and reads "What the pet has learned 3 answers" (with the real count); a click opens it; "Forget" on a row removes it, lowers the count and leaves the section open; every site in it is a name, not an address.

- [ ] **Step 5: The docs**

`docs/CHANGELOG.md`: change the first heading to `## Unreleased · Phases 3 and 4`, and add at the top of its **Changed** list:

```markdown
- Wrap up is a recap of any day. A focus heatmap of the last 18 weeks replaces "Last 7 days"; a click on a day shows that day. "Time by mode" is one bar with its hours, and one list, "Apps and sites", gives a single total per app and site in place of "Sites today", "Apps today" and "Stretches". The browser's row counts only the time not under one of its sites.
- A site is shown by name everywhere, never by address: a built-in list in `modes.rs` (`site_name`), else the first part of the address. This covers the recap, the pet's questions, its drift lines and "What the pet has learned".
- "What the pet has learned" in Settings folds shut and shows how many answers it holds.
- Commands: `modes_today` is now `modes_day` and takes a day; `audio_time` takes a day; `site_time` and `app_time` are gone. `label_day` also returns `usage`.
```

`docs/KNOWN_ISSUES.md`: replace item 66 (the whole numbered item, both paragraphs and its *Fix* line) with:

```markdown
66. **Fixed 2026-10-11: only today could be shown.** A click on a day in the Wrap up heatmap shows that day's recap. What is left: labels are worked out when shown, with the rules as they are now, so an answer given today also relabels earlier days; and a declaration made just before midnight and ended just after it is counted only on the day it was made.
```

and add at the end of the "Phase 3: finished on 2026-10-07" list:

```markdown
79. **Site names are a short list and a guess (2026-10-11).** A site not on the list in `modes.rs` is named after the first part of its address, so `blog.example.com` is "Blog", and two different sites that guess the same name are added together in "Apps and sites". The Chrome extension's popup still lists sites by address. *Fix when it annoys:* add the site to `SITE_NAMES`.
80. **The recap leaves out anything under a minute.** An app or site in front for less than 60 seconds in a day is not listed, so the list can add up to a little less than the bar.
```

`docs/ARCHITECTURE.md`: in the sentence that lists the commands (it starts "**UI to Rust:**"), change `modes_today` to `modes_day`, remove `site_time` and `app_time`, add `keeper` and `audio_time`, and make its last sentence read: "`accessibility` is compiled only on macOS." In the numbered step that starts "**When asked** (`modes_today`)", change it to:

```markdown
4. **When asked** (`modes_day`, for today or any earlier local day), Rust sends that day's rows from both tables, the focus blocks, the declarations and the learned rules to `label_day` in `backend/modes.py`. It returns the labeled stretches, the seconds per mode, the time per app and site, and the apps and sites nothing recognizes. Rust then puts a name in place of each site's address (`name_sites` in `modes.rs`). Nothing of this is stored.
```

`docs/ROADMAP.md`: change the heading `## Phase 4 · Recap (next)` to `## Phase 4 · Recap (built, not yet reinstalled)`, tick all ten tasks in its list (the last two are Step 4 of Tasks 3 to 5 and this step), and add under the list:

```markdown
- [ ] Rebuild and reinstall the everyday app when asked, backing up `pet.db` first (`KNOWN_ISSUES.md`, item 34). Until then the installed app keeps the old Wrap up view; it is not broken by this phase, since `backend/` only gained keys.
```

- [ ] **Step 6: Run every check once more**

Run all three check commands from "Global Constraints".
Expected: all pass.

- [ ] **Step 7: Commit**

```bash
git add panel.html src/panel docs
git commit -m "learned section folds shut; docs for the recap"
```
