"""Pure utility functions — no internal package dependencies."""

import datetime as _dt
import json as _json


def flt(v, precision=None):
    if v is None:
        return 0.0
    try:
        result = float(v)
        return round(result, int(precision)) if precision is not None else result
    except (TypeError, ValueError):
        return 0.0


def cint(v, default=0):
    if v is None:
        return default
    try:
        return int(float(v))
    except (TypeError, ValueError):
        return default


def cstr(v, encoding="utf-8"):
    if v is None:
        return ""
    if isinstance(v, bytes):
        return v.decode(encoding)
    return str(v)


def fmt_money(amount, precision=2, currency="USD"):
    return f"{currency} {flt(amount):,.{precision}f}"


def nowdate():
    return _dt.date.today().isoformat()


def now_datetime():
    return _dt.datetime.now()


def now():
    return _dt.datetime.now().isoformat()


def today():
    return _dt.date.today().isoformat()


def getdate(string_date=None):
    if string_date is None:
        return _dt.date.today()
    if isinstance(string_date, _dt.datetime):
        return string_date.date()
    if isinstance(string_date, _dt.date):
        return string_date
    return _dt.date.fromisoformat(str(string_date)[:10])


def get_datetime(dt=None):
    if dt is None:
        return _dt.datetime.now()
    if isinstance(dt, _dt.datetime):
        return dt
    if isinstance(dt, _dt.date):
        return _dt.datetime(dt.year, dt.month, dt.day)
    try:
        return _dt.datetime.fromisoformat(str(dt))
    except Exception:
        return _dt.datetime.now()


def add_days(date, days):
    return (_dt.date.fromisoformat(str(date)[:10]) + _dt.timedelta(days=int(days))).isoformat()


def date_diff(date1, date2):
    return (getdate(date1) - getdate(date2)).days


def _(msg, lang=None, context=None):
    """Translation stub — returns the string as-is."""
    return msg


def scrub(txt):
    return cstr(txt).replace(" ", "_").replace("-", "_").lower()


def unscrub(txt):
    return str(txt).replace("_", " ").replace("-", " ").title()


def bold(text):
    return f"<b>{text}</b>"


def parse_json(val):
    if isinstance(val, str):
        try:
            return _json.loads(val)
        except Exception:
            return val
    return val


def as_json(val, indent=1):
    return _json.dumps(val, indent=indent, default=str)


def safe_decode(val, encoding="utf-8"):
    if isinstance(val, bytes):
        return val.decode(encoding)
    return val
