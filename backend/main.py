"""Sidecar for the Tauri app. Reads one JSON request per line on stdin
({"method": ..., "params": ...}) and writes one JSON reply per line on stdout
({"ok": ...} or {"error": ...}). Never print anything else to stdout.

Spec PR-6: this process must stay offline. Extraction and graph jobs (Phases 4-5) go in METHODS.
"""

import json
import sys
import time

from backend.drift import Drift, Keeper, is_drift, is_music
from backend.modes import classify, label_day
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


_drift = Drift()


def drift(params):
    """params: {app, bundle_id, domain, url, rules, declared}, what is in front now. The focus keeper is
    on while a focus block runs, unless the user has said they are relaxing.
    -> Drift.update's state, plus "mode": study, work, relax, private or None."""
    on = _timer.phase == "focus" and params.get("declared") != "relax"
    rules = params.get("rules")
    what = params.get("domain") or params.get("app")
    state = _drift.update(time.time(), on and is_drift(params, rules), what, music=on and is_music(params, rules))
    # for the pet's face: the mode of this one sample, before any smoothing
    return state | {"mode": classify(params, params.get("declared"), rules, _timer.phase == "focus")[0]}


_keeper = Keeper()


def watch(params):
    """params: {app, bundle_id, domain, url, rules, declared, break_after_min}, what is in front now.
    The focus keeper turns itself on and off (Keeper in backend/drift.py).
    -> Keeper.update's state, plus "mode": study, work, relax, private or None, for the pet's face."""
    rules, declared = params.get("rules"), params.get("declared")
    after = params.get("break_after_min")
    mode = classify(params, declared, rules)[0]
    state = _keeper.update(
        time.time(), mode, is_drift(params, rules), params.get("domain") or params.get("app"),
        music=is_music(params, rules), relax_declared=declared == "relax",
        break_after_min=after if isinstance(after, (int, float)) else 50,
    )
    return state | {"mode": mode}


METHODS = {
    "ping": ping,
    "timer": timer,
    "streak": lambda params: streak(**params),
    "modes": modes,
    "drift": drift,
    "watch": watch,
    "quick_add": lambda params: parse_quick_add(**params),
}

for line in sys.stdin:
    try:
        req = json.loads(line)
        res = {"ok": METHODS[req["method"]](req.get("params"))}
    except Exception as e:
        res = {"error": repr(e)}
    print(json.dumps(res), flush=True)
