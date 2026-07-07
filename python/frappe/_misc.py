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
    """In-memory cache backing frappe.cache.

    Real Frappe uses Redis; this implementation keeps data in a process-local
    dict. It supports the subset of RedisWrapper operations that bootinfo and
    common desk code actually call.
    """

    def __init__(self):
        self._store = {}
        self._hash = {}
        self._lists = {}
        self._sets = {}

    def _key(self, key, user=None, shared=False):
        if shared:
            return f"__shared:{key}"
        if user:
            return f"__user:{user}:{key}"
        return key

    def get_value(self, key, generator=None, user=None, expires_in_sec=None, shared=False, **kwargs):
        k = self._key(key, user=user, shared=shared)
        if k in self._store:
            return self._store[k]
        if generator is not None:
            val = generator()
            self.set_value(key, val, user=user, expires_in_sec=expires_in_sec, shared=shared)
            return val
        return None

    def get_doc(self, doctype, name=None, *args, **kwargs):
        import frappe
        if name is None:
            name = doctype
        try:
            return frappe.get_doc(doctype, name, *args, **kwargs)
        except Exception:
            # Fallback documents for system types that may not exist in the
            # lightweight SQLite site database.
            if doctype == "Language":
                return _dict(
                    name=name,
                    language_name=name,
                    date_format="yyyy-mm-dd",
                    time_format="HH:mm:ss",
                    number_format="#,###.##",
                )
            if doctype == "Installed Applications":
                return _dict(
                    name="Installed Applications",
                    doctype="Installed Applications",
                    installed_applications=[],
                )
            raise

    def set_value(self, key, val, user=None, expires_in_sec=None, shared=False, **kwargs):
        k = self._key(key, user=user, shared=shared)
        self._store[k] = val

    def delete_value(self, keys, user=None, make_keys=True, shared=False, **kwargs):
        if not keys:
            return
        if isinstance(keys, str):
            keys = [keys]
        for key in keys:
            k = self._key(key, user=user, shared=shared) if make_keys else key
            self._store.pop(k, None)
            self._hash.pop(k, None)
            self._lists.pop(k, None)
            self._sets.pop(k, None)

    def hget(self, key, field, generator=None, shared=False, **kwargs):
        k = self._key(key, shared=shared)
        h = self._hash.get(k, {})
        if field in h:
            return h[field]
        if generator is not None:
            val = generator()
            self.hset(key, field, val, shared=shared)
            return val
        return None

    def hset(self, key, field, val, shared=False, **kwargs):
        k = self._key(key, shared=shared)
        if k not in self._hash:
            self._hash[k] = {}
        self._hash[k][field] = val

    def hdel(self, key, fields, shared=False, **kwargs):
        k = self._key(key, shared=shared)
        if isinstance(fields, str):
            fields = [fields]
        h = self._hash.get(k, {})
        for f in fields:
            h.pop(f, None)

    def hgetall(self, key, shared=False):
        k = self._key(key, shared=shared)
        return self._hash.get(k, {}).copy()

    def hkeys(self, key, shared=False):
        k = self._key(key, shared=shared)
        return list(self._hash.get(k, {}).keys())

    def hexists(self, key, field, shared=False, **kwargs):
        k = self._key(key, shared=shared)
        return field in self._hash.get(k, {})

    def hdel_names(self, names, key, **kwargs):
        for name in names:
            self.hdel(name, key, **kwargs)

    def hdel_keys(self, name_starts_with, key, **kwargs):
        for name in self.get_keys(name_starts_with):
            self.hdel(name, key, **kwargs)

    # Redis-like list helpers used by real Frappe code.
    def rpush(self, key, *values):
        if key not in self._lists:
            self._lists[key] = []
        self._lists[key].extend(values)

    def lpush(self, key, *values):
        if key not in self._lists:
            self._lists[key] = []
        self._lists[key] = list(values) + self._lists[key]

    def lpop(self, key, **kwargs):
        lst = self._lists.get(key, [])
        if lst:
            return lst.pop(0)
        return None

    def blpop(self, key, timeout=0, **kwargs):
        return self.lpop(key)

    def rpop(self, key):
        lst = self._lists.get(key, [])
        if lst:
            return lst.pop()
        return None

    def lrange(self, key, start, end):
        lst = self._lists.get(key, [])
        if end < 0:
            return lst[start:]
        return lst[start : end + 1]

    def lindex(self, key, index):
        lst = self._lists.get(key, [])
        if -len(lst) <= index < len(lst):
            return lst[index]
        return None

    def lrem(self, key, count, value):
        lst = self._lists.get(key, [])
        removed = 0
        out = []
        for item in lst:
            if item == value and (count <= 0 or removed < count):
                removed += 1
                continue
            out.append(item)
        self._lists[key] = out

    def llen(self, key):
        return len(self._lists.get(key, []))

    def ltrim(self, key, start, end):
        lst = self._lists.get(key, [])
        self._lists[key] = lst[start : end + 1] if end >= 0 else lst[start:]

    # Redis-like set helpers.
    def sadd(self, key, *values):
        if key not in self._sets:
            self._sets[key] = set()
        self._sets[key].update(values)

    def smembers(self, key):
        return set(self._sets.get(key, set()))

    def srem(self, key, *values):
        s = self._sets.get(key, set())
        for v in values:
            s.discard(v)

    def sismember(self, key, value):
        return value in self._sets.get(key, set())

    def exists(self, *keys, **kwargs):
        return sum(1 for k in keys if k in self._store)

    def delete(self, key, **kwargs):
        self._store.pop(key, None)
        self._hash.pop(key, None)
        self._lists.pop(key, None)
        self._sets.pop(key, None)

    # Alias used by workspace/report/user/desktop_icon hooks.
    def delete_key(self, *args, **kwargs):
        return self.delete_value(*args, **kwargs)

    def get_keys(self, key, **kwargs):
        return [k for k in self._store if k.startswith(key)]

    def delete_keys(self, key, **kwargs):
        for k in list(self._store):
            if k.startswith(key):
                self._store.pop(k, None)
                self._hash.pop(k, None)
                self._lists.pop(k, None)
                self._sets.pop(k, None)

    def get(self, key):
        return self._store.get(key)

    def get_all(self, key):
        return {k: self.get_value(k) for k in self.get_keys(key)}

    def set(self, key, value, ex=None):
        self._store[key] = value

    def setex(self, key, seconds, value):
        self._store[key] = value

    def expire(self, key, seconds):
        return key in self._store

    def expire_key(self, key, time, *, user=None, shared=False, **kwargs):
        return True

    def ttl(self, key):
        return -1

    def incrby(self, key, increment=1):
        self._store[key] = self._store.get(key, 0) + increment
        return self._store[key]

    def make_key(self, key, user=None, shared=False):
        return self._key(key, user=user, shared=shared)

    def flushall(self):
        self._store.clear()
        self._hash.clear()
        self._lists.clear()
        self._sets.clear()

    def execute_command(self, *args, **kwargs):
        return {}

    def connected(self):
        return True

    def __call__(self):
        return self


