"""Modes (spec "Activity modes"): every stretch of time is Study, Work, Relax or Private.

Signals, strongest first: a Private app (the gate already took its name), the user's declaration,
a learned rule, the category of the app or site, then context (a focus block is running).
A label changes only after about 3 minutes; shorter detours keep the label they interrupt.
"""

SMOOTH_SECS = 180
GAP_SECS = 15  # as in browser.rs: two spans further apart than this are not neighbors

# ponytail: a short fixed list, by bundle ID or site (a site also covers its subdomains). Anything
# else is unknown until the user is asked ("ask once and remember"). YouTube is left out on purpose:
# a lecture and a vlog share the site, and telling them apart needs the page (Phase 4).
CATEGORIES = {
    "study": [
        "edu", "instructure.com", "gradescope.com", "piazza.com", "khanacademy.org", "coursera.org",
        "edx.org", "quizlet.com", "overleaf.com", "wolframalpha.com", "net.ankiweb.dtop",
    ],
    "work": ["linkedin.com", "indeed.com", "joinhandshake.com", "glassdoor.com"],
    "relax": [
        "netflix.com", "twitch.tv", "tiktok.com", "instagram.com", "facebook.com", "x.com",
        "twitter.com", "reddit.com", "hulu.com", "disneyplus.com", "primevideo.com",
        "com.valvesoftware.steam", "com.apple.TV", "com.epicgames.launcher",
    ],
}
# never asked about: a browser is whatever page is in it
BROWSERS = {
    "com.google.chrome", "com.apple.safari", "org.mozilla.firefox", "com.microsoft.edgemac",
    "company.thebrowser.browser", "com.brave.browser",
}
ASK_SECS = 180  # an app or site nothing knows is asked about once it has had this long in front today

# Background audio (spec "Background audio"): a music app or site never changes the mode. In front it
# has no label of its own, so it takes the label around it, and it is never asked about.
MUSIC = [
    "com.spotify.client", "com.apple.Music", "open.spotify.com", "music.youtube.com", "soundcloud.com",
    "music.apple.com",
]
_CATEGORY = {key.lower(): mode for mode, keys in (CATEGORIES | {"music": MUSIC}).items() for key in keys}


def key(span):
    """What a rule is about: the page's site, else the app's bundle ID, else its name."""
    return (span.get("domain") or span.get("bundle_id") or span.get("app") or "").lower()


def category(span):
    if span.get("domain"):
        parts = span["domain"].lower().split(".")
        # "canvas.school.edu" tries itself, then "school.edu", then "edu"
        return next((_CATEGORY[s] for i in range(len(parts)) if (s := ".".join(parts[i:])) in _CATEGORY), None)
    return _CATEGORY.get(key(span))


def classify(span, declared=None, rules=None, in_session=False):
    """-> (mode or None, source). source is the spec's label source: declared, learned, rule or context,
    or "music" for a music app or site, which has no mode."""
    if span.get("app") == "Private" and not span.get("bundle_id"):
        return "private", "rule"
    if declared:
        return declared, "declared"
    if learned := (rules or {}).get(key(span)):
        return learned, "learned"
    if mode := category(span):
        return (None, "music") if mode == "music" else (mode, "rule")
    if in_session:
        return "study", "context"
    return None, None


def timeline(apps, pages):
    """One list in time order. A page wins over the browser it is in: the app's span is cut around it."""
    pages = sorted((p for p in pages if p["end"] > p["start"]), key=lambda p: p["start"])
    out = list(pages)
    for app in apps:
        start = app["start"]
        for page in pages:
            if page["end"] <= start or page["start"] >= app["end"]:
                continue
            if page["start"] > start:
                out.append(app | {"start": start, "end": page["start"]})
            start = max(start, page["end"])
        if start < app["end"]:
            out.append(app | {"start": start})
    return sorted(out, key=lambda s: s["start"])


