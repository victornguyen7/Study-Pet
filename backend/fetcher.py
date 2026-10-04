"""The task fetcher: the only process allowed on the network (spec PR-6), and it only reads.

Run from the project root:  backend/.venv/bin/python -m backend.fetcher
Reads one JSON object on stdin, {"feeds": {"canvas": url, ...}, "files": {"google": ics_text, ...}}, and
writes one JSON object on stdout with {"ok": [items]} or {"error": "..."} per source. A feed is downloaded;
a file is an .ics the user uploaded, which only needs parsing. The items are parse_feed's.

A feed URL is a secret: it never goes in an argument, a log or an error message.
"""

import http.client
import json
import re
import ssl
import sys
import urllib.error
import urllib.request
from datetime import datetime, timedelta, timezone

import certifi

from backend.feeds import parse_feed

MAX_BYTES = 5_000_000
# calendar events are kept from yesterday to a month ahead; assignments are kept whatever their date
BEFORE, AFTER = timedelta(days=1), timedelta(days=30)


class FeedError(Exception):
    """Its message is shown in the panel, so it must not contain the URL."""


def download(url):
    url = re.sub(r"^webcal://", "https://", url.strip(), flags=re.I)
    if not url.lower().startswith("https://"):  # urlopen would also open file:// paths
        raise FeedError("a feed address must start with https://")
    # python.org's macOS build ships without root certificates, so bring our own list
    context = ssl.create_default_context(cafile=certifi.where())
    with urllib.request.urlopen(url, timeout=20, context=context) as res:
        body = res.read(MAX_BYTES + 1)
    if len(body) > MAX_BYTES:
        raise FeedError("the feed is larger than 5 MB")
    return body


def read(source, ics, now):
    """Returns the items in a calendar's text, or raises FeedError with a message that is safe to show."""
    try:
        return parse_feed(ics, source, now - BEFORE, now + AFTER)
    except Exception:  # not a calendar (a login page, say), or one the parser cannot read
        raise FeedError("that is not a calendar this app can read") from None


def fetch(source, url, now, download=download):
    """Downloads a feed and returns its items, or raises FeedError with a message that is safe to show."""
    try:
        body = download(url)
    except FeedError:
        raise
    except urllib.error.HTTPError as e:
        raise FeedError(f"the server answered {e.code}") from None
    except (OSError, http.client.HTTPException, ValueError):
        raise FeedError("could not reach that address") from None
    return read(source, body.decode("utf-8", errors="replace"), now)


def run(request, now, download=download):
    """One reply per source: {"ok": [items]} or {"error": "..."}."""
    jobs = [(source, lambda url=url, source=source: fetch(source, url, now, download)) for source, url in request.get("feeds", {}).items()]
    jobs += [(source, lambda ics=ics, source=source: read(source, ics, now)) for source, ics in request.get("files", {}).items()]
    out = {}
    for source, job in jobs:
        try:
            out[source] = {"ok": job()}
        except FeedError as e:
            out[source] = {"error": str(e)}
    return out


if __name__ == "__main__":
    json.dump(run(json.load(sys.stdin), datetime.now(timezone.utc)), sys.stdout)
