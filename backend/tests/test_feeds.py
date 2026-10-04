"""Run from the project root:  backend/.venv/bin/python -m unittest backend.tests.test_feeds"""

import unittest
from datetime import datetime, timezone
from pathlib import Path

from backend.feeds import parse_feed

HERE = Path(__file__).parent
# The shape of a real Canvas feed, with made-up ids: folded lines, escaped commas, all-day and timed items.
CANVAS = (HERE / "canvas.ics").read_text(encoding="utf-8")
# The shape of a Google Calendar feed: a time zone, a repeating event with one date left out.
GOOGLE = (HERE / "google.ics").read_text(encoding="utf-8")

# Monday to Monday
START = datetime(2026, 10, 5, tzinfo=timezone.utc)
END = datetime(2026, 10, 12, tzinfo=timezone.utc)


def task(uid, title, course, due_at=None, due_date=None):
    return {"kind": "task", "source": "canvas", "uid": uid, "title": title, "course": course, "due_at": due_at, "due_date": due_date}


def event(source, uid, title, start_at, end_at, all_day=False):
    return {"kind": "event", "source": source, "uid": uid, "title": title, "start_at": start_at, "end_at": end_at, "all_day": all_day}


class CanvasFeedTest(unittest.TestCase):
    def setUp(self):
        # the fixture's items are all in September, before the window
        self.items = parse_feed(CANVAS, "canvas", START, END)

    def test_assignments_are_kept_whatever_their_date(self):
        self.assertEqual([i["kind"] for i in self.items], ["task", "task", "task"])

    def test_all_day_assignment_gets_a_date(self):
        self.assertEqual(
            self.items[0],
            task("event-assignment-100001", "Exit ticket 9/8", "STATISTC 310", due_date="2026-09-08"),
        )

    def test_timed_assignment_gets_a_utc_time(self):
        self.assertEqual(
            self.items[1],
            task("event-assignment-100002", "Homework 1", "STATISTC 315", due_at="2026-09-11T20:00:00Z"),
        )

    def test_folded_and_escaped_summary(self):
        self.assertEqual(
            self.items[2]["title"],
            "🤖 AI Track, Project 2, Assembly Simulator - Part 1 (PROJECT ASSIGNMENT)",
        )
        self.assertEqual(self.items[2]["course"], "COMPSCI 230")

    def test_other_events_are_kept_only_inside_the_window(self):
        september = (datetime(2026, 9, 14, tzinfo=timezone.utc), datetime(2026, 9, 21, tzinfo=timezone.utc))
        self.assertEqual(
            parse_feed(CANVAS, "canvas", *september)[3:],
            [
                event(
                    "canvas",
                    "event-calendar-event-200001@2026-09-15T14:00:00Z",
                    "Office hours",
                    "2026-09-15T14:00:00Z",
                    "2026-09-15T15:15:00Z",
                )
            ],
        )

    def test_crlf_line_endings(self):
        self.assertEqual(parse_feed(CANVAS.replace("\n", "\r\n"), "canvas", START, END), self.items)

    def test_summary_without_a_course(self):
        feed = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:event-assignment-1\nDTSTART;VALUE=DATE:20260908\nSUMMARY:Read chapter 2\nEND:VEVENT\nEND:VCALENDAR"
        self.assertEqual(
            parse_feed(feed, "canvas", START, END),
            [task("event-assignment-1", "Read chapter 2", None, due_date="2026-09-08")],
        )

    def test_not_a_calendar_is_an_error_not_an_empty_feed(self):
        # an empty result would flag every upcoming task as removed
        for text in ("", "<html>Please log in</html>"):
            with self.assertRaises(ValueError):
                parse_feed(text, "canvas", START, END)


class GoogleFeedTest(unittest.TestCase):
    def setUp(self):
        self.items = sorted(parse_feed(GOOGLE, "google", START, END), key=lambda i: i["start_at"])

    def test_nothing_from_google_is_a_task(self):
        self.assertEqual({i["kind"] for i in self.items}, {"event"})

    def test_repeats_are_expanded_in_utc_and_left_out_dates_skipped(self):
        # Tuesdays and Thursdays at 10:00 in New York (UTC-4 in October); Thursday the 8th is left out
        self.assertEqual(
            [i for i in self.items if i["title"] == "STAT 310 lecture"],
            [
                event(
                    "google",
                    "lecture123@google.com@2026-10-06T14:00:00Z",
                    "STAT 310 lecture",
                    "2026-10-06T14:00:00Z",
                    "2026-10-06T15:15:00Z",
                )
            ],
        )

    def test_single_and_all_day_events(self):
        self.assertEqual(
            [i for i in self.items if i["title"] != "STAT 310 lecture"],
            [
                event("google", "dentist456@google.com@2026-10-07T18:00:00Z", "Dentist", "2026-10-07T18:00:00Z", "2026-10-07T19:00:00Z"),
                event("google", "holiday789@google.com@2026-10-09", "No classes", "2026-10-09", "2026-10-10", all_day=True),
            ],
        )


if __name__ == "__main__":
    unittest.main()
