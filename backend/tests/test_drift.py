"""Run from the project root:  backend/.venv/bin/python -m unittest backend.tests.test_drift"""

import unittest

from backend.drift import Drift, Keeper, feed, is_drift, is_music


class DriftTest(unittest.TestCase):
    def test_what_counts_as_drift(self):
        self.assertTrue(is_drift({"domain": "www.reddit.com", "url": "https://www.reddit.com/r/all"}))
        self.assertTrue(is_drift({"app": "Steam", "bundle_id": "com.valvesoftware.steam"}))
        self.assertFalse(is_drift({"domain": "canvas.school.edu"}))
        # unknown and Private things are left alone
        self.assertFalse(is_drift({"app": "Preview", "bundle_id": "com.apple.Preview"}))
        self.assertFalse(is_drift({"app": "Private", "bundle_id": None}))
        # the user's own answer decides, both ways
        self.assertFalse(is_drift({"domain": "reddit.com"}, {"reddit.com": "study"}))
        self.assertTrue(is_drift({"app": "Preview", "bundle_id": "com.apple.Preview"}, {"com.apple.preview": "relax"}))

    def test_youtube_feeds_but_not_videos(self):
        for url in ["https://www.youtube.com/", "https://www.youtube.com/shorts/abc", "https://m.youtube.com/feed/subscriptions"]:
            self.assertTrue(feed(url), url)
        for url in ["https://www.youtube.com/watch?v=abc", "https://notyoutube.com/", "", None]:
            self.assertFalse(feed(url), url)
        # Shorts are drift even when the user said YouTube is for study
        self.assertTrue(is_drift({"domain": "www.youtube.com", "url": "https://www.youtube.com/shorts/abc"}, {"www.youtube.com": "study"}))

    def test_ladder(self):
        drift = Drift()
        steps = [drift.update(t, True, "reddit.com")["step"] for t in range(1000, 1320, 5)]
        # 30 s of grace, then a glance, a bubble at 2 min, sitting in the way at 5
        self.assertEqual((steps[5], steps[6], steps[23], steps[24], steps[59], steps[60]), (0, 1, 1, 2, 2, 3))
        self.assertEqual(drift.update(1320, True, "netflix.com"), {"step": 3, "seconds": 320, "what": "netflix.com", "returned": False})
        # step 4 at 10 min, for the opt-in block
        later = [drift.update(t, True, "netflix.com")["step"] for t in range(1325, 1605, 5)]
        self.assertEqual(later[-2:], [3, 4])
        # coming back is noticed once, and the next drift starts from nothing
        self.assertEqual(drift.update(1605, False), {"step": 0, "seconds": 0, "what": None, "returned": True})
        self.assertFalse(drift.update(1610, False)["returned"])
        self.assertEqual(drift.update(1615, True, "reddit.com")["step"], 0)

    def test_a_glance_is_not_celebrated_and_a_gap_resets(self):
        drift = Drift()
        for t in range(0, 45, 5):
            state = drift.update(t, True, "x")
        self.assertEqual(state["step"], 1)
        self.assertFalse(drift.update(45, False)["returned"])
        # 3 min of drift, then a break: nothing is left of it afterwards
        for t in range(100, 285, 5):
            drift.update(t, True, "x")
        self.assertEqual(drift.update(900, True, "x"), {"step": 0, "seconds": 0, "what": "x", "returned": False})
        for t in range(905, 1085, 5):
            drift.update(t, True, "x")
        self.assertEqual(drift.update(2000, False)["returned"], False)

    def test_music_in_front_has_a_minute_of_grace(self):
        spotify = {"app": "Spotify", "bundle_id": "com.spotify.client"}
        self.assertTrue(is_music(spotify) and is_music({"domain": "music.youtube.com"}))
        self.assertFalse(is_drift(spotify) or is_music({"domain": "www.youtube.com"}))
        self.assertFalse(is_music(spotify, {"com.spotify.client": "relax"}))  # the user's answer makes it plain drift
        drift = Drift()
        # skipping a song: 55 s in front, then back, is nothing at all
        for t in range(0, 60, 5):
            self.assertEqual(drift.update(t, False, "Spotify", music=True)["step"], 0)
        self.assertEqual(drift.update(60, False), {"step": 0, "seconds": 0, "what": None, "returned": False})
        # left in front: drift starts at 60 s, and the ladder's own steps count from there
        steps = {t: drift.update(100 + t, False, "Spotify", music=True) for t in range(0, 185, 5)}
        self.assertEqual([steps[t]["step"] for t in (55, 60, 85, 90, 175, 180)], [0, 0, 0, 1, 1, 2])
        self.assertEqual((steps[55]["what"], steps[60]["what"], steps[90]["seconds"]), (None, "Spotify", 30))


