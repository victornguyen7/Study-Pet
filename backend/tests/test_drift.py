"""Run from the project root:  backend/.venv/bin/python -m unittest backend.tests.test_drift"""

import unittest

from backend.drift import Drift, feed, is_drift, is_music


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
