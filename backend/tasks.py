"""Tasks (spec "Daily to-do list and integrations"). Quick add turns one typed line,
such as "STAT 310 HW3 Fri 2h", into the fields of a task row.
"""

import re
from datetime import date, timedelta

DAYS = ("monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday")
# "fri" and "friday" -> 4, plus the usual in-between spellings
WEEKDAY = {name: i for i, day in enumerate(DAYS) for name in (day, day[:3])}
WEEKDAY |= {"tues": 1, "thur": 3, "thurs": 3}
# words that only lead into a due date ("due on Friday", "by next Fri"); dropped along with it
DUE_WORDS = {"due", "on", "by", "next", "this", "in", "at", "for", "before", "until", "till"}
ESTIMATE = re.compile(r"(?:(\d+(?:\.\d+)?)h)?(?:(\d+)m)?", re.I)
COURSE = re.compile(r"([a-z]{2,5})\s*(\d{2,4}[a-z]?)", re.I)


def _estimate(token):
    """ "2h", "30m", "1.5h" or "1h30m" -> minutes, else None."""
    m = ESTIMATE.fullmatch(token)
    if not m:
        return None
    total = round(float(m[1] or 0) * 60) + int(m[2] or 0)
    return total or None


def _due(token, today):
    """ "today", "tomorrow", a weekday name, "10/23" or "2026-10-23" -> date, else None."""
    word = token.lower()
    if word in ("today", "tod"):
        return today
    if word in ("tomorrow", "tmr"):
        return today + timedelta(days=1)
    if word in WEEKDAY:
        # a weekday that is today means today: a task due in hours must not slip a week
        return today + timedelta(days=(WEEKDAY[word] - today.weekday()) % 7)
    try:
        if re.fullmatch(r"\d{1,2}/\d{1,2}", word):
            month, day = map(int, word.split("/"))
            due = date(today.year, month, day)
            return due if due >= today else date(today.year + 1, month, day)
        return date.fromisoformat(word)
    except ValueError:
        return None


def parse_quick_add(text, today):
    """text: the typed line. today: "YYYY-MM-DD". Returns title, course, due_date, estimate_min;
    whatever the line doesn't give is None. The due date and estimate are read off the end of
    the line, in either order, so the same words inside a title are left alone.
    ponytail: "next Friday" is read as the coming Friday, the same as "Friday"."""
    today = date.fromisoformat(today)
    tokens = text.split()
    due = estimate = None
    while tokens:
        if due is None and (found := _due(tokens[-1], today)):
            due = found
            while len(tokens) > 1 and tokens[-2].lower() in DUE_WORDS:
                del tokens[-2]
        elif estimate is None and (found := _estimate(tokens[-1])):
            estimate = found
        else:
            break
        tokens.pop()

    # ponytail: a course is guessed from shape alone (letters then a number, at the start), so
    # "read 100 pages" becomes course READ 100. Match against the user's known courses instead
    # once the Canvas import supplies them.
    title, course = " ".join(tokens), None
    if (m := COURSE.match(title)) and (rest := title[m.end():].strip()) and title[m.end()] == " ":
        title, course = rest, f"{m[1].upper()} {m[2].upper()}"

    if not title:
        raise ValueError("a task needs a title")
    return {
        "title": title,
        "course": course,
        "due_date": due.isoformat() if due else None,
        "estimate_min": estimate,
    }
