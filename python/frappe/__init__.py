"""
Frappe shim — drop-in replacement for the real frappe package.

Strategy:
1. Explicit overrides (db, get_doc, local, session, etc.) delegate to the
   Rust kiff_core PyO3 module.
2. The real frappe package is loaded once with ``sys.modules['frappe']``
   temporarily swapped to the real module so its internal circular imports
   resolve cleanly.  After it loads we restore the shim.
3. Everything else is lazily fetched from the real frappe package.
"""

import datetime
import importlib.util
import json
import logging
import os
import pkgutil
import sys
from collections import defaultdict

from werkzeug.datastructures import Headers

# Allow real framework submodules to be imported alongside this shim.
__path__ = pkgutil.extend_path(__path__, __name__)

# ------------------------------------------------------------------
# Lazy-load the real frappe top-level module
# ------------------------------------------------------------------
_real_frappe = None
_real_frappe_local = None
_loading_real = False


def _ensure_real_frappe():
    global _real_frappe, _loading_real
    if _real_frappe is not None or _loading_real:
        return

    _loading_real = True
    shim_module = sys.modules.get("frappe")
    shim_pkg_dir = os.path.dirname(__file__)
    mod = None
    try:
        for p in sys.path:
            init_file = os.path.join(p, "frappe", "__init__.py")
            if os.path.isfile(init_file) and "python/frappe" not in init_file:
                frappe_pkg_dir = os.path.join(p, "frappe")
                spec = importlib.util.spec_from_file_location(
                    "frappe",
                    init_file,
                    submodule_search_locations=[frappe_pkg_dir],
                )
                mod = importlib.util.module_from_spec(spec)
                # Temporarily replace sys.modules['frappe'] with the real module
                # while it initializes so internal circular imports resolve
                # against the real frappe package instead of this shim.
                sys.modules["frappe"] = mod
                sys.modules["_real_frappe"] = mod

                # Let real frappe find shim-only stubs (e.g. _optimizations)
                # before falling back to its own submodules.
                if hasattr(mod, "__path__"):
                    mod.__path__.insert(0, shim_pkg_dir)

                # Pre-load the no-op _optimizations stub so real frappe's
                # top-level init can call optimize_all().
                _opt_stub = type(sys)("frappe._optimizations")
                _opt_stub.optimize_all = lambda: None
                _opt_stub.register_fault_handler = lambda: None
                sys.modules["frappe._optimizations"] = _opt_stub
                mod._optimizations = _opt_stub

                spec.loader.exec_module(mod)
                _real_frappe = mod
                # Keep a reference to the real werkzeug Local object so we can
                # initialise the real request context in _set_request_context.
                global _real_frappe_local
                _real_frappe_local = getattr(mod, "local", None)

                # Pre-load commonly-imported real submodules while the real
                # frappe module is still active so they bind to real frappe.
                # This prevents later ``import frappe.boot`` from loading them
                # against the shim and creating circular imports.
                for _preload in (
                    "frappe.boot",
                    "frappe.desk",
                    "frappe.desk.desktop",
                    "frappe.desk.desk_page",
                    "frappe.desk.desk_views",
                    "frappe.desk.form.load",
                    "frappe.desk.form.meta",
                    "frappe.desk.doctype.route_history.route_history",
                    "frappe.www.printview",
                    "frappe.email",
                    "frappe.email.inbox",
                    "frappe.exceptions",
                ):
                    try:
                        importlib.import_module(_preload)
                    except Exception:
                        pass

                # Attach top-level submodules to the real frappe module so real
                # code that imports ``frappe`` can access ``frappe.desk`` etc.
                for _attr in ("desk", "email", "www", "exceptions"):
                    try:
                        _sub = sys.modules.get(f"frappe.{_attr}")
                        if _sub is not None:
                            setattr(mod, _attr, _sub)
                    except Exception:
                        pass

                _patch_modules()
                return
    except Exception:
        import traceback
        traceback.print_exc()
    finally:
        # Always restore the shim as the public frappe module.  Real Frappe
        # submodules that were loaded while the real module was active still
        # reference the real module object, which is patched below.
        if shim_module is not None:
            sys.modules["frappe"] = shim_module

        # If loading failed, remove partially-initialized real submodules so
        # future imports bind to the shim instead of a broken real module.
        if _real_frappe is None:
            for key in list(sys.modules.keys()):
                if key == "frappe" or key.startswith("frappe."):
                    mod_obj = sys.modules[key]
                    if mod_obj is shim_module:
                        continue
                    if mod_obj is not None and getattr(mod_obj, "__file__", "") and "apps/frappe" in mod_obj.__file__:
                        del sys.modules[key]
        # Re-apply lightweight API/function patches after cleanup.
        try:
            _patch_real_module(mod if _real_frappe is None else _real_frappe)
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
    flt, cint, cstr, as_unicode, fmt_money,
    nowdate, now_datetime, now, today,
    getdate, get_datetime, add_days, date_diff,
    _, _lt, scrub, unscrub, bold,
    parse_json, as_json, safe_decode,
)
from ._context import (
    _local, _session,
    _LocalProxy, _SessionProxy, _RequestProxy, _LoginManager, _CookieManager, _SessionObj,
    local, session,
    conf, response,
    _build_module_app, _set_context, _generate_hash, _generate_sid, _load_site_config,
)
from ._db import _sqlite_query, _Database, db
from ._meta import _doctype_json_cache, _load_doctype_json, get_meta
from ._document import (
    get_doc, get_list, get_all, get_value,
    new_doc, set_value, save_doc, insert_doc, delete_doc,
    _run_document_onload,
)
from ._permissions import (
    get_roles, has_permission,
    _SimpleUserPermissions, get_user,
)
from ._messaging import throw, msgprint, log_error, enqueue, publish_realtime
from ._misc import (
    _Cache, cache, client_cache, clear_cache,
    whitelist, whitelisted, guest_methods, xss_safe_methods,
    _system_settings_cache, get_system_settings, clear_last_message,
    get_active_domains, get_installed_apps, get_all_apps,
    get_app_path, get_site_path, get_conf,
    set_user, get_module_path, get_pymodule_path, get_doctype_module,
    request_cache,
    get_hooks, format_value, get_module, get_attr, copy_doc, get_cached_doc,
)

