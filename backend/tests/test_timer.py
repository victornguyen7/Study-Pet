"""Run from the project root:  backend/.venv/bin/python -m unittest backend.tests.test_timer"""

import unittest

from backend.timer import Timer

MIN = 60


def at(timer, now):
    timer.tick(now)
    return timer.state(now)


class TimerTest(unittest.TestCase):
    def test_focus_break_callback_cycle(self):
        t = Timer()
        t.start_focus(0, 25, 5, "HW3")
        self.assertEqual(at(t, 10 * MIN), at(t, 10 * MIN) | {"phase": "focus", "remaining": 15 * MIN})

        self.assertEqual(at(t, 25 * MIN)["phase"], "break")
        self.assertEqual(t.state(25 * MIN)["remaining"], 5 * MIN)
        self.assertEqual(t.drain_finished(), [
            {"started_at": 0, "ended_at": 25 * MIN, "planned_min": 25, "break_min": 5, "label": "HW3"}
        ])
        self.assertEqual(t.drain_finished(), [])

        # S-4: called back when the break ends, once more 2 minutes later, then never again
        self.assertEqual(at(t, 30 * MIN)["calls"], 1)
        self.assertEqual(at(t, 32 * MIN - 1)["calls"], 1)
        self.assertEqual(at(t, 32 * MIN)["calls"], 2)
        self.assertEqual(at(t, 90 * MIN)["calls"], 2)

        t.resume(40 * MIN)
        self.assertEqual(at(t, 40 * MIN), at(t, 40 * MIN) | {"phase": "focus", "remaining": 25 * MIN, "calls": 0})

    def test_stopping_early_still_counts(self):
        t = Timer()
        t.start_focus(0, 50, 10)
        t.stop(7 * MIN)
        self.assertEqual(t.state(7 * MIN)["phase"], "idle")
        self.assertEqual(t.drain_finished()[0]["ended_at"], 7 * MIN)

    def test_three_skipped_breaks_suggest_a_real_one(self):
        t = Timer()
        t.start_focus(0, 10, 5)
        now = 0
        for skips in range(3):
            now += 10 * MIN
            self.assertFalse(at(t, now)["suggest_real_break"], skips)
            t.skip_break(now)
        now += 10 * MIN
        self.assertTrue(at(t, now)["suggest_real_break"])
        # taking the break resets the count
        now += 5 * MIN
        self.assertEqual(at(t, now)["phase"], "callback")
        t.resume(now)
        now += 10 * MIN
        self.assertFalse(at(t, now)["suggest_real_break"])

    def test_one_tick_after_a_long_sleep_catches_up(self):
        t = Timer()
        t.start_focus(0, 25, 5)
        self.assertEqual(at(t, 3 * 60 * MIN) | {"label": None}, {
            "phase": "callback", "remaining": 0, "focus_min": 25, "break_min": 5,
            "label": None, "calls": 2, "suggest_real_break": False,
        })
        self.assertEqual(t.drain_finished()[0]["ended_at"], 25 * MIN)

    def test_rejects_non_positive_lengths(self):
        with self.assertRaises(ValueError):
            Timer().start_focus(0, 0, 5)


if __name__ == "__main__":
    unittest.main()
