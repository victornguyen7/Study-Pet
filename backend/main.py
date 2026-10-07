"""Sidecar for the Tauri app. Reads one JSON request per line on stdin
({"method": ..., "params": ...}) and writes one JSON reply per line on stdout
({"ok": ...} or {"error": ...}). Never print anything else to stdout.

Spec PR-6: this process must stay offline. Extraction and graph jobs (Phases 4-5) go in METHODS.
"""

import json
import sys
import time

from backend.modes import label_day
from backend.streaks import streak
from backend.tasks import parse_quick_add
from backend.timer import Timer

_timer = Timer()
TIMER_ACTIONS = {"tick", "start_focus", "skip_break", "resume", "stop"}


def ping(_params):
    return {"pong": True, "python": sys.version.split()[0]}


def timer(params):
    """params: {"action": one of TIMER_ACTIONS, ...keyword arguments for that Timer method}."""
    action = params.pop("action")
    if action not in TIMER_ACTIONS:
        raise ValueError(f"unknown timer action {action!r}")
    now = time.time()  # wall clock, so time asleep with the lid closed still counts
    getattr(_timer, action)(now, **params)
    return _timer.state(now) | {"sessions": _timer.drain_finished()}


def modes(params):
    """params: the arguments of label_day. A focus block still running is added here: only the timer knows it."""
    if _timer.phase == "focus":
        params["sessions"] = [*params.get("sessions", []), {"start": _timer.started_at, "end": time.time()}]
    return label_day(**params)


METHODS = {
    "ping": ping,
    "timer": timer,
    "streak": lambda params: streak(**params),
    "modes": modes,
    "quick_add": lambda params: parse_quick_add(**params),
}

for line in sys.stdin:
    try:
        req = json.loads(line)
        res = {"ok": METHODS[req["method"]](req.get("params"))}
    except Exception as e:
        res = {"error": repr(e)}
    print(json.dumps(res), flush=True)
