"""Request-local context state: _local, _session, proxies, conf, response."""

import os

from collections import defaultdict
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
    "request_cache": defaultdict(dict),
    "jenv_restricted": None,
    "jenv_unrestricted": None,
    "request": None,
}


# ------------------------------------------------------------------
# Request stub
# ------------------------------------------------------------------
class _AfterResponse:
    """Minimal stand-in for Werkzeug request.after_response callbacks."""

    def __init__(self):
        self._callbacks = []

    def add(self, callback):
        self._callbacks.append(callback)

    def __call__(self, callback):
        self._callbacks.append(callback)
        return callback


class _RequestProxy:
    """Lightweight Werkzeug/Flask request stand-in for the shim runtime."""

    def __init__(self, method="GET", path="/", headers=None, cookies=None,
                 query=None, body=None, request_ip=None, scheme="http", host=None):
        self.method = method.upper()
        self.path = path
        self.headers = _dict(headers or {})
        self.cookies = _dict(cookies or {})
        self.args = _dict(query or {})
        self.form = _dict()
        self.files = _dict()
        self._body = body or b""
        self.environ = {}
        self.scheme = scheme
        self.host = host or "localhost"
        self.url = f"{scheme}://{self.host}{path}"
        self.base_url = f"{scheme}://{self.host}"
        self.remote_addr = request_ip
        self.after_response = _AfterResponse()

    def get_data(self, cache=True, as_text=False, parse_form_data=False):
        data = self._body
        if as_text and isinstance(data, bytes):
            data = data.decode("utf-8", "replace")
        return data

    def get_json(self, force=False, silent=False):
        import json
        try:
            return json.loads(self._body.decode("utf-8", "replace"))
        except Exception:
            if silent:
                return None
            raise


# ------------------------------------------------------------------
# Proxies
# ------------------------------------------------------------------
class _LocalProxy:
    """Dict-backed proxy so any attribute can be get/set on frappe.local."""

    __slots__ = ("_store", "_key")

    def __init__(self, store, key=None):
        object.__setattr__(self, "_store", store)
        object.__setattr__(self, "_key", key)

    def _resolve(self):
        store = object.__getattribute__(self, "_store")
        key = object.__getattribute__(self, "_key")
        if key is None:
            return store
        return store[key]

    def __getattr__(self, name):
        resolved = self._resolve()
        # First try attribute access (for methods like .pop, .keys, etc.).
        try:
            return getattr(resolved, name)
        except AttributeError:
            pass
        # Then try item access (keys exposed as attributes, e.g. frappe.flags.in_install).
        try:
            return resolved[name]
        except (KeyError, TypeError):
            raise AttributeError(name)

    def __setattr__(self, name, val):
        self._resolve()[name] = val

    def __delattr__(self, name):
        del self._resolve()[name]

    def __contains__(self, name):
        return name in self._resolve()

    def __iter__(self):
        return iter(self._resolve().items())

    def __bool__(self):
        return bool(self._resolve())

    def __len__(self):
        return len(self._resolve())

    def __getitem__(self, key):
        return self._resolve()[key]

    def __setitem__(self, key, val):
        self._resolve()[key] = val

    def get(self, name, default=None):
        return self._resolve().get(name, default)


class _SessionProxy(_dict):
    """Session dict that also exposes common keys as attributes."""

    @property
    def user(self):
        return self.get("user", "Guest")

    @user.setter
    def user(self, value):
        self["user"] = value

    @property
    def data(self):
        if "data" not in self:
            self["data"] = _dict()
        return self["data"]

    @data.setter
    def data(self, value):
        self["data"] = value


local = _LocalProxy(_local)
session = _LocalProxy(_local, "session")

# Backward-compat direct reference kept in sync with _local["session"]
_session = _local["session"]


# Module-level config / response (mutable dicts shared by reference)
conf = _dict(
    developer_mode=True,
    db_type="sqlite",
    db_name="site.db",
)
response = _LocalProxy(_local, "response")

# Keep the local store in sync with module-level objects.
_local["conf"] = conf
_local["db"] = None
_local["qb"] = None


# ------------------------------------------------------------------
# module_app map
# ------------------------------------------------------------------
def _build_module_app() -> dict:
    """Return {scrubbed_module: app_name} from every modules.txt on disk."""
    result: dict = {}
    project_root = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
    apps_root = os.path.join(project_root, "apps")
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
def release_local(local_proxy):
    """Clear a local proxy store (Werkzeug-compatible)."""
    if isinstance(local_proxy, _LocalProxy):
        store = object.__getattribute__(local_proxy, "_store")
        key = object.__getattribute__(local_proxy, "_key")
        if key is None:
            store.clear()
        else:
            store[key] = _dict()


def _set_context(site, user="Guest"):
    _local["site"] = site
    _local["flags"] = _dict()
    _local["form_dict"] = _dict()
    _local["conf"] = conf
    _local["session"] = _SessionProxy(user=user, data=_dict())
    _local["request"] = None
