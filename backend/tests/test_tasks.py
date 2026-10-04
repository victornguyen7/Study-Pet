"""Run from the project root:  backend/.venv/bin/python -m unittest backend.tests.test_tasks"""

import unittest

from backend.tasks import parse_quick_add

TODAY = "2026-10-20"  # a Tuesday


def task(title, course=None, due_date=None, estimate_min=None):
    return {"title": title, "course": course, "due_date": due_date, "estimate_min": estimate_min}


class QuickAddTest(unittest.TestCase):
    def parse(self, text):
        return parse_quick_add(text, TODAY)

    def test_spec_example(self):
        self.assertEqual(self.parse("STAT 310 HW3 Fri 2h"), task("HW3", "STAT 310", "2026-10-23", 120))

    def test_title_only(self):
        self.assertEqual(self.parse("buy index cards"), task("buy index cards"))

    def test_estimate_forms(self):
        for text, minutes in (("essay 30m", 30), ("essay 1.5h", 90), ("essay 1h30m", 90), ("essay 2H", 120)):
            self.assertEqual(self.parse(text)["estimate_min"], minutes, text)

    def test_due_forms(self):
        for word, due in (
            ("today", "2026-10-20"),
            ("tomorrow", "2026-10-21"),
            ("tue", "2026-10-20"),  # today's weekday means today, not next week
            ("Monday", "2026-10-26"),
            ("thurs", "2026-10-22"),
            ("10/31", "2026-10-31"),
            ("1/5", "2027-01-05"),  # already past this year
            ("2026-11-02", "2026-11-02"),
        ):
            self.assertEqual(self.parse(f"essay {word}")["due_date"], due, word)

    def test_due_and_estimate_in_either_order(self):
        self.assertEqual(self.parse("essay 2h fri"), task("essay", None, "2026-10-23", 120))

    def test_words_leading_into_the_due_date_are_dropped(self):
        self.assertEqual(self.parse("Stat HW 5 next Friday"), task("Stat HW 5", None, "2026-10-23"))
        self.assertEqual(self.parse("essay due on next fri 2h"), task("essay", None, "2026-10-23", 120))
        self.assertEqual(self.parse("essay 2h by tomorrow"), task("essay", None, "2026-10-21", 120))
        self.assertEqual(self.parse("what comes next")["title"], "what comes next")  # no due date after it
        with self.assertRaises(ValueError):
            self.parse("due on fri")

    def test_only_the_end_of_the_line_is_read(self):
        self.assertEqual(
            self.parse("read Friday Night Lights sat"), task("read Friday Night Lights", None, "2026-10-24")
        )
        self.assertEqual(self.parse("essay 13/40")["title"], "essay 13/40")  # not a date
        self.assertEqual(self.parse("sunscreen")["title"], "sunscreen")  # not "sun"

    def test_course_forms(self):
        self.assertEqual(self.parse("cs61a lab 4")["course"], "CS 61A")
        self.assertEqual(self.parse("stat 310 hw3")["course"], "STAT 310")
        self.assertEqual(self.parse("HW3 fri"), task("HW3", None, "2026-10-23"))
        self.assertEqual(self.parse("STAT 310"), task("STAT 310"))  # nothing left to be the title
        self.assertEqual(self.parse("abc123xyz notes")["course"], None)

    def test_needs_a_title(self):
        for text in ("", "   ", "fri 2h"):
            with self.assertRaises(ValueError):
                self.parse(text)


if __name__ == "__main__":
    unittest.main()
