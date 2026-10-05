"""Turns an ICS calendar feed into rows for the Rust core to store.

Pure text in, dicts out: no network and no clock here. The task fetcher downloads the feed and calls this.
"""

import re
from datetime import datetime, timezone

import icalendar
import recurring_ical_events


def _when(value):
    """A date or datetime from the feed -> ("2026-09-11T20:00:00Z", False) or ("2026-09-08", True)."""
    if not isinstance(value, datetime):
        return value.isoformat(), True
    # a time with no zone means local wall time; astimezone() reads it as this laptop's zone
    return value.astimezone(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"), False


def _is_assignment(event, source):
    return source == "canvas" and str(event.get("UID", "")).startswith("event-assignment-")


def _title(event, source):
    """Returns (title, course). Canvas ends a summary with the course: "Homework 1 [STATISTC 315 (174949) FA26]"."""
    title = str(event.get("SUMMARY", "")).strip() or "(no title)"
    named = re.fullmatch(r"(.*)\[([^\]]*)\]", title, re.S) if source == "canvas" else None
    if not named:
        return title, None
    # drop the section number and term
    return named[1].strip(), re.sub(r"\s*\(\d+\).*", "", named[2]).strip() or None


def parse_feed(ics, source, start, end):
    """Returns the feed's tasks and events as dicts, in the shape FeedItem in src-tauri/src/lib.rs reads.

    A Canvas assignment becomes a "task", whatever its date. Everything else becomes an "event", and only
    the ones between `start` and `end` (datetimes with a zone) are kept, with repeating events expanded.
    Times are UTC "YYYY-MM-DDTHH:MM:SSZ", or "YYYY-MM-DD" for all-day items. Raises ValueError if `ics`
    is not a calendar.
    """
    calendar = icalendar.Calendar.from_ical(ics)
    items = []
    for event in calendar.walk("VEVENT"):
        if not _is_assignment(event, source) or "DTSTART" not in event:
            continue
        title, course = _title(event, source)
        due, all_day = _when(event.decoded("DTSTART"))
        items.append(
            {
                "kind": "task",
                "source": source,
                "uid": str(event["UID"]),
                "title": title,
                "course": course,
                "due_at": None if all_day else due,
                "due_date": due if all_day else None,
            }
        )
    for event in recurring_ical_events.of(calendar).between(start, end):
        if _is_assignment(event, source) or "UID" not in event:
            continue
        begin, all_day = _when(event.decoded("DTSTART"))
        finish = _when(event.decoded("DTEND"))[0] if "DTEND" in event else begin
        items.append(
            {
                "kind": "event",
                "source": source,
                # every repeat of an event shares one UID, so the start makes it unique
                "uid": f"{event['UID']}@{begin}",
                "title": _title(event, source)[0],
                "start_at": begin,
                "end_at": finish,
                "all_day": all_day,
            }
        )
    return items