# Bind the shim database into the local context immediately.
_local["db"] = db

# ------------------------------------------------------------------
# Query builder (frappe.qb)
# ------------------------------------------------------------------
qb = None


def _init_qb():
    global qb
    if qb is not None:
        return qb
    try:
        db_type = getattr(conf, "db_type", "sqlite")
        if db_type == "postgres":
            from frappe.query_builder.builder import Postgres as _Builder
        elif db_type == "sqlite":
            from frappe.query_builder.builder import SQLite as _Builder
        else:
            from frappe.query_builder.builder import MariaDB as _Builder
        qb = _Builder
        _local["qb"] = _Builder
        try:
            from frappe.query_builder.utils import patch_query_execute, patch_query_aggregation
            patch_query_execute()
            patch_query_aggregation()
        except Exception:
            pass
    except Exception:
        qb = None
        _local["qb"] = None
    return qb


# ------------------------------------------------------------------
# Load the shim frappe.utils package so we can patch it onto real frappe
# ------------------------------------------------------------------
try:
    import frappe.utils as _utils_shim
except Exception:
    _utils_shim = None
utils = _utils_shim

# ------------------------------------------------------------------
# Module-level shortcuts (werkzeug LocalProxy shadow)
# These reference mutable objects inside _local so dict contents stay live.
# form_dict is also reset in _set_request_context below.
# ------------------------------------------------------------------
form_dict = form = _LocalProxy(_local, "form_dict")
flags = _LocalProxy(_local, "flags")
lang = "en"
request = _LocalProxy(_local, "request")
job = None
error_log = _LocalProxy(_local, "error_log")
debug_log = _LocalProxy(_local, "debug_log")
message_log = _LocalProxy(_local, "message_log")
user = _LocalProxy(_local, "user")
_optimizations = None

# Common module-level globals used by real Frappe code.
STANDARD_USERS = ("Guest", "Administrator")
in_test = False
controllers = {}
lazy_controllers = {}

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
    "delete_doc", "save_doc", "insert_doc", "db_sql", "db_set_values",
    "db_exists", "db_count", "get_roles", "has_permission",
    "enqueue", "publish_realtime", "whitelist", "whitelisted",
    "guest_methods", "xss_safe_methods", "throw", "msgprint",
    "log_error", "cache", "client_cache", "clear_cache", "get_system_settings",
    "get_hooks", "get_cached_doc", "get_meta", "copy_doc",
    "get_attr", "get_module", "format_value", "get_app_path",
    "get_site_path", "get_conf", "set_user", "get_user",
    "get_installed_apps", "get_all_apps", "get_active_domains",
    "get_module_path", "get_pymodule_path", "get_doctype_module",
    "request_cache", "_set_context", "_set_request_context",
    "_dict", "flt", "cint", "cstr", "as_unicode", "fmt_money",
    "nowdate", "now_datetime", "now", "today", "getdate", "get_datetime",
    "add_days", "date_diff", "_", "_lt", "scrub", "unscrub", "bold",
    "parse_json", "as_json", "safe_decode", "clear_last_message",
    "flags", "qb", "utils", "lang", "request", "job", "form",
    "error_log", "debug_log", "message_log", "user",
    "_optimizations",
    "is_setup_complete", "get_single", "logger", "call", "respond_as_web_page",
    "is_whitelisted",
    "generate_hash", "STANDARD_USERS", "in_test", "controllers", "lazy_controllers",
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
# Explicit implementations for commonly-used frappe APIs that must not be
# catch-all stubs (the stubs return decorator functions, breaking callers).
# ------------------------------------------------------------------
def is_setup_complete():
    """Return whether the site setup is complete."""
    try:
        if not db.table_exists("Installed Application"):
            return False
        rows = get_all("Installed Application", filters={"app_name": "frappe"}, fields=["is_setup_complete"])
        if rows:
            return bool(rows[0].get("is_setup_complete"))
    except BaseException:
        pass
    return True


def get_installed_apps(*, _ensure_on_bench=False):
    """Return installed Frappe apps plus Rust apps declared in rust_apps/apps.json.

    The real frappe function relies on a global ``installed_apps`` value that the
    Kiff runtime does not maintain, so we start with the core "frappe" app and
    append any Rust apps declared in ``rust_apps/apps.json``.
    """
    installed = ["frappe"]
    try:
        rust_apps_path = os.path.join(
            os.path.dirname(os.path.dirname(__file__)), "rust_apps", "apps.json"
        )
        if os.path.isfile(rust_apps_path):
            with open(rust_apps_path, encoding="utf-8") as f:
                rust_apps = json.loads(f.read())
            for app in rust_apps.get("apps", []):
                if app not in installed:
                    installed.append(app)
    except BaseException:
        pass
    return installed


def get_single(doctype):
    """Return a document for a Single DocType."""
    return get_doc(doctype, doctype)


def logger(module=None, with_more_info=False, allow_site=True, filter=None, max_size=100_000, file_count=20):
    """Return a standard Python logger."""
    return logging.getLogger(module or "frappe")


def call(fn, *args, **kwargs):
    """Call a function by dotted path or callable.

    Filters keyword arguments to the function's accepted parameters so
    real Frappe hooks that expect a newer signature don't crash on older
    app code.
    """
    import inspect
    if isinstance(fn, str):
        fn = get_attr(fn)
    try:
        sig = inspect.signature(fn)
        accepts_kwargs = any(
            p.kind == inspect.Parameter.VAR_KEYWORD for p in sig.parameters.values()
        )
        if not accepts_kwargs:
            allowed = set(sig.parameters.keys())
            kwargs = {k: v for k, v in kwargs.items() if k in allowed}
    except Exception:
        pass
    return fn(*args, **kwargs)


