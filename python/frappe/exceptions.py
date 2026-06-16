"""
Shim for frappe.exceptions — imports from the real frappe exceptions module.
Defines minimal fallbacks if the real module is unavailable.
"""
import importlib.util
import os
import sys

# Core exceptions needed at shim init time (defined inline to bootstrap)
class ValidationError(Exception):
    pass

class DoesNotExistError(ValidationError):
    pass

class PermissionError(Exception):
    pass

class DuplicateEntryError(Exception):
    pass

class AuthenticationError(Exception):
    pass

class SessionExpired(Exception):
    pass


# Database-level exceptions used by real Frappe code.
class SQLError(Exception):
    pass


class OperationalError(SQLError):
    pass


class ProgrammingError(SQLError):
    pass


class InternalError(SQLError):
    pass


class DataError(SQLError):
    pass


class TableMissingError(SQLError):
    pass

# Eagerly load the real frappe exceptions and merge them in.
def _load_real():
    _shim_dir = os.path.dirname(os.path.abspath(__file__))
    for _p in sys.path:
        _candidate = os.path.join(os.path.abspath(_p), "frappe", "exceptions.py")
        if os.path.abspath(os.path.dirname(_candidate)) == _shim_dir:
            continue
        if os.path.isfile(_candidate):
            spec = importlib.util.spec_from_file_location("_frappe_exceptions_real", _candidate)
            mod = importlib.util.module_from_spec(spec)
            try:
                spec.loader.exec_module(mod)
                globals().update({k: v for k, v in vars(mod).items() if not k.startswith("__")})
                return
            except Exception:
                pass

_load_real()
