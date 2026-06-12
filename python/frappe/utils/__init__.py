"""
Frappe utils shim — extends __path__ and eagerly re-exports the real
frappe.utils so that `from frappe.utils import format_timedelta` works.

By the time this module is imported, frappe/__init__ is fully initialized
(since we removed the `from .utils import ...` startup import), so loading
the real frappe utils here is safe.
"""
import importlib.util
import os
import pkgutil
import sys

# Extend __path__ so submodules (deprecations, password, etc.) resolve to apps/frappe.
__path__ = pkgutil.extend_path(__path__, __name__)
_shim_utils_dir = os.path.dirname(os.path.abspath(__file__))
for _p in sys.path:
    _candidate = os.path.join(os.path.abspath(_p), "frappe", "utils")
    if os.path.isdir(_candidate) and os.path.abspath(_candidate) not in [os.path.abspath(x) for x in __path__]:
        __path__.append(_candidate)

# ---------------------------------------------------------------------------
# Eagerly load the real frappe/utils/__init__.py and merge its namespace.
# ---------------------------------------------------------------------------
def _load_real():
    for _p in list(__path__):
        if os.path.abspath(_p) == _shim_utils_dir:
            continue
        _init = os.path.join(os.path.abspath(_p), "__init__.py")
        if os.path.isfile(_init):
            spec = importlib.util.spec_from_file_location("_frappe_utils_real_init", _init)
            mod = importlib.util.module_from_spec(spec)
            try:
                spec.loader.exec_module(mod)
                globals().update({k: v for k, v in vars(mod).items() if not k.startswith("__")})
                return
            except Exception:
                import traceback
                traceback.print_exc()

_load_real()

# Inline stubs as fallback for anything not provided by the real module.
import datetime as _dt

def _flt(v, precision=None):
    if v is None:
        return 0.0
    try:
        result = float(v)
        return round(result, int(precision)) if precision is not None else result
    except (TypeError, ValueError):
        return 0.0

def _cint(v, default=0):
    if v is None:
        return default
    try:
        return int(float(v))
    except (TypeError, ValueError):
        return default

# Only define stubs if the real module didn't provide them
if "flt" not in globals():
    flt = _flt
if "cint" not in globals():
    cint = _cint
if "now_datetime" not in globals():
    def now_datetime():
        return _dt.datetime.now()
if "nowdate" not in globals():
    def nowdate():
        return _dt.date.today().isoformat()
if "today" not in globals():
    def today():
        return _dt.date.today().isoformat()