def respond_as_web_page(
    title,
    html,
    success=None,
    http_status_code=None,
    context=None,
    indicator_color=None,
    primary_action="/",
    primary_label=None,
    fullpage=False,
    width=None,
    template="message",
):
    """Populate frappe.response so the Rust HTTP layer can render a message page."""
    response["type"] = "page"
    response["route"] = template
    response["message_title"] = title
    response["message"] = html
    response["no_cache"] = 1
    if http_status_code:
        response["http_status_code"] = http_status_code


def is_whitelisted(method=None):
    """Shim predicate — treat every method as whitelisted."""
    if method is None:
        return True
    return True


def generate_hash(txt=None, length=56):
    """Return a hash/random string matching Frappe's generate_hash signature."""
    return _generate_hash(txt, length)


# ------------------------------------------------------------------
# Request context reset (updates module-level names that can be rebound)
# ------------------------------------------------------------------
def _set_request_context(
    kwargs_dict,
    user="Guest",
    request=None,
    request_ip=None,
    headers=None,
    cookies=None,
    method="GET",
    path="/",
    scheme="http",
    host=None,
):
    """Called by the Rust bridge before each Python method dispatch."""
    global lang

    # Ensure form_dict is a _dict so attribute access works.
    if not isinstance(kwargs_dict, _dict):
        kwargs_dict = _dict(kwargs_dict)
    _local["form_dict"] = kwargs_dict

    # Session is a real dict-like object shared by frappe.session and
    # frappe.local.session.
    sid = _generate_sid()
    _local["session"] = _SessionProxy(
        user=user,
        sid=sid,
        data=_dict(
            user=user,
            user_type="System User",
            csrf_token="",
            full_name=user,
            ipinfo=None,
            session_ip=request_ip or "127.0.0.1",
            sid=sid,
        ),
    )
    _local["session_obj"] = _SessionObj(user=user, sid=sid)
    _local["cookie_manager"] = _CookieManager()
    _local["user"] = user
    _local["request_ip"] = request_ip
    _local["user_perms"] = None
    _local["flags"] = _dict(
        currently_saving=[],
        redirect_location="",
        in_install_db=False,
        in_install=False,
        in_migrate=False,
        in_patch=False,
        in_import=False,
        in_test=False,
        in_setup_wizard=False,
        in_uninstall=False,
        in_fixtures=False,
        in_safe_exec=False,
        in_render_safe_exec=False,
        in_web_form=False,
        in_create_custom_fields=False,
        mute_messages=False,
        ignore_links=False,
        ignore_permissions=False,
        ignore_mandatory=False,
        ignore_validate=False,
        mute_emails=False,
        has_dataurl=False,
        read_only=False,
        root_login=False,
        root_password=False,
        touched_tables=[],
        doc_event_calls=[],
        link_fields={},
        selected_children=[],
        web_block_scripts=[],
        web_block_styles=[],
        allow_doctype_export=False,
        force_website_cache=False,
        disable_traceback=False,
        final_patches=False,
        error_message=None,
        auto_scroll=False,
        do_not_update_password=False,
    )
    _local["error_log"] = []
    _local["debug_log"] = []
    _local["message_log"] = []
    _local["permission_debug_log"] = []
    _local["role_permissions"] = {}
    _local["valid_columns"] = {}
    _local["conf"] = conf
    _local["db"] = db
    _local["login_manager"] = _LoginManager(user)
    _local["request_cache"] = defaultdict(dict)
    _local["jenv_restricted"] = None
    _local["jenv_unrestricted"] = None

    # Build a lightweight request proxy if the Rust bridge didn't pass one.
    if request is None:
        request = _RequestProxy(
            method=method,
            path=path,
            headers=headers,
            cookies=cookies,
            query=dict(kwargs_dict),
            request_ip=request_ip,
            scheme=scheme,
            host=host,
        )
    _local["request"] = request

    _init_qb()
    _local["qb"] = qb
    lang = _local["lang"] = conf.get("lang") or "en"

    # frappe.response is cleared but must remain the same proxy object.
    _local["response"].clear()
    _local["response"]["docs"] = []
    _local["response_headers"] = Headers()

    # Rebind module-level names that are not proxies so freshly-imported
    # code sees the updated values.
    mod = sys.modules["frappe"]
    mod.form_dict = kwargs_dict
    mod.form = kwargs_dict
    mod.lang = lang
    mod.user = user

    # Mirror the same values into real Frappe's werkzeug-style local store so
    # module-level proxies such as ``frappe.lang`` and ``frappe.db`` resolve.
    # Keep ``session`` as a _SessionProxy (it exposes .user) rather than a
    # plain dict, because real Frappe code accesses ``local.session.user``.
    if _real_frappe_local is not None:
        try:
            for _key, _val in _local.items():
                try:
                    if _key == "session":
                        setattr(_real_frappe_local, _key, _val)
                    else:
                        setattr(_real_frappe_local, _key, _val)
                except Exception:
                    pass
            _real_frappe_local.user = user
            _real_frappe_local.lang = lang
            _real_frappe_local.session = _local["session"]
            _real_frappe_local.form_dict = kwargs_dict
            _real_frappe_local.response = _local["response"]
            _real_frappe_local.flags = _local["flags"]
        except Exception:
            pass