def samples(keeper, start, secs, mode, drifting=False, **kw):
    """One sample every 5 s from `start` for `secs` seconds. -> {time: state}."""
    return {t: keeper.update(t, mode, drifting, "x", **kw) for t in range(start, start + secs + 1, 5)}


class KeeperTest(unittest.TestCase):
    def on_keeper(self):
        """A keeper that turned on at t=300, after samples of Study from 0."""
        keeper = Keeper()
        samples(keeper, 0, 300, "study")
        return keeper

    def test_on_after_five_minutes_of_study_or_work(self):
        for mode in ("study", "work"):
            states = samples(Keeper(), 0, 300, mode)
            self.assertEqual((states[295]["on"], states[300]["on"]), (False, True), mode)

    def test_what_does_not_earn(self):
        # unknown, Private and Relax time never turn it on, and Relax is not drift while it is off
        for mode, drifting in ((None, False), ("private", False), ("relax", True)):
            last = samples(Keeper(), 0, 900, mode, drifting)[900]
            self.assertEqual((last["on"], last["step"]), (False, 0), mode)
        # nor does a music app in front
        self.assertFalse(samples(Keeper(), 0, 900, None, music=True)[900]["on"])
        # an unknown app in the middle neither adds nor takes away
        keeper = Keeper()
        samples(keeper, 0, 200, "study")
        self.assertFalse(samples(keeper, 205, 195, None)[400]["on"])
        states = samples(keeper, 405, 95, "study")
        self.assertEqual((states[495]["on"], states[500]["on"]), (False, True))

    def test_the_ladder_runs_only_while_on(self):
        keeper = self.on_keeper()
        states = samples(keeper, 305, 120, "relax", True)
        self.assertEqual((states[425]["step"], states[425]["on"], states[425]["what"]), (2, True, "x"))
        back = keeper.update(430, "study", False)
        self.assertEqual((back["returned"], back["on"], back["step"]), (True, True, 0))

    def test_fifteen_minutes_of_relax_was_a_break(self):
        keeper = self.on_keeper()
        states = samples(keeper, 305, 900, "relax", True)
        self.assertEqual((states[1200]["on"], states[1200]["step"]), (True, 4))
        # the keeper lets go without a "welcome back", and the five minutes are earned again
        self.assertEqual(states[1205], {"step": 0, "seconds": 0, "what": None, "returned": False, "on": False, "break": False})
        states = samples(keeper, 1210, 295, "study")
        self.assertEqual((states[1500]["on"], states[1505]["on"]), (False, True))

    def test_five_minutes_away_was_a_break(self):
        # a shorter gap adds nothing and changes nothing
        self.assertTrue(self.on_keeper().update(595, "study", False)["on"])
        keeper = self.on_keeper()
        self.assertFalse(keeper.update(600, "study", False)["on"])
        # and a keeper that was nudging does not welcome anyone back after it
        keeper = self.on_keeper()
        samples(keeper, 305, 300, "relax", True)
        self.assertEqual(keeper.update(1000, "study", False)["returned"], False)

    def test_relax_declared_keeps_it_off(self):
        keeper = self.on_keeper()
        state = keeper.update(305, "relax", True, relax_declared=True)
        self.assertEqual((state["on"], state["step"]), (False, 0))
        self.assertFalse(samples(Keeper(), 0, 900, "relax", relax_declared=True)[900]["on"])

    def test_a_break_is_suggested_once_per_run(self):
        keeper = Keeper()
        states = samples(keeper, 0, 3600, "study")
        self.assertEqual([t for t, s in states.items() if s["break"]], [3000])  # 50 minutes of work
        # away for a while, then a new run: suggested again, 50 minutes in
        states = samples(keeper, 4000, 3100, "study")
        self.assertEqual([t for t, s in states.items() if s["break"]], [7000])

    def test_break_length_zero_never_suggests(self):
        for minutes in (0, -5):
            states = samples(Keeper(), 0, 7200, "study", break_after_min=minutes)
            self.assertFalse(any(s["break"] for s in states.values()), minutes)
        # shorter than the time it takes to turn on: said as it turns on
        states = samples(Keeper(), 0, 600, "study", break_after_min=1)
        self.assertEqual([t for t, s in states.items() if s["break"]], [300])
