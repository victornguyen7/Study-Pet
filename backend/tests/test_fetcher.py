"""Run from the project root:  backend/.venv/bin/python -m unittest backend.tests.test_fetcher
Nothing here touches the network: the download is faked."""

import signal
import subprocess
import sys
import unittest
import urllib.error
import urllib.parse
from datetime import datetime, timezone
from pathlib import Path

from backend.feeds import parse_feed
from backend.fetcher import EVENTS_URI, SCOPE, TOKEN_URI, FeedError, download, fetch, run, sign_in_url

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

    def test_a_crash_in_one_source_is_its_own_error_and_shows_no_secret(self):
        # Google answers without an access token: a KeyError, not a FeedError
        out = run({"feeds": {"canvas": SECRET}, "accounts": {"google": ACCOUNT}}, NOW, lambda url: CANVAS, lambda *a, **k: {"REFRESHTOKEN": 1})
        self.assertEqual((len(out["canvas"]["ok"]), out["google"]), (3, {"error": "the task fetcher failed: KeyError"}))


ACCOUNT = {"client_id": "id", "client_secret": "CLIENTSECRET", "refresh_token": "REFRESHTOKEN"}
# The events of google.ics as Google's API lists them, plus two it must skip.
API_EVENTS = [
    {"iCalUID": "lecture123@google.com", "summary": "STAT 310 lecture", "start": {"dateTime": "2026-10-06T10:00:00-04:00"}, "end": {"dateTime": "2026-10-06T11:15:00-04:00"}},
    {"iCalUID": "dentist456@google.com", "summary": "Dentist", "start": {"dateTime": "2026-10-07T18:00:00Z"}, "end": {"dateTime": "2026-10-07T19:00:00Z"}},
    {"iCalUID": "holiday789@google.com", "summary": "No classes", "start": {"date": "2026-10-09"}, "end": {"date": "2026-10-10"}},
    {"iCalUID": "gone@google.com", "summary": "Called off", "status": "cancelled", "start": {"date": "2026-10-09"}, "end": {"date": "2026-10-10"}},
    {"iCalUID": "lecture123@google.com", "status": "cancelled"},
]


def google(pages, token_error=None):
    """A fake Google: the token endpoint, then the events in `pages`, one page per call."""
    left = list(pages)
    seen = []

    def call(url, data=None, token=None):
        seen.append((url, data, token))
        if url == TOKEN_URI:
            if token_error:
                raise urllib.error.HTTPError(url, token_error, "Bad Request", None, None)
            return {"access_token": "ACCESSTOKEN"}
        page = {"items": left.pop(0)}
        return {**page, "nextPageToken": "next"} if left else page

    call.seen = seen
    return call


class GoogleAccountTest(unittest.TestCase):
    def events(self, call):
        return run({"accounts": {"google": ACCOUNT}}, NOW, call=call)["google"]

    def test_events_match_the_ics_route_row_for_row(self):
        # the same calendar read both ways gives the same rows, uid included, so switching updates in place
        by_start = lambda items: sorted(items, key=lambda i: i["start_at"])
        from_file = parse_feed(GOOGLE, "google", NOW, datetime(2026, 10, 12, tzinfo=timezone.utc))
        self.assertEqual(by_start(self.events(google([API_EVENTS]))["ok"]), by_start(from_file))

    def test_every_page_is_read_with_the_access_token(self):
        call = google([API_EVENTS[:1], API_EVENTS[1:2], API_EVENTS[2:]])
        self.assertEqual(len(self.events(call)["ok"]), 3)
        pages = [(url, token) for url, _, token in call.seen[1:]]
        self.assertEqual([token for _, token in pages], ["ACCESSTOKEN"] * 3)
        self.assertTrue(all(url.startswith(EVENTS_URI) and "singleEvents=true" in url for url, _ in pages))
        self.assertEqual(["pageToken=next" in url for url, _ in pages], [False, True, True])
        self.assertEqual(call.seen[0][1], {**ACCOUNT, "grant_type": "refresh_token"})

    def test_a_refused_token_asks_for_a_new_sign_in_and_shows_no_secret(self):
        for code, message in ((400, "Google sign-in has expired; sign in again"), (401, "Google sign-in has expired; sign in again"), (500, "the server answered 500")):
            self.assertEqual(self.events(google([], token_error=code)), {"error": message})

        def offline(url, data=None, token=None):
            raise urllib.error.URLError(f"no route with {data}")

        self.assertEqual(self.events(offline), {"error": "could not reach Google"})

    def test_the_sign_in_address_asks_for_read_only_access_with_pkce(self):
        query = dict(urllib.parse.parse_qsl(urllib.parse.urlparse(sign_in_url("id", "http://127.0.0.1:1", "STATE", "CHALLENGE")).query))
        self.assertEqual(
            (query["scope"], query["state"], query["code_challenge"], query["code_challenge_method"], query["access_type"]),
            (SCOPE, "STATE", "CHALLENGE", "S256", "offline"),
        )
        self.assertNotIn("client_secret", query)

    def test_a_stuck_sync_is_ended_by_the_alarm(self):
        stuck = (
            "import io, sys, time, backend.fetcher as f\n"
            "f.SYNC_SECONDS = 1\n"
            "f.run = lambda request, now: time.sleep(30)\n"
            "sys.stdin = io.StringIO('{}')\n"
            "f.main()"
        )
        done = subprocess.run([sys.executable, "-c", stuck], timeout=10)
        self.assertEqual(done.returncode, -signal.SIGALRM)


if __name__ == "__main__":
    unittest.main()