class _ClientCache:
    """Lightweight client-side cache stub for frappe.client_cache."""

    def __init__(self):
        self._store = {}
        self.hits = 0
        self.misses = 0
        self.healthy = True

    def get_value(self, key, generator=None, **kwargs):
        if key in self._store:
            self.hits += 1
            return self._store[key]
        self.misses += 1
        if generator is not None:
            val = generator()
            self._store[key] = val
            return val
        return None

    def set_value(self, key, val, **kwargs):
        self._store[key] = val

    def delete_value(self, key, **kwargs):
        self._store.pop(key, None)

    def delete_keys(self, key, **kwargs):
        for k in list(self._store):
            if k.startswith(key):
                self._store.pop(k, None)

    def hget(self, key, field, generator=None, **kwargs):
        h = self._store.setdefault(key, {})
        if field in h:
            self.hits += 1
            return h[field]
        self.misses += 1
        if generator is not None:
            val = generator()
            h[field] = val
            return val
        return None

    def hset(self, key, field, val, **kwargs):
        self._store.setdefault(key, {})[field] = val

    def get_doc(self, doctype, name=None, **kwargs):
        import frappe
        if name is None:
            name = doctype
        try:
            return frappe.get_doc(doctype, name, **kwargs)
        except Exception:
            if doctype == "Language":
                return _dict(
                    name=name,
                    language_name=name,
                    date_format="yyyy-mm-dd",
                    time_format="HH:mm:ss",
                    number_format="#,###.##",
                )
            if doctype == "Installed Applications":
                return _dict(
                    name="Installed Applications",
                    doctype="Installed Applications",
                    installed_applications=[],
                )
            raise

    def clear_cache(self):
        self._store.clear()

    def erase_persistent_caches(self, *, doctype=None):
        self.clear_cache()

    @property
    def statistics(self):
        from collections import namedtuple
        CacheStatistics = namedtuple(
            "CacheStatistics",
            ["hits", "misses", "capacity", "used", "utilization", "hit_ratio", "healthy"],
        )
        total = self.hits + self.misses
        return CacheStatistics(
            hits=self.hits,
            misses=self.misses,
            capacity=0,
            used=len(self._store),
            utilization=0.0,
            hit_ratio=round(self.hits / total, 2) if total else None,
            healthy=self.healthy,
        )

    def reset_statistics(self):
        self.hits = self.misses = 0


