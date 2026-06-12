"""
Frappe shim — drop-in replacement for the real frappe package.

Strategy:
1. Explicit overrides (db, get_doc, local, session, etc.) delegate to the
   Rust kiff_core PyO3 module.
2. Everything else is lazily fetched from the real frappe package (loaded
   via importlib so it doesn't conflict with this shim in sys.modules).
3. __path__ is extended via pkgutil so real frappe submodules resolve
   normally.
"""

import importlib.util
import os
import pkgutil
import sys

# Allow real framework submodules to be imported alongside this shim.
__path__ = pkgutil.extend_path(__path__, __name__)

# ------------------------------------------------------------------
# Lazy-load the real frappe top-level module
# ------------------------------------------------------------------
_real_frappe = None
_loading_real = False


def _ensure_real_frappe():
    global _real_frappe, _loading_real
    if _real_frappe is not None or _loading_real:
        return

    _loading_real = True
    try:
        for p in sys.path:
            init_file = os.path.join(p, "frappe", "__init__.py")
            if os.path.isfile(init_file) and "python/frappe" not in init_file:
                spec = importlib.util.spec_from_file_location(
                    "_real_frappe",
                    init_file,
                    submodule_search_locations=[os.path.join(p, "frappe")],
                )
                mod = importlib.util.module_from_spec(spec)
                sys.modules["_real_frappe"] = mod
                spec.loader.exec_module(mod)
                _real_frappe = mod
                _patch_modules()
                return
    except Exception:
        import traceback
        traceback.print_exc()
    finally:
        _loading_real = False


# ------------------------------------------------------------------
# kiff_core bridge
# ------------------------------------------------------------------
try:
    import kiff_core as _rust
except ImportError:
    _rust = None

# ------------------------------------------------------------------
# Exceptions
# ------------------------------------------------------------------
from .exceptions import (
    ValidationError,
    DoesNotExistError,
    PermissionError,
    DuplicateEntryError,
)

# ------------------------------------------------------------------
# Submodule imports (re-exported as part of the frappe namespace)
# ------------------------------------------------------------------
from ._types import _dict, _DocProxy, _make_doc_proxy, _MetaProxy
from ._utils import (
    flt, cint, cstr, fmt_money,
    nowdate, now_datetime, now, today,
    getdate, get_datetime, add_days, date_diff,
    _, scrub, unscrub, bold,
    parse_json, as_json, safe_decode,
)
from ._context import (
    _local, _session,
    _LocalProxy, _SessionProxy,
    local, session,
    conf, response,
    _build_module_app, _set_context,
)
from ._db import _sqlite_query, _Database, db
from ._meta import _doctype_json_cache, _load_doctype_json, get_meta
from ._document import (
    get_doc, get_list, get_all, get_value,
    new_doc, set_value, save_doc, insert_doc, delete_doc,
)
from ._permissions import (
    get_roles, has_permission,
    _SimpleUserPermissions, get_user,
)
from ._messaging import throw, msgprint, log_error, enqueue, publish_realtime
from ._misc import (
    _Cache, cache, clear_cache,
    whitelist, whitelisted, guest_methods, xss_safe_methods,
    _system_settings_cache, get_system_settings, clear_last_message,
    get_active_domains, get_installed_apps, get_all_apps,
    get_app_path, get_site_path, get_conf,
    set_user, get_module_path, get_pymodule_path, get_doctype_module,
    request_cache,
    get_hooks, format_value, get_module, get_attr, copy_doc, get_cached_doc,
)

# ------------------------------------------------------------------
# Query builder (frappe.qb)
# ------------------------------------------------------------------
try:
    from frappe.query_builder.builder import MariaDB as _MariaDB
    qb = _MariaDB
    try:
        from frappe.query_builder.utils import patch_query_execute, patch_query_aggregation
        patch_query_execute()
        patch_query_aggregation()
    except Exception:
        pass
except Exception:
    qb = None

# ------------------------------------------------------------------
# Module-level shortcuts (werkzeug LocalProxy shadow)
# These reference mutable objects inside _local so dict contents stay live.
# form_dict is also reset in _set_request_context below.
# ------------------------------------------------------------------
form_dict = form = _local["form_dict"]
flags = _local["flags"]
lang = "en"
request = None
job = None
error_log = _local["error_log"]
debug_log = _local["debug_log"]
message_log = _local["message_log"]
user = None

