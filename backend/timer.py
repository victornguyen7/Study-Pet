"""Session timer state machine (spec S-1, S-3..S-6).

    idle -> focus -> break -> callback -> focus -> ...

Every method takes `now` (epoch seconds) instead of reading the clock, so tests can fake time.
"""

CALLBACK_REPEAT_S = 120  # S-4: call the user back again once, 2 minutes after the break ends
SKIPS_BEFORE_SUGGESTION = 3  # S-5

# ponytail: state lives in memory only, so quitting the app mid-session drops the block in
# progress. Persist the running block if the spec's "lose at most the last minute" starts to matter.


class Timer:
    def __init__(self):
        self.phase = "idle"
        self.started_at = self.ends_at = 0.0
        self.focus_min = self.break_min = 0
        self.label = None
        self.calls = 0  # how many times the pet has called the user back from this break
        self.skipped = 0  # breaks skipped in a row
        self.finished = []  # completed focus blocks, waiting for the app to store them

    def start_focus(self, now, focus_min, break_min, label=None):
        if not (focus_min > 0 and break_min > 0):
            raise ValueError("focus_min and break_min must be positive")
        self._end_focus(now)
        self.focus_min, self.break_min, self.label = focus_min, break_min, label
        self.skipped = 0
        self._focus(now)

    def skip_break(self, now):
        if self.phase == "break":
            self.skipped += 1
            self._focus(now)

    def resume(self, now):
        """The user came back after a break."""
        if self.phase == "callback":
            self._focus(now)

    def stop(self, now):
        self._end_focus(now)
        self.phase = "idle"
        self.skipped = 0

    def tick(self, now):
        # Plain ifs, not elifs: after the laptop sleeps, one tick walks through every phase it missed.
        if self.phase == "focus" and now >= self.ends_at:
            self._end_focus(self.ends_at)
            self.phase = "break"
            self.ends_at += self.break_min * 60
        if self.phase == "break" and now >= self.ends_at:
            self.phase = "callback"
            self.calls = 1
            self.skipped = 0  # this break was taken
        if self.phase == "callback" and self.calls == 1 and now >= self.ends_at + CALLBACK_REPEAT_S:
            self.calls = 2

    def state(self, now):
        counting = self.phase in ("focus", "break")
        return {
            "phase": self.phase,
            "remaining": max(0, round(self.ends_at - now)) if counting else 0,
            "focus_min": self.focus_min,
            "break_min": self.break_min,
            "label": self.label,
            "calls": self.calls if self.phase == "callback" else 0,
            "suggest_real_break": self.phase == "break" and self.skipped >= SKIPS_BEFORE_SUGGESTION,
        }

    def drain_finished(self):
        finished, self.finished = self.finished, []
        return finished

    def _focus(self, now):
        self.phase = "focus"
        self.started_at = now
        self.ends_at = now + self.focus_min * 60

    def _end_focus(self, at):
        # S-6: a block of any length counts, including one stopped early
        if self.phase == "focus":
            self.finished.append({
                "started_at": self.started_at,
                "ended_at": min(at, self.ends_at),
                "planned_min": self.focus_min,
                "break_min": self.break_min,
                "label": self.label,
            })