def _cut(spans, intervals):
    """Splits spans where an interval starts or ends, so each piece is wholly inside it or outside."""
    edges = sorted({t for i in intervals for t in (i["start"], i["end"])})
    for span in spans:
        start = span["start"]
        for edge in edges:
            if start < edge < span["end"]:
                yield span | {"start": start, "end": edge}
                start = edge
        yield span | {"start": start}


def _covering(intervals, span):
    return next((i for i in intervals if i["start"] <= span["start"] < i["end"]), None)


def _fixed(span):
    """Declared and Private time is never relabeled."""
    return span["source"] == "declared" or span["mode"] == "private"


def _merge(spans):
    """Joins neighbors with the same label; the first one's source is kept, and every name in `what`."""
    out = []
    for s in spans:
        last = out[-1] if out else None
        if last and last["mode"] == s["mode"] and _fixed(last) == _fixed(s) and s["start"] - last["end"] <= GAP_SECS:
            last["end"] = s["end"]
            last["what"] = last["what"] + [w for w in s["what"] if w not in last["what"]]
        else:
            out.append(dict(s))
    return out


def smooth(spans):
    """A stretch under 3 minutes, or one with no label, takes the label it interrupts."""
    current, last_end, out = None, None, []
    for s in _merge(spans):
        if last_end is not None and s["start"] - last_end > GAP_SECS:
            current = None  # a break or a closed lid: nothing carries across it
        short = s["mode"] is None or s["end"] - s["start"] < SMOOTH_SECS
        if _fixed(s):
            if s["source"] == "declared":
                current = s["mode"]  # a glance at a Private app leaves the label as it was
        elif short and current and s["mode"] != current:
            s = s | {"mode": current, "source": "smoothed"}
        else:
            current = s["mode"]
        last_end = s["end"]
        out.append(s)
    return _merge(out)


def usage(spans):
    """spans: timeline()'s pieces. -> [{name, site, seconds}], longest first: one total per app and site.
    A page has already been cut out of its browser's span, so no time is counted twice."""
    totals = {}
    for s in spans:
        row = (s.get("domain") or s["app"], bool(s.get("domain")))
        totals[row] = totals.get(row, 0) + s["end"] - s["start"]
    return [
        {"name": name, "site": site, "seconds": seconds}
        for (name, site), seconds in sorted(totals.items(), key=lambda t: -t[1])
        if seconds > 0
    ]


def label_day(apps, pages, sessions=(), declarations=(), rules=None):
    """apps: [{start, end, app, bundle_id}], pages: [{start, end, domain}], sessions: [{start, end}],
    declarations: [{start, end, mode}], rules: {key: mode}. Times are epoch seconds.
    -> {"spans": [{start, end, mode, source, what: the apps and sites in it}], "totals": {mode or "unknown": seconds},
        "unknown": [{key, name, site, seconds}], longest first: what to ask the user about,
        "usage": [{name, site, seconds}], longest first: the time in each app and site}"""
    pieces = timeline(apps, pages)
    labeled, unknown = [], {}
    for span in _cut(pieces, [*sessions, *declarations]):
        declared = _covering(declarations, span)
        mode, source = classify(span, declared and declared["mode"], rules, bool(_covering(sessions, span)))
        what = span.get("domain") or span["app"]
        labeled.append({"start": span["start"], "end": span["end"], "mode": mode, "source": source, "what": [what]})
        # a guess from context is still a guess, so it is asked about too
        if source in (None, "context", "declared") and key(span) not in BROWSERS | (rules or {}).keys() and not category(span):
            ask = unknown.setdefault(key(span), {"key": key(span), "name": what, "site": bool(span.get("domain")), "seconds": 0})
            ask["seconds"] += span["end"] - span["start"]
    spans = smooth(labeled)
    totals = {}
    for s in spans:
        name = s["mode"] or "unknown"
        totals[name] = totals.get(name, 0) + s["end"] - s["start"]
    asks = sorted((u for u in unknown.values() if u["seconds"] >= ASK_SECS), key=lambda u: -u["seconds"])
    return {"spans": spans, "totals": totals, "unknown": asks, "usage": usage(pieces)}
