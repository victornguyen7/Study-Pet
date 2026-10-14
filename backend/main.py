"""Sidecar for the Tauri app. Reads one JSON request per line on stdin
({"method": ..., "params": ...}) and writes one JSON reply per line on stdout
({"ok": ...} or {"error": ...}). Never print anything else to stdout.

Spec PR-6: this process must stay offline.
"""

import json
import sys
import time

from backend.drift import Keeper, is_drift, is_music
from backend.modes import classify, label_day
from backend.streaks import streak
from backend.tasks import parse_quick_add


def ping(_params):
    return {"pong": True, "python": sys.version.split()[0]}


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
    "streak": lambda params: streak(**params),
    "modes": lambda params: label_day(**params),
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