cache = _Cache()
client_cache = _ClientCache()


def clear_cache(user=None, doctype=None):
    global cache, client_cache
    cache = _Cache()
    client_cache = _ClientCache()


# ------------------------------------------------------------------
# Whitelist
# ------------------------------------------------------------------
whitelisted = []
guest_methods = []
xss_safe_methods = []


def whitelist(allow_guest=False, xss_safe=False, methods=None):
    def innerfn(fn):
        fn.whitelisted = True
        fn.allow_guest = bool(allow_guest)
        fn.xss_safe = bool(xss_safe)
        # Keep list-based bookkeeping for compatibility with code that may
        # still import these module-level lists.
        whitelisted.append(fn)
        if allow_guest:
            guest_methods.append(fn)
        if xss_safe:
            xss_safe_methods.append(fn)
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


def _read_apps_txt():
    """Read the list of installed apps from sites/apps.txt."""
    project_root = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
    apps_txt = os.path.join(project_root, "sites", "apps.txt")
    if os.path.isfile(apps_txt):
        with open(apps_txt) as f:
            return [line.strip() for line in f if line.strip() and not line.strip().startswith("#")]
    return ["frappe"]


def get_installed_apps(sort=False, _ensure_on_bench=False):
    apps = _read_apps_txt()
    if sort:
        apps = sorted(apps)
    return apps


def get_all_apps():
    return _read_apps_txt()


def get_app_path(app_name, *joins):
    for p in sys.path:
        candidate = os.path.join(p, app_name, *joins)
        if os.path.isdir(candidate) or joins:
            return candidate
        candidate = os.path.join(p, app_name)
        if os.path.isdir(candidate):
            return candidate
    return ""


def get_site_path(*args):
    return os.path.join("sites", "localhost", *args)


def get_conf(site=None):
    return {}


def set_user(user):
    from ._context import _local
    _local["session"]["user"] = user


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
_hooks_cache = None


def _load_app_hooks(app_name=None):
    """Load and merge hooks.py from every installed app."""
    global _hooks_cache
    if _hooks_cache is not None and app_name is None:
        return _hooks_cache

    hooks = _dict()
    project_root = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

    for app in _read_apps_txt():
        if app_name and app != app_name:
            continue
        hooks_path = os.path.join(project_root, "apps", app, app, "hooks.py")
        if not os.path.isfile(hooks_path):
            continue
        try:
            spec = importlib.util.spec_from_file_location(f"{app}.hooks", hooks_path)
            mod = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(mod)
        except Exception:
            continue

        for key, value in vars(mod).items():
            if key.startswith("_"):
                continue
            _append_hook(hooks, key, value)

    _hooks_cache = hooks
    return hooks


def _append_hook(target, key, value):
    """Merge a single hook value the same way real Frappe does.

    Dict-valued hooks (e.g. ``jinja``, ``doc_events``) are merged recursively
    by inner key; everything else becomes a list.
    """
    if isinstance(value, dict):
        target.setdefault(key, _dict())
        for inkey in value:
            _append_hook(target[key], inkey, value[inkey])
    else:
        if not isinstance(value, list):
            value = [value]
        target.setdefault(key, []).extend(value)


def get_hooks(hook=None, default=None, app_name=None):
    """Return merged hooks from all installed apps.

    If ``hook`` is None, returns a dict of all hooks.  If ``hook`` is given,
    returns the list of values for that hook (or ``default`` / ``[]``).
    """
    hooks = _load_app_hooks(app_name=app_name)
    if hook is None:
        return hooks
    return hooks.get(hook, default if default is not None else [])


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
        if doctype == "Notification Settings":
            return _DocProxy({
                "doctype": "Notification Settings",
                "name": name or "",
                "enabled": True,
                "enable_email_notifications": True,
                "enable_email_mention": True,
                "enable_email_assignment": True,
                "enable_email_threads_on_assigned_document": True,
                "enable_email_share": True,
                "enable_email_event_reminders": True,
            })
        proxy = _DocProxy({"doctype": doctype, "name": name or ""})
        return proxy
