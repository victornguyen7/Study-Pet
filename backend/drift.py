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


ON_AFTER = 300  # seconds of Study or Work before the keeper turns on
OFF_AFTER = 900  # this much Relax in a row was a break: the keeper turns off
AWAY_SECS = 300  # no sample for this long (the lid was shut, or nobody was there) was a break too


class Keeper:
    """Turns the focus keeper on and off by itself, and runs the ladder (Drift) while it is on.

    On after ON_AFTER of Study or Work. Off after a break: OFF_AFTER of Relax in a row, AWAY_SECS
    with no sample, or the user saying they are relaxing. A break is suggested once per run.
    """

    def __init__(self):
        self.drift = Drift()
        self.last = None  # when update was last called
        self.earned = 0  # seconds of Study or Work since the keeper was last off
        self.on_since = None  # None while the keeper is off
        self.suggested = False  # a break has been suggested in this run

    def _off(self):
        self.drift = Drift()  # a fresh ladder, so a break is not answered with "welcome back"
        self.earned, self.on_since, self.suggested = 0, None, False

    # ponytail: while the keeper is off, `earned` only goes up, so two minutes of work, an hour of
    # Relax with no gap, and three more minutes turn it on. Let Relax wear `earned` down if that
    # turns it on too easily.
    def update(self, now, mode, drifting, what=None, music=False, relax_declared=False, break_after_min=50):
        """Called with every sample of the front app. mode: study, work, relax, private or None.
        -> Drift.update's state, plus {"on": the keeper is on, "break": True on the one update
        that suggests a break}."""
        gap = AWAY_SECS if self.last is None else now - self.last
        self.last = now
        if gap >= AWAY_SECS or relax_declared:
            self._off()
        elif mode in ("study", "work") and not drifting and not music and gap <= GAP_SECS:
            self.earned += gap
        if self.on_since is None and self.earned >= ON_AFTER:
            self.on_since = now
        on = self.on_since is not None
        state = self.drift.update(now, on and drifting, what, music=on and music)
        if state["seconds"] >= OFF_AFTER:
            self._off()
            on, state = False, self.drift.update(now, False)
        # the run began ON_AFTER before the keeper turned on
        suggest = (
            on and not self.suggested and break_after_min > 0
            and now - self.on_since + ON_AFTER >= break_after_min * 60
        )
        self.suggested = self.suggested or suggest
        return state | {"on": on, "break": suggest}
