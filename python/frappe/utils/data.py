"""Shim for frappe.utils.data.

Try to use the real frappe.utils.data implementation where possible, and
fall back to lightweight stubs for functions that pull in heavy dependencies.
"""

import importlib.util
import os
import sys

from frappe._utils import (
    cint,
    cstr,
    flt,
    fmt_money,
    nowdate,
    now_datetime,
    now,
    today,
    getdate,
    get_datetime,
    add_days,
    date_diff,
    parse_json,
    as_json,
    safe_decode,
    scrub,
    unscrub,
    _,
)


def _load_real_data():
    """Return the real frappe.utils.data module if it can be imported."""
    shim_dir = os.path.dirname(os.path.abspath(__file__))
    for p in sys.path:
        candidate = os.path.join(os.path.abspath(p), "frappe", "utils", "data.py")
        if os.path.abspath(os.path.dirname(candidate)) == shim_dir:
            continue
        if os.path.isfile(candidate):
            spec = importlib.util.spec_from_file_location("_frappe_utils_data_real", candidate)
            mod = importlib.util.module_from_spec(spec)
            try:
                spec.loader.exec_module(mod)
                return mod
            except Exception:
                pass
    return None


_real_data = _load_real_data()
if _real_data is not None:
    # Re-export everything from the real module, shadowing with our stubs
    # only when we need to avoid heavy dependencies.
    for _name in dir(_real_data):
        if not _name.startswith("_") and _name not in globals():
            globals()[_name] = getattr(_real_data, _name)


# Lightweight fallbacks for functions that may not exist or that we want to stub.
def format_date(value, format_string=None):
    if value is None:
        return ""
    return str(value)


def format_time(value, format_string=None):
    if value is None:
        return ""
    return str(value)


def format_datetime(value, format_string=None):
    if value is None:
        return ""
    return str(value)


def get_url(uri=None, **kwargs):
    return uri or "/"


def get_fullname(user=None):
    return user or "Guest"


def get_time_zone():
    return "UTC"


def has_common(lst1, lst2):
    return bool(set(lst1 or []) & set(lst2 or []))


def unique(seq):
    seen = set()
    out = []
    for item in seq:
        if item not in seen:
            seen.add(item)
            out.append(item)
    return out


def strip_html(text):
    import re
    if text is None:
        return ""
    return re.sub(r"<[^>]+>", "", str(text))


def comma_sep(seq):
    return ", ".join(str(x) for x in seq)


def cast(fieldtype, value):
    return value


def get_user_info_for_avatar(user_id):
    return {"name": user_id, "image": "", "full_name": user_id}


# Module-level fallback for any other frappe.utils.data names.
def __getattr__(name):
    if name.startswith("_"):
        raise AttributeError(name)
    return _make_stub(name)


def _make_stub(name):
    def _stub(*args, **kwargs):
        if len(args) == 1 and callable(args[0]) and not kwargs:
            return args[0]

        def _decorator(fn_or_val=None):
            if callable(fn_or_val):
                return fn_or_val
            return None

        return _decorator

    _stub.__name__ = name
    return _stub
