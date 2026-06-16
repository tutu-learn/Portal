"""Shim for frappe.utils.

Try to use the real frappe.utils package where possible, and fall back to
lightweight stubs for functions that pull in heavy dependencies.
"""

import importlib.util
import os
import pkgutil
import sys

# Fall back to real frappe.utils submodules for anything not shimmed here.
__path__ = pkgutil.extend_path(__path__, __name__)

from frappe._utils import (
    nowdate,
    now_datetime,
    now,
    today,
    getdate,
    get_datetime,
    add_days,
    date_diff,
    cint,
    cstr,
    flt,
    fmt_money,
    parse_json,
    as_json,
    safe_decode,
    scrub,
    unscrub,
    _,
)


def _load_real_utils():
    """Return the real frappe.utils module if it can be imported."""
    shim_dir = os.path.dirname(os.path.abspath(__file__))
    for p in sys.path:
        candidate = os.path.join(os.path.abspath(p), "frappe", "utils", "__init__.py")
        if os.path.abspath(os.path.dirname(candidate)) == shim_dir:
            continue
        if os.path.isfile(candidate):
            spec = importlib.util.spec_from_file_location(
                "_frappe_utils_real",
                candidate,
                submodule_search_locations=[os.path.dirname(candidate)],
            )
            mod = importlib.util.module_from_spec(spec)
            try:
                spec.loader.exec_module(mod)
                return mod
            except Exception:
                pass
    return None


_real_utils = _load_real_utils()
if _real_utils is not None:
    for _name in dir(_real_utils):
        if not _name.startswith("_") and _name not in globals():
            globals()[_name] = getattr(_real_utils, _name)


def get_system_timezone():
    return "UTC"


def add_user_info(user, user_info=None):
    """Populate user_info dict for bootinfo / docinfo.

    Mirrors real Frappe's semantics enough for the desk to resolve user
    full names and avatars. Accepts a single user or an iterable of users.
    """
    if user_info is None:
        user_info = {}

    users = user
    if isinstance(user, str):
        users = [user]
    elif not isinstance(user, (list, tuple, set)):
        users = [user]

    import frappe
    for u in users:
        if u in user_info:
            continue
        full_name = u
        image = None
        try:
            row = frappe.db.get_value(
                "User",
                u,
                ["full_name", "user_image"],
                as_dict=True,
            )
            if row:
                full_name = row.get("full_name") or u
                image = row.get("user_image")
        except Exception:
            pass

        user_info[u] = frappe._dict(
            email=u,
            fullname=full_name,
            image=image,
            name=u,
            time_zone=None,
        )
    return user_info


def get_table_name(table_name: str, wrap_in_backticks: bool = False) -> str:
    """Return the SQL table name for a DocType, matching real Frappe's API.

    In the kiff runtime data tables are stored without the ``tab`` prefix and
    lower-cased, so the translation layer in :func:`frappe.db.sql` handles the
    conversion.  This function keeps the ``tab`` prefix so existing Frappe code
    that builds raw SQL continues to work.
    """
    name = f"tab{table_name}" if not table_name.startswith("__") else table_name
    if wrap_in_backticks:
        return f"`{name}`"
    return name


def get_datetime(dt=None):
    import datetime
    if dt is None:
        return datetime.datetime.now()
    if isinstance(dt, datetime.datetime):
        return dt
    if isinstance(dt, datetime.date):
        return datetime.datetime(dt.year, dt.month, dt.day)
    try:
        return datetime.datetime.fromisoformat(str(dt))
    except Exception:
        return datetime.datetime.now()


def get_traceback():
    import traceback
    return traceback.format_exc()


def create_folder(path, with_init=False):
    import os
    os.makedirs(path, exist_ok=True)


def mock(*args, **kwargs):
    pass


def safe_eval(*args, **kwargs):
    pass


# Module-level fallback for any other frappe.utils names.
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
