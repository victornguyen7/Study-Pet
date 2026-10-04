"""Run from the project root:  backend/.venv/bin/python -m unittest backend.tests.test_streaks"""

import unittest
from datetime import date, timedelta

from backend.streaks import streak

TODAY = "2026-10-20"


def history(pattern, minutes=30):
    """pattern: one char per day ending today, "x" = studied, "." = missed."""
    start = date.fromisoformat(TODAY) - timedelta(days=len(pattern) - 1)
    return {
        (start + timedelta(days=i)).isoformat(): minutes
        for i, c in enumerate(pattern)
        if c == "x"
    }


class StreakTest(unittest.TestCase):
    def days(self, pattern):
        return streak(history(pattern), TODAY, 25)["days"]

    def test_no_history(self):
        self.assertEqual(streak({}, TODAY, 25), {
            "days": 0, "today_minutes": 0, "today_counted": False, "threshold": 25,
            "freezes_left": 2, "milestone": 0,
        })

    def test_consecutive_days(self):
        self.assertEqual(self.days("xxx"), 3)

    def test_today_not_done_yet_keeps_the_streak(self):
        self.assertEqual(self.days("xxx."), 3)

    def test_threshold(self):
        self.assertEqual(streak({TODAY: 24.9}, TODAY, 25)["days"], 0)
        self.assertEqual(streak({TODAY: 25}, TODAY, 25)["days"], 1)
        self.assertEqual(streak({TODAY: 10}, TODAY, 10)["days"], 1)

    def test_freeze_covers_one_missed_day(self):
        res = streak(history("xx.xx"), TODAY, 25)
        self.assertEqual((res["days"], res["freezes_left"]), (4, 1))

    def test_third_miss_in_a_month_breaks_the_streak(self):
        self.assertEqual(self.days("xx.x.x.xx"), 2)

    def test_two_misses_in_a_row_use_both_freezes(self):
        res = streak(history("xx..x"), TODAY, 25)
        self.assertEqual((res["days"], res["freezes_left"]), (3, 0))

    def test_freezes_refill_each_month(self):
        # two misses in September, two in October
        minutes = history("x" * 40)
        for day in ("2026-09-20", "2026-09-25", "2026-10-05", "2026-10-10"):
            del minutes[day]
        self.assertEqual(streak(minutes, TODAY, 25)["days"], 36)

    def test_milestones(self):
        for n, milestone in ((6, 0), (7, 7), (29, 7), (30, 30), (100, 100), (250, 100)):
            self.assertEqual(streak(history("x" * n), TODAY, 25)["milestone"], milestone, n)


if __name__ == "__main__":
    unittest.main()
