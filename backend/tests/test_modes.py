"""Run from the project root:  backend/.venv/bin/python -m unittest backend.tests.test_modes"""

import unittest

from backend.modes import classify, label_day, timeline

MIN = 60


def app(start, end, name, bundle_id=None):
    return {"start": start * MIN, "end": end * MIN, "app": name, "bundle_id": bundle_id}


def page(start, end, domain):
    return {"start": start * MIN, "end": end * MIN, "domain": domain}


def labels(day):
    return [(s["start"] // MIN, s["end"] // MIN, s["mode"], s["source"]) for s in day["spans"]]


class ModesTest(unittest.TestCase):
    def test_signals_strongest_first(self):
        canvas, steam, unknown = page(0, 1, "canvas.school.edu"), app(0, 1, "Steam", "com.valvesoftware.steam"), app(0, 1, "Preview", "com.apple.Preview")
        self.assertEqual(classify(canvas), ("study", "rule"))
        self.assertEqual(classify(steam), ("relax", "rule"))
        self.assertEqual(classify(page(0, 1, "www.LinkedIn.com")), ("work", "rule"))
        self.assertEqual(classify(page(0, 1, "notreddit.com")), (None, None))  # a whole name, not its ending
        self.assertEqual(classify(unknown), (None, None))
        # context only fills in what nothing else knows
        self.assertEqual(classify(unknown, in_session=True), ("study", "context"))
        self.assertEqual(classify(steam, in_session=True), ("relax", "rule"))
        # a learned rule beats the category, a declaration beats both
        rules = {"com.valvesoftware.steam": "work", "canvas.school.edu": "relax"}
        self.assertEqual(classify(steam, rules=rules), ("work", "learned"))
        self.assertEqual(classify(canvas, rules=rules), ("relax", "learned"))
        self.assertEqual(classify(steam, "study", rules, True), ("study", "declared"))
        # the gate's Private stays Private whatever is declared
        self.assertEqual(classify(app(0, 1, "Private"), "study", rules, True), ("private", "rule"))

    def test_a_page_cuts_the_browser_span_around_it(self):
        spans = timeline([app(0, 10, "Chrome", "com.google.Chrome"), app(10, 12, "Preview")], [page(2, 5, "a.edu"), page(5, 5, "b.edu")])
        self.assertEqual(
            [(s["start"] // MIN, s["end"] // MIN, s.get("domain") or s["app"]) for s in spans],
            [(0, 2, "Chrome"), (2, 5, "a.edu"), (5, 10, "Chrome"), (10, 12, "Preview")],
        )

    def test_short_detours_keep_the_label(self):
        day = label_day(
            [app(0, 60, "Chrome", "com.google.Chrome")],
            # 10 min of Canvas, 2 min of Reddit, back for 10, then Netflix for 5, then 1 min with no page
            [page(0, 10, "canvas.school.edu"), page(10, 12, "reddit.com"), page(12, 22, "canvas.school.edu"), page(22, 27, "netflix.com")],
        )
        self.assertEqual(labels(day)[:2], [(0, 22, "study", "rule"), (22, 60, "relax", "rule")])
        self.assertEqual(day["totals"], {"study": 22 * MIN, "relax": 38 * MIN})

    def test_two_short_stretches_of_one_mode_add_up(self):
        day = label_day([], [page(0, 10, "a.edu"), page(10, 12, "reddit.com"), page(12, 14, "netflix.com")])
        self.assertEqual(labels(day), [(0, 10, "study", "rule"), (10, 14, "relax", "rule")])

    def test_unknown_time_and_gaps(self):
        day = label_day(
            # nothing known for 20 min, then study; after a gap the label does not carry over
            [app(0, 20, "Preview"), app(40, 41, "Preview")],
            [page(20, 30, "a.edu")],
        )
        self.assertEqual(labels(day), [(0, 20, None, None), (20, 30, "study", "rule"), (40, 41, None, None)])
        self.assertEqual(day["totals"], {"unknown": 21 * MIN, "study": 10 * MIN})

    def test_declared_private_and_session_time(self):
        day = label_day(
            [app(0, 10, "Preview"), app(10, 11, "Private"), app(11, 30, "Preview"), app(30, 50, "Steam", "com.valvesoftware.steam")],
            [],
            sessions=[{"start": 0, "end": 25 * MIN}],
            declarations=[{"start": 40 * MIN, "end": 50 * MIN, "mode": "study"}],
        )
        self.assertEqual(labels(day), [
            (0, 10, "study", "context"),
            (10, 11, "private", "rule"),  # a minute in a Private app is not folded into study
            (11, 30, "study", "context"),  # the unknown time after the block ends keeps its label
            (30, 40, "relax", "rule"),
            (40, 50, "study", "declared"),
        ])

    def test_what_to_ask_about(self):
        day = label_day(
            # Preview for 12 min (2 of them in a focus block), a glance at Notes, Chrome with no page, Steam
            [app(0, 12, "Preview", "com.apple.Preview"), app(12, 13, "Notes", "com.apple.Notes"),
             app(13, 30, "Google Chrome", "com.google.Chrome"), app(30, 40, "Steam", "com.valvesoftware.steam"),
             app(40, 50, "Private"), app(50, 60, "Zed", "dev.zed.Zed")],
            [page(13, 18, "news.example.com"), page(18, 20, "a.edu")],
            sessions=[{"start": 0, "end": 2 * MIN}],
            rules={"dev.zed.zed": "work"},
        )
        self.assertEqual(day["unknown"], [
            {"key": "com.apple.preview", "name": "Preview", "seconds": 12 * MIN},
            {"key": "news.example.com", "name": "news.example.com", "seconds": 5 * MIN},
        ])
        self.assertEqual(labels(day)[-1], (50, 60, "work", "learned"))
