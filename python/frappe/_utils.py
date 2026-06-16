"""Pure utility functions — minimal internal package dependencies."""

import datetime as _dt
import json as _json

from ._types import _dict


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


def as_unicode(v, encoding="utf-8"):
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


def getdate(v=None):
    if v is None:
        return _dt.date.today()
    if isinstance(v, _dt.datetime):
        return v.date()
    if isinstance(v, _dt.date):
        return v
    if isinstance(v, str):
        return _dt.date.fromisoformat(v.split()[0])
    return v


def get_datetime(v=None):
    if v is None:
        return _dt.datetime.now()
    if isinstance(v, _dt.datetime):
        return v
    if isinstance(v, _dt.date):
        return _dt.datetime.combine(v, _dt.time.min)
    if isinstance(v, str):
        return _dt.datetime.fromisoformat(v)
    return v


def add_days(date, days):
    return getdate(date) + _dt.timedelta(days=days)


def date_diff(a, b):
    return (getdate(a) - getdate(b)).days


# Translation helpers — return the input unchanged.  Real Frappe replaces these
# with lazy translation objects, but the shim must provide them so real Frappe
# submodules can be imported before the real frappe module is initialized.
def _(text, *args, **kwargs):
    if args:
        try:
            return text % args
        except Exception:
            pass
    return text


def _lt(text, *args, **kwargs):
    return _(text, *args, **kwargs)


def scrub(txt):
    return str(txt).lower().replace(" ", "_").replace("-", "_")


def unscrub(txt):
    return str(txt).replace("_", " ").replace("-", " ").title()


def bold(text):
    return f"<b>{text}</b>"


def parse_json(val):
    if isinstance(val, str):
        try:
            val = _json.loads(val)
        except Exception:
            return val
    if isinstance(val, dict):
        return _dict(val)
    if isinstance(val, list):
        return [parse_json(i) for i in val]
    return val


def as_json(val, indent=1):
    return _json.dumps(val, indent=indent, default=str)


def safe_decode(val, encoding="utf-8"):
    if isinstance(val, bytes):
        return val.decode(encoding)
    return val
