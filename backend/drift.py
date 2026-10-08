"""Drift ladder (spec "Focus keeper"): while a focus block runs, time on something that is Relax
makes the pet step in, a little more the longer it lasts.

Step 1 after 30 s (a glance), step 2 after 2 min (a speech bubble), step 3 after 5 min (the pet sits
in the way), step 4 after 10 min (the app blocks the site for 5 minutes, if the user opted in).
"""

from urllib.parse import urlsplit

from backend.modes import GAP_SECS, classify

STEPS = ((30, 1), (120, 2), (300, 3), (600, 4))  # seconds of drift -> step
MUSIC_GRACE = 60  # a music app or tab may be in front this long, to skip a song, before it is drift


def feed(url):
    """Feed patterns on a site that is otherwise fine: YouTube's home page, its feeds and Shorts."""
    parts = urlsplit(url or "")
    host = parts.hostname or ""
    return (host == "youtube.com" or host.endswith(".youtube.com")) and (
        parts.path in ("", "/") or parts.path.startswith(("/shorts", "/feed"))
    )


def is_drift(front, rules=None):
    """front: {app, bundle_id, domain, url}, what is in front now. Unknown and Private things are not drift."""
    return classify(front, rules=rules)[0] == "relax" or feed(front.get("url"))


def is_music(front, rules=None):
    """A music app or site in front: fine for a minute, drift after that."""
    return classify(front, rules=rules)[1] == "music"


class Drift:
    def __init__(self):
        self.since = None  # when this stretch of drift began
        self.music_since = None  # when a music app or tab came to the front
        self.last = 0.0  # when update was last called
        self.step = 0

    # ponytail: one sample that is not drift ends the stretch, so a 5-second look at the notes starts
    # the ladder again from nothing. Let a short return keep the count if that gets gamed.
    def update(self, now, drifting, what=None, music=False):
        """Called with every sample of the front app. -> {"step": 0 to 4, "seconds": drift so far,
        "what": its name, "returned": True on the one update that ends drift the pet had spoken up about}.
        `music` is not drift until it has been in front for MUSIC_GRACE; the ladder starts there."""
        if now - self.last > GAP_SECS:
            self.since, self.music_since, self.step = None, None, 0  # a break or a closed lid: nothing carries across it
        self.last = now
        self.music_since = (now if self.music_since is None else self.music_since) if music else None
        drifting = drifting or (music and now - self.music_since >= MUSIC_GRACE)
        returned = not drifting and self.step >= 2
        if drifting:
            self.since = self.since if self.since is not None else now
            self.step = max((step for after, step in STEPS if now - self.since >= after), default=0)
        else:
            self.since, self.step = None, 0
        return {
            "step": self.step,
            "seconds": int(now - self.since) if drifting else 0,
            "what": what if drifting else None,
            "returned": returned,
        }
