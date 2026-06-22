"""Request-local context state: _local, _session, proxies, conf, response."""

import hashlib
import json
import os
import secrets
import uuid

from collections import defaultdict
from pathlib import Path
from ._types import _dict


# ------------------------------------------------------------------
# Context stores
# ------------------------------------------------------------------
_local = {
    "flags": _dict(),
    "site": "localhost",
    "sites_path": "sites",
    "site_path": "sites/localhost",
    "site_name": "localhost",
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
    "response_headers": {},
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

    @property
    def sid(self):
        return self.get("sid", "")

    @sid.setter
    def sid(self, value):
        self["sid"] = value


def _generate_sid() -> str:
    """Return a session id similar to real Frappe (hex of random bytes)."""
    return secrets.token_hex(16)


def _generate_hash(txt: str = None, length: int = 56) -> str:
    """Return a short random/hash string like Frappe's generate_hash."""
    if txt:
        return hashlib.sha224(txt.encode("utf-8")).hexdigest()[:length]
    return secrets.token_hex(length)[:length]


class _SessionObj:
    """Minimal stand-in for frappe.sessions.Session.

    Real Frappe's LoginManager.make_session() creates a Session object and
    stores it in frappe.local.session_obj. Code accesses .user, .data, .sid
    and calls .update(force=True).
    """

    def __init__(self, user="Guest", sid=None):
        self.user = user
        self.sid = sid or _generate_sid()
        self.data = _dict(
            user=user,
            session_ip=_local.get("request_ip") or "127.0.0.1",
            session_country=None,
            session_expiry=None,
            session_start=True,
            csrf_token="",
            audit_user=None,
            impersonated_by=None,
        )

    def update(self, force=False):
        """No-op refresh; the Rust runtime owns session persistence."""
        pass

    def set_impersonated(self, user):
        self.data.impersonated_by = user


class _CookieManager:
    """Minimal stand-in for frappe.auth.CookieManager.

    Real Frappe's auth flow calls init_cookies, set_cookie, delete_cookie and
    flush_cookies. The shim keeps cookies in memory so the Rust HTTP layer can
    read them from frappe.response.cookies after the Python call.
    """

    def __init__(self):
        self.cookies = _dict()
        self.to_delete = []

    def init_cookies(self):
        self.cookies = _dict()
        self.to_delete = []

    def set_cookie(self, key, value, expires=None, secure=False, httponly=False, samesite="Lax"):
        self.cookies[key] = _dict(
            value=value,
            expires=expires,
            secure=secure,
            httponly=httponly,
            samesite=samesite,
        )

    def delete_cookie(self, key):
        if key not in self.to_delete:
            self.to_delete.append(key)
        self.cookies.pop(key, None)

    def flush_cookies(self, response=None):
        response = response or _local.get("response")
        if not isinstance(response, _dict):
            return
        response["cookies"] = response.get("cookies", _dict())
        for key, val in self.cookies.items():
            response["cookies"][key] = val
        for key in self.to_delete:
            response["cookies"][key] = _dict(delete=1)


class _LoginManager:
    """Minimal stand-in for frappe.auth.LoginManager.

    Real Frappe's OAuth, setup-wizard and password flows call login_as,
    logout, check_password, impersonate, post_login, fail, run_trigger and
    clear_cookies. The shim provides safe no-op/stub versions so those flows
    can complete; the Rust HTTP layer handles real session/cookie persistence.
    """

    def __init__(self, user="Guest"):
        self.user = user
        self.info = None
        self.full_name = None
        self.user_type = "System User"
        self.resume = False

    def login_as(self, user, session_end=None, audit_user=None):
        self.user = user
        _local["user"] = user
        session = _local.get("session")
        if isinstance(session, _SessionProxy):
            session.user = user
        session_obj = _local.get("session_obj")
        if isinstance(session_obj, _SessionObj):
            session_obj.user = user
            session_obj.data.user = user
        response = _local.get("response")
        if isinstance(response, _dict):
            response["message"] = "Logged In"

    def login_as_guest(self):
        self.login_as("Guest")

    def logout(self, arg="", user=None):
        self.login_as_guest()
        cookie_manager = _local.get("cookie_manager")
        if isinstance(cookie_manager, _CookieManager):
            cookie_manager.delete_cookie("sid")
            cookie_manager.delete_cookie("user_id")
            cookie_manager.delete_cookie("user_image")

    def check_password(self, user, pwd):
        """No-op verification. The Rust runtime handles password checks."""
        return self

    def authenticate(self, user=None, pwd=None):
        """No-op. Return successfully so callers proceed."""
        if user:
            self.user = user
            _local["user"] = user
        return self

    def post_login(self, session_end=None, audit_user=None):
        """No-op: session creation is handled by the Rust runtime."""
        pass

    def make_session(self, resume=False):
        """Ensure session_obj exists; resume flag is ignored."""
        if not isinstance(_local.get("session_obj"), _SessionObj):
            _local["session_obj"] = _SessionObj(user=self.user)

    def set_user_info(self, resume=False):
        """No-op: user info is resolved on demand."""
        pass

    def impersonate(self, user):
        current_user = _local.get("user")
        self.login_as(user)
        session_obj = _local.get("session_obj")
        if isinstance(session_obj, _SessionObj):
            session_obj.set_impersonated(current_user)

    def fail(self, message, title=None):
        from ._messaging import throw
        throw(message, title=title)

    def run_trigger(self, event):
        """No-op trigger runner."""
        pass

    def clear_cookies(self):
        cookie_manager = _local.get("cookie_manager")
        if isinstance(cookie_manager, _CookieManager):
            cookie_manager.delete_cookie("sid")
            cookie_manager.delete_cookie("user_id")
            cookie_manager.delete_cookie("user_image")


local = _LocalProxy(_local)
session = _LocalProxy(_local, "session")

# Backward-compat direct reference kept in sync with _local["session"]
_session = _local["session"]

# Ensure auth/session stubs are available even outside an explicit request
# context, so code that touches frappe.local.login_manager/cookie_manager
# during import or startup does not crash.
_local["session"] = _SessionProxy(user="Guest", data=_dict())
_local["session_obj"] = _SessionObj()
_local["cookie_manager"] = _CookieManager()
_local["login_manager"] = _LoginManager()


def _load_site_config(site: str = None, sites_path: str = None) -> dict:
    """Load the site's site_config.json, returning an empty dict on failure.

    Order of precedence for encryption_key:
      1. FRAPPE_ENCRYPTION_KEY environment variable.
      2. encryption_key field in the site's site_config.json.
    """
    site = site or _local.get("site") or "localhost"
    sites_path = sites_path or _local.get("sites_path") or "sites"
    config_path = Path(sites_path) / site / "site_config.json"
    try:
        with open(config_path) as f:
            config = json.load(f)
    except Exception:
        config = {}

    env_key = os.environ.get("FRAPPE_ENCRYPTION_KEY", "")
    if env_key:
        config["encryption_key"] = env_key

    return config


# Module-level config / response (mutable dicts shared by reference)
_SITE_CONFIG = _load_site_config()
conf = _dict(
    developer_mode=True,
    db_type="sqlite",
    db_name="site.db",
    **_SITE_CONFIG,
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
    _local["site_name"] = site
    _local["site_path"] = f"sites/{site}"
    _local["flags"] = _dict()
    _local["form_dict"] = _dict()
    _local["conf"] = conf
    sid = _generate_sid()
    _local["session"] = _SessionProxy(
        user=user,
        sid=sid,
        data=_dict(
            user=user,
            session_ip=_local.get("request_ip") or "127.0.0.1",
            csrf_token="",
            sid=sid,
        ),
    )
    _local["session_obj"] = _SessionObj(user=user, sid=sid)
    _local["cookie_manager"] = _CookieManager()
    _local["login_manager"] = _LoginManager(user)
    _local["request"] = None
