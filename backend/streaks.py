"""Streaks (spec "Timelapse and streaks"): a day counts once it has `threshold` minutes of study.
Two freezes a month each cover one missed day. Milestones at 7, 30 and 100 days.
"""

from datetime import date, timedelta

FREEZES_PER_MONTH = 2
MILESTONES = (7, 30, 100)

# ponytail: recomputed from the full session history on every call, so changing the threshold
# re-judges past days. Store a per-day "counted" flag (the spec's Day summary) if that must stick.


def streak(minutes, today, threshold):
    """minutes: {"YYYY-MM-DD": study minutes that day}. today: "YYYY-MM-DD"."""
    today = date.fromisoformat(today)
    days = 0
    used = {}  # (year, month) -> freezes spent

    # Walk forward through every finished day, so a freeze is spent when the miss happens.
    day = min(map(date.fromisoformat, minutes), default=today)
    while day < today:
        month = (day.year, day.month)
        if minutes.get(day.isoformat(), 0) >= threshold:
            days += 1
        elif days and used.get(month, 0) < FREEZES_PER_MONTH:
            used[month] = used.get(month, 0) + 1  # streak survives, but the day adds nothing
        else:
            days = 0
        day += timedelta(days=1)

    # Today isn't over: it extends the streak once counted, and never breaks it.
    today_minutes = minutes.get(today.isoformat(), 0)
    counted = today_minutes >= threshold
    days += counted

    return {
        "days": days,
        "today_minutes": int(today_minutes),
        "today_counted": counted,
        "threshold": threshold,
        "freezes_left": FREEZES_PER_MONTH - used.get((today.year, today.month), 0),
        "milestone": max((m for m in MILESTONES if days >= m), default=0),
    }
