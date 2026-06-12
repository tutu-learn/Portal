"""Request-local context state: _local, _session, proxies, conf, response."""

import os

from ._types import _dict


# ------------------------------------------------------------------
# Context stores
# ------------------------------------------------------------------
_local = {
    "flags": _dict(),
    "site": "localhost",
    "sites_path": "sites",
    "site_path": "sites/localhost",
    "form_dict": _dict(),
    "dev_server": int(os.environ.get("DEV_SERVER", "0")),
    "initialised": True,
    "valid_columns": {},
    "role_permissions": {},
    "new_doc_templates": {},
    "module_app": {},
    "app_modules": {},
    "user": None,
    "user_perms": None,
    "session": _dict(),
    "lang": "en",
    "error_log": [],
    "debug_log": [],
    "message_log": [],
    "permission_debug_log": [],
    "response": _dict(docs=[]),
    "preload_assets": {"style": [], "script": [], "icons": []},
    "no_cache": 0,
    "jenv": None,
    "jloader": None,
    "cache": {},
    "conf": None,
    "request_ip": None,
    "task_id": None,
    "all_apps": None,
    "locked_documents": [],
    "test_objects": {},
    "primary_db": None,
    "replica_db": None,
    "system_settings": {},
    "website_settings": None,
}

_session = {"user": "Guest", "data": {}}


# ------------------------------------------------------------------
# Proxies
# ------------------------------------------------------------------
class _LocalProxy:
    """Dict-backed proxy so any attribute can be get/set on frappe.local."""

    __slots__ = ("_store",)

    def __init__(self, store):
        object.__setattr__(self, "_store", store)

    def __getattr__(self, name):
        store = object.__getattribute__(self, "_store")
        try:
            return store[name]
        except KeyError:
            raise AttributeError(name)

    def __setattr__(self, name, val):
        object.__getattribute__(self, "_store")[name] = val

    def __delattr__(self, name):
        object.__getattribute__(self, "_store").__delitem__(name)

    def __contains__(self, name):
        return name in object.__getattribute__(self, "_store")

    def __iter__(self):
        return iter(object.__getattribute__(self, "_store"))

    def get(self, name, default=None):
        return object.__getattribute__(self, "_store").get(name, default)


class _SessionProxy:
    def __init__(self, store):
        self._store = store

    @property
    def user(self):
        return self._store["user"]

    @property
    def data(self):
        return _dict(self._store["data"])


local = _LocalProxy(_local)
session = _SessionProxy(_session)

# Module-level config / response (mutable dicts shared by reference)
conf = _dict(developer_mode=True)
response = _dict(docs=[])


# ------------------------------------------------------------------
# module_app map
# ------------------------------------------------------------------
def _build_module_app() -> dict:
    """Return {scrubbed_module: app_name} from every modules.txt on disk."""
    result: dict = {}
    apps_root = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "apps")
    if not os.path.isdir(apps_root):
        return result
    for app_name in os.listdir(apps_root):
        modules_txt = os.path.join(apps_root, app_name, app_name, "modules.txt")
        if not os.path.isfile(modules_txt):
            continue
        with open(modules_txt) as _f:
            for line in _f:
                module = line.strip()
                if module:
                    result[module.lower().replace(" ", "_")] = app_name
    return result


_local["module_app"] = _build_module_app()
_local["app_modules"] = {}


# ------------------------------------------------------------------
# Context reset helpers
# ------------------------------------------------------------------
def _set_context(site, user="Guest"):
    _local["site"] = site
    _local["flags"] = {}
    _local["form_dict"] = {}
    _session["user"] = user
    _session["data"] = {}