# ------------------------------------------------------------------
# Monkey-patches applied after real frappe loads
# ------------------------------------------------------------------
def _patch_real_module(mod):
    """Patch a (possibly partially-loaded) real frappe module with shim
    objects and lightweight replacements for heavy real-frappe functions."""
    if mod is None:
        return
    for attr in (
        "cache", "client_cache", "db", "conf", "response", "session", "flags",
        "get_user", "get_roles", "has_permission",
        "get_list", "get_all", "get_value", "set_value",
        "get_doc", "new_doc", "save_doc", "insert_doc", "delete",
        "get_meta", "get_cached_doc", "copy_doc",
        "get_attr", "get_module", "format_value",
        "get_system_settings", "get_hooks",
        "get_installed_apps", "get_all_apps", "get_active_domains",
        "is_setup_complete", "get_single", "logger", "call", "respond_as_web_page",
        "is_whitelisted",
        "throw", "msgprint", "log_error", "enqueue", "publish_realtime",
        "parse_json", "as_json", "safe_decode",
        "qb", "utils",
        "_set_request_context",
    ):
        try:
            val = globals().get(attr)
            if val is not None:
                setattr(mod, attr, val)
        except Exception:
            pass
    try:
        setattr(mod, "DoesNotExistError", DoesNotExistError)
    except Exception:
        pass

    # Patch OAuth token exchange so Microsoft Entra ID receives a scope in the
    # token request body. The scope is taken from the provider's auth_url_data.
    try:
        import frappe.utils.oauth as _oauth_mod
        if not getattr(_oauth_mod, "_kiff_patched", False):

            def _resolve_oauth_provider(provider):
                """Map a provider slug to the actual Social Login Key document name.

                Frappe's provider-specific login endpoints (login_via_office365,
                etc.) pass a hard-coded slug like "office_365".  If the user's
                Social Login Key uses a custom Provider Name, resolve it by
                matching social_login_provider instead.
                """
                import frappe

                if frappe.db.exists("Social Login Key", provider):
                    return provider
                for key in frappe.get_all(
                    "Social Login Key",
                    fields=["name", "social_login_provider"],
                ):
                    if key.social_login_provider and frappe.scrub(
                        key.social_login_provider
                    ) == provider:
                        return key.name
                return provider

            def _oauth_provider_key(login_key):
                """Map a Social Login Key name to the OAuth provider config key.

                Frappe's oauth2_providers dict is keyed by the provider type
                (e.g. "office_365"), not the Social Login Key document name
                (e.g. "microsoft").  We need the config key for get_oauth2_flow
                and for reading provider settings like api_endpoint.
                """
                import frappe

                try:
                    doc = frappe.get_doc("Social Login Key", login_key)
                    provider = doc.get("social_login_provider")
                    if provider:
                        return frappe.scrub(provider)
                except Exception:
                    pass
                return login_key

            def _oauth_login_key(provider_key):
                """Map an OAuth provider config key back to a Social Login Key name.

                Frappe's get_oauth_keys() uses the provider argument as the
                Social Login Key document name to fetch the client_secret.  When
                get_oauth2_flow() is called with the config key (office_365),
                get_oauth_keys() must receive the actual key name (microsoft).
                """
                import frappe

                try:
                    for key in frappe.get_all(
                        "Social Login Key",
                        fields=["name", "social_login_provider"],
                    ):
                        if key.social_login_provider and frappe.scrub(
                            key.social_login_provider
                        ) == provider_key:
                            return key.name
                except Exception:
                    pass
                return provider_key

            _orig_login_via_oauth2 = _oauth_mod.login_via_oauth2
            _orig_login_via_oauth2_id_token = _oauth_mod.login_via_oauth2_id_token

            def _kiff_login_via_oauth2(provider, code, state, decoder=None):
                return _orig_login_via_oauth2(
                    _resolve_oauth_provider(provider), code, state, decoder
                )

            def _kiff_login_via_oauth2_id_token(provider, code, state, decoder=None):
                return _orig_login_via_oauth2_id_token(
                    _resolve_oauth_provider(provider), code, state, decoder
                )

            _oauth_mod.login_via_oauth2 = _kiff_login_via_oauth2
            _oauth_mod.login_via_oauth2_id_token = _kiff_login_via_oauth2_id_token

            _orig_get_oauth_keys = _oauth_mod.get_oauth_keys

            def _kiff_get_oauth_keys(provider):
                # provider is the OAuth provider config key; resolve to the Social
                # Login Key document name so the stored password can be found.
                return _orig_get_oauth_keys(_oauth_login_key(provider))

            _oauth_mod.get_oauth_keys = _kiff_get_oauth_keys

            _orig_get_oauth2_providers = _oauth_mod.get_oauth2_providers

            def _kiff_get_oauth2_providers():
                providers = _orig_get_oauth2_providers()
                has_microsoft = any(
                    "login.microsoftonline.com" in (p.get("flow_params") or {}).get("access_token_url", "")
                    for p in providers.values()
                )
                if not has_microsoft:
                    providers["office_365"] = {
                        "flow_params": {
                            "name": "office_365",
                            "authorize_url": "https://login.microsoftonline.com/common/oauth2/v2.0/authorize",
                            "access_token_url": "https://login.microsoftonline.com/common/oauth2/v2.0/token",
                            "base_url": "https://login.microsoftonline.com/common/oauth2/v2.0/",
                        },
                        "auth_url_data": {
                            "scope": "https://graph.microsoft.com/User.Read openid email",
                            "response_type": "code id_token",
                        },
                        "api_endpoint": "https://graph.microsoft.com/v1.0/me",
                        "api_endpoint_args": {},
                    }
                return providers

            _oauth_mod.get_oauth2_providers = _kiff_get_oauth2_providers

            _orig_get_info_via_oauth = _oauth_mod.get_info_via_oauth

            def _kiff_get_info_via_oauth(provider, code, decoder=None, id_token=False):
                import json as _json
                import jwt

                # provider is the Social Login Key document name; get_oauth2_flow
                # and oauth2_providers are keyed by the OAuth provider type.
                provider_key = _oauth_provider_key(provider)
                oauth2_providers = _oauth_mod.get_oauth2_providers()

                # The server's Frappe build may use a different provider key than
                # the standard "office_365".  If the resolved key is missing, try
                # to find the Microsoft provider by its token endpoint.
                if provider_key not in oauth2_providers:
                    for key, cfg in oauth2_providers.items():
                        flow_params = cfg.get("flow_params") or {}
                        token_url = flow_params.get("access_token_url", "")
                        if "login.microsoftonline.com" in token_url:
                            provider_key = key
                            break
                    else:
                        raise KeyError(
                            "OAuth provider '{}' not found in oauth2_providers. "
                            "Available keys: {}".format(
                                provider_key, list(oauth2_providers.keys())
                            )
                        )

                flow = _oauth_mod.get_oauth2_flow(provider_key)

                args = {
                    "data": {
                        "code": code,
                        "redirect_uri": _oauth_mod.get_redirect_uri(provider_key),
                        "grant_type": "authorization_code",
                    }
                }

                access_token_url = oauth2_providers[provider_key]["flow_params"].get(
                    "access_token_url", ""
                )
                if "login.microsoftonline.com" in access_token_url:
                    auth_url_data = oauth2_providers[provider_key].get("auth_url_data") or {}
                    if isinstance(auth_url_data, str):
                        auth_url_data = _json.loads(auth_url_data)
                    if scope := auth_url_data.get("scope"):
                        args["data"]["scope"] = scope

                if decoder:
                    args["decoder"] = decoder

                session = flow.get_auth_session(**args)

                if id_token:
                    parsed_access = _json.loads(session.access_token_response.text)
                    token = parsed_access["id_token"]
                    info = jwt.decode(
                        token, flow.client_secret, options={"verify_signature": False}
                    )
                else:
                    api_endpoint = oauth2_providers[provider_key].get("api_endpoint")
                    api_endpoint_args = oauth2_providers[provider_key].get("api_endpoint_args")
                    info = session.get(api_endpoint, params=api_endpoint_args).json()

                    if provider_key == "github" and not info.get("email"):
                        emails = session.get("/user/emails", params=api_endpoint_args).json()
                        email_dict = next(filter(lambda x: x.get("primary"), emails))
                        info["email"] = email_dict.get("email")

                if not (info.get("email_verified") or _oauth_mod.get_email(info)):
                    from frappe import throw, _

                    throw(_("Email not verified with {0}").format(provider.title()))

                return info

            _oauth_mod.get_info_via_oauth = _kiff_get_info_via_oauth
            _oauth_mod._kiff_patched = True
    except Exception:
        pass

    # Patch password removal to use a direct SQL DELETE on the __auth table.
    # The shim's generic db.delete() path can struggle with the composite-key
    # __auth table, leaving orphaned secrets when a document is deleted.
    try:
        import frappe.utils.password as _password_mod
        if not getattr(_password_mod, "_kiff_patched", False):
            _orig_remove_encrypted_password = _password_mod.remove_encrypted_password

            def _kiff_remove_encrypted_password(doctype, name, fieldname="password"):
                import frappe

                try:
                    frappe.db.sql(
                        'DELETE FROM "__auth" WHERE doctype = ? AND name = ? AND fieldname = ?',
                        (doctype, name, fieldname),
                    )
                except Exception:
                    # Fall back to the original implementation if direct SQL fails.
                    _orig_remove_encrypted_password(doctype, name, fieldname)

            _password_mod.remove_encrypted_password = _kiff_remove_encrypted_password
            _password_mod._kiff_patched = True
    except Exception:
        pass

    # Patch Social Login Key validation so empty/placeholder Code field values
    # (e.g. "", "None", "undefined") don't crash json.loads().
    try:
        from frappe.integrations.doctype.social_login_key.social_login_key import (
            SocialLoginKey as _SocialLoginKey,
        )
        if not getattr(_SocialLoginKey, "_kiff_patched", False):
            _orig_slk_validate = _SocialLoginKey.validate

            def _normalize_json_code_field(value):
                if not isinstance(value, str):
                    return value
                value = value.strip()
                if not value or value in ("None", "undefined", "null"):
                    return None
                return value

            def _patched_slk_validate(self):
                self.auth_url_data = _normalize_json_code_field(self.auth_url_data)
                self.api_endpoint_args = _normalize_json_code_field(self.api_endpoint_args)
                return _orig_slk_validate(self)

            _SocialLoginKey.validate = _patched_slk_validate
            _SocialLoginKey._kiff_patched = True
    except Exception:
        pass

    # Provide lightweight shim replacements for functions that real frappe
    # implements with heavy dependencies (query builder, MariaDB, Redis, etc.)
    # so bootinfo can run without a full real-frappe runtime.
    try:
        import frappe.defaults as _defaults
        if not getattr(_defaults, "_kiff_patched", False):

            def _kiff_get_defaults(user=None):
                # Merge global defaults with sensible hard-coded fallbacks.
                defaults = _dict(
                    date_format="yyyy-mm-dd",
                    time_format="HH:mm:ss",
                    float_precision=3,
                    currency_precision=2,
                    currency="USD",
                    hide_currency_symbol="No",
                    rounding_method="Banker's Rounding (legacy)",
                    setup_complete=1,
                    letter_head=None,
                    session_recording_start=0,
                    disable_change_log_notification=1,
                    max_report_rows=100000,
                    link_field_results_limit=10,
                    force_web_capture_mode_for_uploads=0,
                )
                # Try to overlay real defaults from the DB if the table exists.
                try:
                    defaults.update(db.get_defaults(user=user) or {})
                except Exception:
                    pass
                return defaults

            _defaults.get_defaults = _kiff_get_defaults
            _defaults.get_defaults_for = lambda parent="__default": _dict()
            _defaults._kiff_patched = True
    except Exception:
        pass

    # Stub session-default settings so bootinfo doesn't abort.
    try:
        import frappe.core.doctype.session_default_settings.session_default_settings as _sds
        if not getattr(_sds, "_kiff_patched", False):
            import json as _json
            _sds.get_session_default_values = lambda: _json.dumps([])
            _sds._kiff_patched = True
    except Exception:
        pass

    # Stub print style generation so bootinfo doesn't require Jinja templates.
    try:
        import frappe.www.printview as _printview
        if not getattr(_printview, "_kiff_patched", False):
            _orig_get_print_style = _printview.get_print_style

            def _patched_get_print_style(print_style=None, for_legacy=False):
                try:
                    return _orig_get_print_style(print_style, for_legacy=for_legacy)
                except Exception:
                    return ""

            _printview.get_print_style = _patched_get_print_style
            _printview._kiff_patched = True
    except Exception:
        pass

    # FormMeta copies ``frappe.get_meta(...).__dict__`` into itself.  The shim
    # meta is a dict subclass, so its ``__dict__`` is empty.  Patch FormMeta to
    # copy the mapping contents instead and to skip filesystem-based asset
    # loading (we don't have a real bench/module tree in the Rust runtime).
    try:
        from frappe.desk.form import meta as _form_meta
        if not getattr(_form_meta.FormMeta, "_kiff_patched", False):

            def _patched_formmeta_init(self, doctype, *, cached=True):
                import frappe as _frappe
                _meta = _frappe.get_meta(doctype, cached=cached)
                self.__dict__.update(_meta)
                # Ensure common flags that real Meta always exposes.
                for _flag in ("istable", "issingle", "custom"):
                    self.__dict__.setdefault(_flag, 0)
                self.__dict__.setdefault("module", "Core")
                self.load_assets()

            def _patched_load_assets(self):
                if self.get("__assets_loaded", False):
                    return
                for _key in _form_meta.ASSET_KEYS:
                    self.__dict__.setdefault(_key, None)
                self.__dict__["__assets_loaded"] = True

            def _patched_formmeta_as_dict(self, no_nulls=False):
                # FormMeta is built from plain JSON/dict meta; the inherited
                # Document._serialize skips non-Document child lists. Return the
                # internal dict directly so fields/permissions survive.
                from frappe._types import _dict
                return _dict(self.__dict__)

            _form_meta.FormMeta.__init__ = _patched_formmeta_init
            _form_meta.FormMeta.load_assets = _patched_load_assets
            _form_meta.FormMeta.as_dict = _patched_formmeta_as_dict
            _form_meta.FormMeta._kiff_patched = True
    except Exception:
        pass

    # Workspace loader (frappe.desk.desktop.Workspace) assumes child-table
    # attributes on the Workspace doc are always lists. With the minimal
    # SQLite-backed fixtures used by Kiff they can be None, which crashes the
    # desk page. Patch the getters to treat None as an empty list.
    try:
        from frappe.desk import desktop as _desktop_mod
        if not getattr(_desktop_mod.Workspace, "_kiff_patched", False):
            import functools

            def _safe_list(getter):
                @functools.wraps(getter)
                def wrapper(self, *args, **kwargs):
                    return getter(self, *args, **kwargs) or []
                return wrapper

            for _method_name in (
                "get_charts",
                "get_shortcuts",
                "get_quick_lists",
                "get_number_cards",
                "get_custom_blocks",
            ):
                _orig = getattr(_desktop_mod.Workspace, _method_name)
                setattr(_desktop_mod.Workspace, _method_name, _safe_list(_orig))

            _desktop_mod.Workspace._kiff_patched = True
    except Exception:
        pass

    # Document.save() calls load_doc_before_save(), which iterates child-table
    # fields with ``for row in self.get(fieldname)``. In the minimal SQLite
    # runtime child tables can be None, causing a TypeError. Treat None as an
    # empty list.
    try:
        from frappe.model.document import Document as _Document
        if not getattr(_Document, "_kiff_patched_load_doc_before_save", False):
            _orig_load_doc_before_save = _Document.load_doc_before_save

            def _patched_load_doc_before_save(self, raise_exception=True):
                import frappe as _frappe

                self._doc_before_save = None
                if self.is_new():
                    return

                try:
                    self._doc_before_save = _frappe.get_doc(
                        self.doctype, self.name, for_update=True
                    )
                except _frappe.DoesNotExistError:
                    if raise_exception:
                        raise
                    return _frappe.clear_last_message()

                for fieldname in self._non_computed_table_fieldnames:
                    for row in self.get(fieldname) or []:
                        row._doc_before_save = next(
                            (
                                d
                                for d in (self._doc_before_save.get(fieldname) or [])
                                if d.name == row.name
                            ),
                            None,
                        )

            _Document.load_doc_before_save = _patched_load_doc_before_save
            _Document._kiff_patched_load_doc_before_save = True
    except Exception:
        pass

    # Desk settings are read from the User doc; seeded Administrator often has
    # all desk_properties set to 0, which hides the sidebar/search/notifications.
    # Ensure sensible defaults while still respecting real values if present.
    try:
        from frappe.core.doctype.user import user as _user_mod
        if not getattr(_user_mod, "_kiff_patched", False):
            _orig_get_desk_settings = _user_mod.get_desk_settings

            def _patched_get_desk_settings():
                settings = _orig_get_desk_settings() or _dict()
                for prop in _user_mod.desk_properties:
                    if settings.get(prop) is None:
                        settings[prop] = 1
                return settings

            _user_mod.get_desk_settings = _patched_get_desk_settings
            _user_mod._kiff_patched = True
    except Exception:
        pass

    # Document.queue_action enqueues background jobs through Redis/RQ, which is
    # not available in the minimal SQLite runtime. Run the requested action
    # synchronously instead (e.g. RoleProfile.on_update -> update_all_users).
    try:
        from frappe.model.document import Document as _Document
        if not getattr(_Document, "_kiff_patched_queue_action", False):
            _orig_queue_action = _Document.queue_action

            def _patched_queue_action(self, action, **kwargs):
                if hasattr(self, f"_{action}"):
                    action = f"_{action}"
                method = getattr(self, action, None)
                if method is None:
                    raise AttributeError(f"No action '{action}' on {self.doctype}")
                return method()

            _Document.queue_action = _patched_queue_action
            _Document._kiff_patched_queue_action = True
    except Exception:
        pass

    # The deprecated User.role_profile_name field is cleared when role_profiles
    # is empty, breaking API assignments. Convert a lone role_profile_name into
    # a role_profiles child row so populate_role_profile_roles() can run.
    try:
        from frappe.core.doctype.user.user import User as _User
        if not getattr(_User, "_kiff_patched_move_role_profile", False):
            _orig_move_role_profile = _User.move_role_profile_name_to_role_profiles

            def _patched_move_role_profile_name_to_role_profiles(self):
                if not self.role_profile_name:
                    return _orig_move_role_profile(self)

                current_role_profiles = {r.role_profile for r in (self.role_profiles or [])}
                if self.role_profile_name in current_role_profiles:
                    self.role_profile_name = None
                    return

                if not self.role_profiles:
                    self.role_profiles = []
                self.append("role_profiles", {"role_profile": self.role_profile_name})
                self.role_profile_name = None

            _User.move_role_profile_name_to_role_profiles = _patched_move_role_profile_name_to_role_profiles
            _User._kiff_patched_move_role_profile = True
    except Exception:
        pass

    # ModuleProfile.update_all_users() unpacks QB results as tuples, but the
    # SQLite query builder in this runtime returns dict rows. Handle both so
    # module-profile changes propagate to linked users.
    try:
        from frappe.core.doctype.module_profile.module_profile import ModuleProfile as _ModuleProfile
        if not getattr(_ModuleProfile, "_kiff_patched_update_all_users", False):
            _orig_mp_update_all_users = _ModuleProfile.update_all_users

            def _patched_mp_update_all_users(self):
                from collections import defaultdict

                import frappe as _frappe

                block_module = _frappe.qb.DocType("Block Module")
                user = _frappe.qb.DocType("User")

                all_current_modules = (
                    _frappe.qb.from_(user)
                    .join(block_module)
                    .on(user.name == block_module.parent)
                    .where(user.module_profile == self.name)
                    .select(user.name, block_module.module)
                ).run()

                user_modules = defaultdict(set)
                for row in all_current_modules:
                    if isinstance(row, dict):
                        user_modules[row["name"]].add(row["module"])
                    else:
                        user_modules[row[0]].add(row[1])

                module_profile_modules = {module.module for module in self.block_modules}

                for user_name, modules in user_modules.items():
                    if modules != module_profile_modules:
                        user = _frappe.get_doc("User", user_name)
                        user.block_modules = []
                        for module in module_profile_modules:
                            user.append("block_modules", {"module": module})
                        user.save()

            _ModuleProfile.update_all_users = _patched_mp_update_all_users
            _ModuleProfile._kiff_patched_update_all_users = True
    except Exception:
        pass

    # User.has_desk_access() uses frappe.db.count() with a QueryBuilder
    # expression. The SQLite bridge in this runtime returns 0 for that query
    # even when matching roles exist, so users with System Manager etc. are
    # forced to Website User and the module editor stays hidden. Use a plain
    # get_all() filter instead.
    try:
        from frappe.core.doctype.user.user import User as _User
        if not getattr(_User, "_kiff_patched_has_desk_access", False):

            def _patched_has_desk_access(self):
                if not self.roles:
                    return False
                role_names = [d.role for d in self.roles if getattr(d, "role", None)]
                if not role_names:
                    return False
                import frappe as _frappe
                return bool(
                    _frappe.get_all(
                        "Role",
                        filters={"desk_access": 1, "name": ["in", role_names]},
                        limit=1,
                    )
                )

            _User.has_desk_access = _patched_has_desk_access
            _User._kiff_patched_has_desk_access = True
    except Exception:
        pass

    # frappe.utils.user.get_user_fullname() builds a QueryBuilder expression
    # (Concat_ws) and passes a DocType object to frappe.get_value(). The shim's
    # Rust bridge only accepts string doctypes/fieldnames. Use a plain get_doc
    # fallback instead.
    try:
        from frappe.utils import user as _user_utils
        if not getattr(_user_utils, "_kiff_patched_get_user_fullname", False):
            _orig_get_user_fullname = _user_utils.get_user_fullname

            def _patched_get_user_fullname(user):
                import frappe as _frappe
                try:
                    doc = _frappe.get_doc("User", user)
                    full = " ".join(filter(None, [doc.first_name, doc.last_name])).strip()
                    return full or user
                except Exception:
                    return _orig_get_user_fullname(user)

            _user_utils.get_user_fullname = _patched_get_user_fullname
            _user_utils._kiff_patched_get_user_fullname = True
    except Exception:
        pass

    # There is no email server / Jinja template loader configured in this
    # minimal runtime, so welcome/reset emails would otherwise block User insert.
    # Make frappe.sendmail a no-op.
    try:
        if not getattr(_real_frappe, "_kiff_patched_sendmail", False):
            _real_frappe._kiff_sendmail = _real_frappe.sendmail

            def _patched_sendmail(*args, **kwargs):
                return None

            _real_frappe.sendmail = _patched_sendmail
            _real_frappe._kiff_patched_sendmail = True
    except Exception:
        pass

    # Navbar / website settings helpers hit the DB and the bench hooks tree.
    # Provide safe fallbacks so bootinfo can finish.
    try:
        from frappe.core.doctype.navbar_settings import navbar_settings as _navbar
        if not getattr(_navbar, "_kiff_patched", False):
            _navbar.get_app_logo = lambda: ""
            _navbar._kiff_patched = True
    except Exception:
        pass

    try:
        if not hasattr(_real_frappe, "_kiff_get_website_settings"):
            _real_frappe._kiff_get_website_settings = _real_frappe.get_website_settings

            def _patched_get_website_settings(key):
                if getattr(_real_frappe.local, "website_settings", None) is None:
                    _real_frappe.local.website_settings = _dict(app_logo=None)
                return _real_frappe.local.website_settings.get(key)

            _real_frappe.get_website_settings = _patched_get_website_settings
    except Exception:
        pass


