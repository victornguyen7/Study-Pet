"""Run from the project root:  backend/.venv/bin/python -m unittest backend.tests.test_fetcher
Nothing here touches the network: the download is faked."""

import unittest
import urllib.error
from datetime import datetime, timezone
from pathlib import Path

from backend.fetcher import FeedError, download, fetch, run

NOW = datetime(2026, 10, 5, tzinfo=timezone.utc)
SECRET = "https://school.instructure.com/feeds/calendars/user_SECRET.ics"
CANVAS = (Path(__file__).parent / "canvas.ics").read_bytes()
GOOGLE = (Path(__file__).parent / "google.ics").read_text(encoding="utf-8")


class FetcherTest(unittest.TestCase):
    def error(self, download):
        with self.assertRaises(FeedError) as caught:
            fetch("canvas", SECRET, NOW, download)
        self.assertNotIn("SECRET", str(caught.exception))
        return str(caught.exception)

    def test_a_good_feed_is_parsed(self):
        self.assertEqual(len(fetch("canvas", SECRET, NOW, lambda url: CANVAS)), 3)

    def test_only_https_is_opened(self):
        for url in ("file:///etc/passwd", "http://example.com/a.ics", "/etc/passwd", ""):
            with self.assertRaises(FeedError):
                download(url)

    def test_errors_never_show_the_url(self):
        def http_404(url):
            raise urllib.error.HTTPError(url, 404, "Not Found", None, None)

        def offline(url):
            raise urllib.error.URLError(f"no route to {url}")

        self.assertEqual(self.error(http_404), "the server answered 404")
        self.assertEqual(self.error(offline), "could not reach that address")

    def test_a_page_that_is_not_a_calendar_is_an_error(self):
        # an empty list here would flag every upcoming task as removed
        for body in (b"", b"<html>Please log in</html>", b"\xff\xfe"):
            self.assertIn("not a calendar", self.error(lambda url: body))

    def test_an_uploaded_file_is_parsed_without_a_download(self):
        def no_network(url):
            raise AssertionError("a file must not be downloaded")

        out = run({"files": {"google": GOOGLE, "canvas": "not a calendar"}}, NOW, no_network)
        self.assertEqual({i["title"] for i in out["google"]["ok"]}, {"STAT 310 lecture", "Dentist", "No classes"})
        self.assertEqual(out["canvas"], {"error": "that is not a calendar this app can read"})

    def test_one_bad_feed_does_not_sink_the_others(self):
        def one_down(url):
            if url != SECRET:
                raise urllib.error.URLError("offline")
            return CANVAS

        out = run({"feeds": {"canvas": SECRET, "google": "https://example.com/a.ics"}}, NOW, one_down)
        self.assertEqual((len(out["canvas"]["ok"]), out["google"]), (3, {"error": "could not reach that address"}))


if __name__ == "__main__":
    unittest.main()