# ------------------------------------------------------------------
# Attributes we explicitly override — never delegate to real frappe.
# ------------------------------------------------------------------
_SHIM_OVERRIDES = {
    "__name__", "__doc__", "__package__", "__loader__", "__spec__",
    "__file__", "__cached__", "__builtins__", "__path__",
    "_real_frappe", "_loading_real", "_ensure_real_frappe",
    "_SHIM_OVERRIDES", "_make_stub", "_LocalProxy", "_SessionProxy",
    "_local", "_session", "_system_settings_cache", "_rust",
    "local", "session", "db", "conf", "response", "form_dict",
    "get_list", "get_all", "get_value", "set_value", "new_doc",
    "delete", "save_doc", "insert_doc", "db_sql", "db_set_values",
    "db_exists", "db_count", "get_roles", "has_permission",
    "enqueue", "publish_realtime", "whitelist", "whitelisted",
    "guest_methods", "xss_safe_methods", "throw", "msgprint",
    "log_error", "cache", "clear_cache", "get_system_settings",
    "get_hooks", "get_cached_doc", "get_meta", "copy_doc",
    "get_attr", "get_module", "format_value", "get_app_path",
    "get_site_path", "get_conf", "set_user", "get_user",
    "get_installed_apps", "get_all_apps", "get_active_domains",
    "get_module_path", "get_pymodule_path", "get_doctype_module",
    "request_cache", "_set_context", "_set_request_context",
    "_dict", "flt", "cint", "cstr", "fmt_money",
    "nowdate", "now_datetime", "now", "today", "getdate", "get_datetime",
    "add_days", "date_diff", "_", "scrub", "unscrub", "bold",
    "parse_json", "as_json", "safe_decode", "clear_last_message",
    "flags", "qb", "lang", "request", "job", "form",
    "error_log", "debug_log", "message_log", "user",
}


def _make_stub(name):
    """Return a do-nothing stub that satisfies decorator and callable patterns."""

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


# ------------------------------------------------------------------
# Request context reset (updates module-level names that can be rebound)
# ------------------------------------------------------------------
def _set_request_context(kwargs_dict, user="Guest"):
    """Called by the Rust bridge before each Python method dispatch."""
    global form_dict, form
    _local["form_dict"] = kwargs_dict
    form_dict = form = kwargs_dict
    _session["user"] = user
    sys.modules['frappe'].user = user
    _session["data"] = {
        "user_type": "System User",
        "csrf_token": "",
        "full_name": user,
        "ipinfo": None,
    }
    _local["user_perms"] = None
    _local["flags"] = _dict()
    _local["error_log"] = []
    _local["debug_log"] = []
    _local["message_log"] = []
    _local["permission_debug_log"] = []
    _local["role_permissions"] = {}
    _local["valid_columns"] = {}
    response.clear()
    response["docs"] = []


# ------------------------------------------------------------------
# Monkey-patches applied after real frappe loads
# ------------------------------------------------------------------
def _patch_modules():
    # Patch critical shim objects onto real frappe so code running inside
    # real frappe modules doesn't hit uninitialized (None) attributes.
    if _real_frappe is not None:
        for attr in ("cache", "db", "conf", "response", "local", "session", "flags"):
            try:
                if globals().get(attr) is not None:
                    setattr(_real_frappe, attr, globals()[attr])
            except Exception:
                pass

    try:
        import frappe.desk.listview as _listview
        if not hasattr(_listview, "get_list_view_counts"):
            def get_list_view_counts(doctype):
                return {}
            _listview.get_list_view_counts = get_list_view_counts
    except Exception:
        pass

    try:
        import frappe.desk.desk_page as _desk_page
        _orig_get = _desk_page.get

        def _patched_get(name):
            if name in ("Workspaces", "desk"):
                return _dict(
                    name=name,
                    title=name,
                    module="Desk",
                    standard="Yes",
                    page_name=name.lower(),
                    roles=[],
                )
            return _orig_get(name)

        _desk_page.get = _patched_get
    except Exception:
        pass


# ------------------------------------------------------------------
# Catch-all: delegate to real frappe, then fall back to stub
# ------------------------------------------------------------------
def __getattr__(name):
    if name in _SHIM_OVERRIDES:
        raise AttributeError(name)

    if not _loading_real:
        _ensure_real_frappe()
        if _real_frappe is not None and hasattr(_real_frappe, name):
            return getattr(_real_frappe, name)

    return _make_stub(name)