def _sanitize_for_json(value, seen=None):
    """Recursively convert Frappe objects to plain JSON-serializable values."""
    if seen is None:
        seen = set()

    # Resolve Werkzeug/Flask-style local proxies first.
    proxy_target = getattr(value, "_get_current_object", None)
    if proxy_target is not None:
        try:
            return _sanitize_for_json(proxy_target(), seen)
        except Exception:
            pass

    if isinstance(value, dict):
        # Guard against circular references.
        obj_id = id(value)
        if obj_id in seen:
            return {}
        seen.add(obj_id)
        return {k: _sanitize_for_json(v, seen) for k, v in value.items()}

    if isinstance(value, (list, tuple)):
        return [_sanitize_for_json(v, seen) for v in value]

    if isinstance(value, (str, int, float, bool)) or value is None:
        return value

    if isinstance(value, (datetime.date, datetime.datetime, datetime.time)):
        return value.isoformat()

    # Frappe Document / FormMeta / similar objects expose as_dict().
    as_dict = getattr(value, "as_dict", None)
    if callable(as_dict):
        try:
            return _sanitize_for_json(as_dict(), seen)
        except Exception:
            pass

    # Fallback: string representation.
    try:
        return str(value)
    except Exception:
        return None


def _patch_modules():
    # Patch critical shim objects onto real frappe so code running inside
    # real frappe modules doesn't hit uninitialized (None) attributes.
    _patch_real_module(_real_frappe)

    try:
        import frappe.desk.listview as _listview
        if not hasattr(_listview, "get_list_view_counts"):
            def get_list_view_counts(doctype):
                return {}
            _listview.get_list_view_counts = get_list_view_counts
    except Exception:
        pass

    # Make module discovery aware of Rust apps so custom Rust modules show up
    # in the User form's "Allow Modules" list and anywhere else Frappe enumerates
    # modules per installed app.
    try:
        import frappe.utils.modules as _modules

        def _patched_get_modules_from_all_apps():
            apps = set(get_installed_apps())
            rows = get_all("Module Def", fields=["module_name", "app_name as app"])
            return [r for r in rows if r.get("app") in apps]

        _modules.get_modules_from_all_apps = _patched_get_modules_from_all_apps
    except Exception:
        pass

    # The User form's "Allow Modules" section loads __onload.all_modules via a
    # direct import of get_modules_from_all_apps, so patching the module above
    # is not enough. Patch User.onload to build the list directly from Module Def
    # using a raw SQL query to avoid metadata recursion.
    try:
        from frappe.core.doctype.user.user import User as _User

        def _patched_user_onload(self):
            rows = db.sql('SELECT module_name FROM "module_def"', as_dict=True)
            self.set_onload(
                "all_modules",
                sorted(r.get("module_name") for r in rows if r.get("module_name")),
            )

        _User.onload = _patched_user_onload
    except Exception:
        pass

    try:
        import frappe.boot as _boot
        _orig_get_bootinfo = _boot.get_bootinfo

        def _patched_get_bootinfo():
            try:
                info = _orig_get_bootinfo()
            except Exception:
                import traceback
                traceback.print_exc()
                raise
            # Ensure fields the Frappe 16 frontend always iterates are present
            # and well-typed so the desk sidebar / workspace pages don't crash.
            info.setdefault("app_data", [])
            info.setdefault("app_name_style", "Default")
            info.setdefault("desktop_icons", [])
            info.setdefault("workspace_sidebar_item", {})
            info.setdefault("module_app", {})
            info.setdefault("notes", [])
            info.setdefault("allowed_modules", [])
            info.setdefault("frequently_visited_links", [])
            info.setdefault("link_preview_doctypes", [])
            info.setdefault("calendars", [])
            info.setdefault("treeviews", [])
            info.setdefault("nested_set_doctypes", [])
            info.setdefault("single_types", [])
            info.setdefault("doctype_layouts", [])
            info.setdefault("success_action", [])
            info.setdefault("setup_wizard_completed_apps", [])
            info.setdefault("setup_wizard_not_required_apps", [])

            # Workspaces must expose has_access=true for logged-in users.
            if isinstance(info.get("workspaces"), dict):
                info["workspaces"].setdefault("has_access", True)
                info["workspaces"].setdefault("has_create_access", True)

            # Sidebar items must have an app string; null breaks sidebar.js.
            if isinstance(info.get("workspace_sidebar_item"), dict):
                for ws in info["workspace_sidebar_item"].values():
                    if isinstance(ws, dict) and not ws.get("app"):
                        ws["app"] = "frappe"

            return _sanitize_for_json(info)

        _boot.get_bootinfo = _patched_get_bootinfo
        # The Frappe 16 frontend calls the API method ``frappe.boot.bootinfo``,
        # but the real module only exposes ``get_bootinfo``. Alias it.
        if not hasattr(_boot, "bootinfo"):
            _boot.bootinfo = _patched_get_bootinfo

        # Desk settings read from User.desk_properties; seeded Administrator often
        # has every flag set to 0, which hides the whole desk UI. Override with
        # sensible defaults while preserving truthy values.
        _orig_get_desk_settings = _boot.get_desk_settings

        def _patched_get_desk_settings():
            settings = _orig_get_desk_settings() or _dict()
            try:
                from frappe.core.doctype.user.user import desk_properties as _dp
                for prop in _dp:
                    if not settings.get(prop):
                        settings[prop] = 1
            except Exception:
                pass
            return settings

        _boot.get_desk_settings = _patched_get_desk_settings
    except Exception:
        pass

    try:
        import frappe.utils.oauth as _oauth

        _orig_update_oauth_user = _oauth.update_oauth_user

        def _patched_update_oauth_user(user, data, provider):
            # Detect whether this is a brand-new OAuth sign-up before the
            # original function creates the User document.
            try:
                get_doc("User", user)
                is_new = False
            except DoesNotExistError:
                is_new = True

            result = _orig_update_oauth_user(user, data, provider)

            # New OAuth users are created as Website User with no roles by
            # default. Give them the standard "All" role so they become System
            # Users and can access the desk / have roles assigned.
            if is_new and result is not False:
                try:
                    user_doc = get_doc("User", user)
                    if not any(r.role == "All" for r in user_doc.get("roles", [])):
                        user_doc.append("roles", {"role": "All"})
                        user_doc.flags.ignore_permissions = True
                        user_doc.save()
                except Exception:
                    pass

            return result

        _oauth.update_oauth_user = _patched_update_oauth_user
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

    # If ``name`` looks like a real Frappe submodule (e.g. frappe.desk),
    # try to import it.  This makes ``frappe.desk.form.meta.get_meta`` work
    # without pre-loading every submodule.
    try:
        return importlib.import_module(f"frappe.{name}")
    except Exception:
        pass

    return _make_stub(name)


# ------------------------------------------------------------------
# Eagerly load the real frappe package now that the shim namespace is ready.
# This guarantees real frappe's internal circular imports resolve against the
# real module and that common submodules like ``frappe.boot`` are available.
# ------------------------------------------------------------------
_ensure_real_frappe()
_init_qb()
if _real_frappe is not None:
    _patch_real_module(_real_frappe)
