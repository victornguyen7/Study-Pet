"""Sidecar for the Tauri app. Reads one JSON request per line on stdin
({"method": ..., "params": ...}) and writes one JSON reply per line on stdout
({"ok": ...} or {"error": ...}). Never print anything else to stdout.

Spec PR-6: this process must stay offline. Extraction and graph jobs (Phases 4-5) go in METHODS.
"""

import json
import sys
import time

from backend.streaks import streak
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


METHODS = {"ping": ping, "timer": timer, "streak": lambda params: streak(**params)}

for line in sys.stdin:
    try:
        req = json.loads(line)
        res = {"ok": METHODS[req["method"]](req.get("params"))}
    except Exception as e:
        res = {"error": repr(e)}
    print(json.dumps(res), flush=True)
