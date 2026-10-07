"""The task fetcher: the only process allowed on the network (spec PR-6), and it only reads.

Run from the project root:  backend/.venv/bin/python -m backend.fetcher
Reads one JSON object on stdin, {"feeds": {"canvas": url, ...}, "files": {"google": ics_text, ...}}, and
writes one JSON object on stdout with {"ok": [items]} or {"error": "..."} per source. A feed is downloaded;
a file is an .ics the user uploaded, which only needs parsing. The items are parse_feed's.

Google by sign-in (docs/google_calendar_oauth.md) adds two keys to the request:
"accounts": {"google": {"client_id", "client_secret", "refresh_token"}} reads the primary calendar, and
"connect": {"google": {"client_id", "client_secret"}} signs in through the browser and replies
{"ok": {"refresh_token": ...}}.

A feed URL, a token and a client secret are secrets: they never go in an argument, a log or an error message.
"""

import base64
import hashlib
import http.server
import secrets
import signal
import urllib.parse
import webbrowser

import http.client
import json
import re
import ssl
import sys
import urllib.error
import urllib.request
from datetime import datetime, timedelta, timezone

import certifi

from backend.feeds import google_events, parse_feed

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


AUTH_URI = "https://accounts.google.com/o/oauth2/v2/auth"
TOKEN_URI = "https://oauth2.googleapis.com/token"
EVENTS_URI = "https://www.googleapis.com/calendar/v3/calendars/primary/events"
SCOPE = "https://www.googleapis.com/auth/calendar.readonly"
SIGN_IN_SECONDS = 300
SYNC_SECONDS = 120  # a whole sync; each network operation has its own 20 s


def call(url, data=None, token=None):
    """One request to Google, answered in JSON. `data` makes it a form POST; `token` is an access token."""
    req = urllib.request.Request(url, data=urllib.parse.urlencode(data).encode() if data else None)
    if token:
        req.add_header("Authorization", f"Bearer {token}")
    context = ssl.create_default_context(cafile=certifi.where())
    with urllib.request.urlopen(req, timeout=20, context=context) as res:
        return json.loads(res.read(MAX_BYTES))


def _ask(call, *args, refused=None, **kwargs):
    """call(), with its failures turned into messages that are safe to show. `refused` is for a 400 or 401."""
    try:
        return call(*args, **kwargs)
    except urllib.error.HTTPError as e:
        raise FeedError(refused if refused and e.code in (400, 401) else f"the server answered {e.code}") from None
    except (OSError, http.client.HTTPException, ValueError):
        raise FeedError("could not reach Google") from None


def account_events(source, account, now, call=call):
    """The primary calendar's events for a signed-in account, in the window an .ics feed is read in."""
    # the refresh token is traded for an access token that lives an hour and is never stored
    grant = {**account, "grant_type": "refresh_token"}
    token = _ask(call, TOKEN_URI, data=grant, refused="Google sign-in has expired; sign in again")["access_token"]
    query = {
        "timeMin": (now - BEFORE).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "timeMax": (now + AFTER).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "singleEvents": "true",  # Google expands repeating events
        "maxResults": "2500",
    }
    items = []
    while True:
        page = _ask(call, EVENTS_URI + "?" + urllib.parse.urlencode(query), token=token)
        items += page.get("items", [])
        if "nextPageToken" not in page:
            return google_events(items, source)
        query["pageToken"] = page["nextPageToken"]


def sign_in_url(client_id, redirect, state, challenge):
    """The page the browser opens: read-only calendar access, with PKCE, asking for a refresh token."""
    return AUTH_URI + "?" + urllib.parse.urlencode(
        {
            "client_id": client_id,
            "redirect_uri": redirect,
            "response_type": "code",
            "scope": SCOPE,
            "state": state,
            "code_challenge": challenge,
            "code_challenge_method": "S256",
            "access_type": "offline",
            "prompt": "consent",  # without it Google gives no refresh token on a second sign-in
        }
    )


def connect(client, call=call, open_browser=webbrowser.open):
    """Signs in through the browser and returns {"refresh_token": ...}. Waits SIGN_IN_SECONDS at most."""
    answer = {}

    class Handler(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            query = urllib.parse.parse_qs(urllib.parse.urlparse(self.path).query)
            if "code" in query or "error" in query:
                answer.update({k: v[0] for k, v in query.items()})
            self.send_response(200)
            self.send_header("Content-Type", "text/plain; charset=utf-8")
            self.end_headers()
            self.wfile.write(b"Done. You can close this tab and go back to Study Pet.")

        def log_message(self, *args):  # the request line holds the sign-in code
            pass

    with http.server.HTTPServer(("127.0.0.1", 0), Handler) as server:
        server.timeout = SIGN_IN_SECONDS
        redirect = f"http://127.0.0.1:{server.server_port}"
        verifier, state = secrets.token_urlsafe(64), secrets.token_urlsafe(16)
        challenge = base64.urlsafe_b64encode(hashlib.sha256(verifier.encode()).digest()).rstrip(b"=").decode()
        open_browser(sign_in_url(client["client_id"], redirect, state, challenge))
        # ponytail: one request is waited for; a stray one (a favicon, a port scan) ends the sign-in early
        server.handle_request()
    if "code" not in answer or answer.get("state") != state:
        raise FeedError("Sign-in was not completed")
    grant = {**client, "code": answer["code"], "code_verifier": verifier, "grant_type": "authorization_code", "redirect_uri": redirect}
    tokens = _ask(call, TOKEN_URI, data=grant, refused="Sign-in was not completed")
    if not tokens.get("refresh_token"):
        raise FeedError("Sign-in was not completed")
    return {"refresh_token": tokens["refresh_token"]}


def run(request, now, download=download, call=call):
    """One reply per source: {"ok": [items]} or {"error": "..."}."""
    jobs = [(source, lambda url=url, source=source: fetch(source, url, now, download)) for source, url in request.get("feeds", {}).items()]
    jobs += [(source, lambda ics=ics, source=source: read(source, ics, now)) for source, ics in request.get("files", {}).items()]
    jobs += [(source, lambda a=a, source=source: account_events(source, a, now, call)) for source, a in request.get("accounts", {}).items()]
    jobs += [(source, lambda client=client: connect(client, call)) for source, client in request.get("connect", {}).items()]
    out = {}
    for source, job in jobs:
        try:
            out[source] = {"ok": job()}
        except FeedError as e:
            out[source] = {"error": str(e)}
        except Exception as e:  # a bug, or a reply in a shape nobody expected: only its kind is safe to show
            out[source] = {"error": f"the task fetcher failed: {type(e).__name__}"}
    return out


def main():
    request = json.load(sys.stdin)
    # The app waits on this process while holding its sync lock, so it must end. The alarm's default action
    # kills the process even when it is stuck outside Python, such as on a server that sends a byte at a time.
    # ponytail: Unix only, like the Keychain calls in lib.rs
    signal.alarm(SIGN_IN_SECONDS + 60 if "connect" in request else SYNC_SECONDS)
    json.dump(run(request, datetime.now(timezone.utc)), sys.stdout)


if __name__ == "__main__":
    main()
