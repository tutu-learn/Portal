"""Miscellaneous stubs, cache, whitelist, and path helpers."""

import copy
import importlib
import os
import sys

from ._types import _dict, _DocProxy
from .exceptions import DoesNotExistError


# ------------------------------------------------------------------
# Cache
# ------------------------------------------------------------------
class _Cache:
    def get_value(self, key, generator=None, user=None, expires_in_sec=None):
        if generator is not None:
            return generator()
        return None

    def set_value(self, key, val, user=None, expires_in_sec=None):
        pass

    def delete_value(self, key):
        pass

    def hget(self, key, field, generator=None):
        if generator is not None:
            return generator()
        return None

    def hset(self, key, field, val, expires_in_sec=None):
        pass

    def __call__(self):
        return self


cache = _Cache()


def clear_cache(user=None, doctype=None):
    pass


# ------------------------------------------------------------------
# Whitelist
# ------------------------------------------------------------------
whitelisted = []
guest_methods = []
xss_safe_methods = []


def whitelist(allow_guest=False, xss_safe=False, methods=None):
    def innerfn(fn):
        whitelisted.append(fn)
        if allow_guest:
            guest_methods.append(fn)
        return fn

    if callable(allow_guest):
        fn = allow_guest
        allow_guest = False
        return innerfn(fn)
    return innerfn


# ------------------------------------------------------------------
# System settings
# ------------------------------------------------------------------
_system_settings_cache = {}


def get_system_settings(key):
    return _system_settings_cache.get(key)


def clear_last_message():
    pass


# ------------------------------------------------------------------
# App / site path helpers
# ------------------------------------------------------------------
def get_active_domains():
    return []


def get_installed_apps(sort=False):
    return ["frappe"]


def get_all_apps():
    return ["frappe"]


def get_app_path(app_name):
    for p in sys.path:
        candidate = os.path.join(p, app_name)
        if os.path.isdir(candidate):
            return candidate
    return ""


def get_site_path(*args):
    return os.path.join("sites", "localhost", *args)


def get_conf(site=None):
    return {}


def set_user(user):
    from ._context import _session
    _session["user"] = user


def get_module_path(module, *joins):
    from ._utils import scrub
    base = scrub(module)
    return os.path.join("apps", "frappe", "frappe", base, *joins)


def get_pymodule_path(modulename, *joins):
    try:
        mod = importlib.import_module(modulename)
        mod_file = getattr(mod, "__file__", None)
        if mod_file:
            return os.path.join(os.path.dirname(mod_file), *joins)
    except Exception:
        pass
    return os.path.join(*joins) if joins else ""


def get_doctype_module(doctype):
    from ._utils import scrub
    return scrub(doctype)


def request_cache(fn):
    return fn


# ------------------------------------------------------------------
# Misc frappe API stubs
# ------------------------------------------------------------------
def get_hooks(hook=None, default=None, app_name=None):
    if hook is None:
        return {}
    return default if default is not None else []


def format_value(value, df=None, doc=None, currency=None, format_string=None):
    return str(value) if value is not None else ""


def get_module(modulename):
    return importlib.import_module(modulename)


def get_attr(method_string):
    parts = method_string.split(".")
    module = importlib.import_module(".".join(parts[:-1]))
    return getattr(module, parts[-1])


def copy_doc(doc, ignore_no_copy=True):
    return copy.deepcopy(doc)


def get_cached_doc(doctype, name=None):
    from ._document import get_doc
    try:
        return get_doc(doctype, name)
    except DoesNotExistError:
        proxy = _DocProxy({"doctype": doctype, "name": name or ""})
        return proxy
