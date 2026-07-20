"""DocType metadata: _load_doctype_json, get_meta."""

import json as _json
import os
import sqlite3 as _sqlite3

from ._types import _dict, _MetaProxy
from ._utils import scrub


_doctype_json_cache: dict = {}
_doctype_db_cache: dict = {}


def _site_db_path():
    """Return the path to the current site SQLite database, if it exists."""
    this_dir = os.path.dirname(os.path.abspath(__file__))
    candidates = [
        os.path.join(os.path.dirname(os.path.dirname(this_dir)), "sites", "localhost", "site.db"),
        os.path.join(os.path.dirname(os.path.dirname(os.path.dirname(this_dir))), "sites", "localhost", "site.db"),
        os.path.join(os.getcwd(), "sites", "localhost", "site.db"),
    ]
    for c in candidates:
        if os.path.isfile(c):
            return c
    return None


def _connect_readonly(db_path):
    """Open the site DB read-only so close-time can never checkpoint or
    delete the WAL out from under the live Rust pool (POSIX fcntl locks are
    per-process, so a read-write connection here can look like the "last"
    user on close; see _db.py). Falls back to read-write only when the
    read-only open fails (WAL recovery needed, i.e. no server running)."""
    try:
        return _sqlite3.connect(f"file:{db_path}?mode=ro", uri=True)
    except _sqlite3.OperationalError:
        return _sqlite3.connect(db_path)


def _db_rows(sql, params=None):
    """Run a read-only query against the site SQLite database."""
    db_path = _site_db_path()
    if not db_path:
        return []
    try:
        conn = _connect_readonly(db_path)
        conn.row_factory = _sqlite3.Row
        cur = conn.execute(sql, params or [])
        rows = [dict(r) for r in cur.fetchall()]
        conn.close()
        return rows
    except Exception:
        return []


def _load_doctype_json(doctype: str):
    """Find and load a doctype JSON file from the apps/ directory."""
    if doctype in _doctype_json_cache:
        return _doctype_json_cache[doctype]
    fname = scrub(doctype)
    apps_root = os.path.join(os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))), "apps")
    if os.path.isdir(apps_root):
        for app_name in os.listdir(apps_root):
            app_dir = os.path.join(apps_root, app_name, app_name)
            if not os.path.isdir(app_dir):
                continue
            for module_name in os.listdir(app_dir):
                dt_file = os.path.join(app_dir, module_name, "doctype", fname, fname + ".json")
                if os.path.isfile(dt_file):
                    try:
                        with open(dt_file) as _f:
                            data = _json.load(_f)
                        _doctype_json_cache[doctype] = data
                        return data
                    except Exception:
                        pass
    _doctype_json_cache[doctype] = None
    return None


def _load_doctype_from_db(doctype: str):
    """Load DocType metadata from the synced metadata tables."""
    if doctype in _doctype_db_cache:
        return _doctype_db_cache[doctype]

    table = doctype.lower().replace(" ", "_")
    doctype_rows = _db_rows(
        'SELECT * FROM "doctype" WHERE name = ? LIMIT 1',
        [doctype],
    )
    if not doctype_rows:
        _doctype_db_cache[doctype] = None
        return None

    data = dict(doctype_rows[0])
    # SQLite stores booleans as integers; keep them as-is for JSON compatibility.
    fields = _db_rows(
        'SELECT * FROM "docfield" WHERE parent = ? ORDER BY idx',
        [doctype],
    )
    data["fields"] = [_clean_db_row(r) for r in fields]

    perms = _db_rows(
        'SELECT * FROM "docperm" WHERE parent = ? ORDER BY idx',
        [doctype],
    )
    data["permissions"] = [_clean_db_row(r) for r in perms]

    # DocPerm from the synced metadata only includes standard rows. Custom
    # permissions are stored in custom_docperm.
    custom_perms = _db_rows(
        'SELECT * FROM "custom_docperm" WHERE parent = ? ORDER BY idx',
        [doctype],
    )
    data["permissions"].extend([_clean_db_row(r) for r in custom_perms])

    _doctype_db_cache[doctype] = data
    return data


def _clean_db_row(row: dict):
    """Remove empty internal columns and normalize integer booleans."""
    out = {}
    for k, v in row.items():
        if k in ("doctype", "parent", "parentfield", "parenttype", "idx"):
            continue
        if v == "" and k not in ("label", "description", "options"):
            continue
        if isinstance(v, int) and k in (
            "issingle", "istable", "is_submittable", "read_only", "allow_import",
            "track_changes", "custom", "hidden", "reqd", "unique", "set_only_once",
            "remember_last_saved_value", "ignore_user_permissions", "allow_on_submit",
            "report_hide", "search_index", "in_list_view", "in_standard_filter",
            "in_preview", "in_global_search", "in_filter", "bold", "italic", "no_copy",
            "allow_in_quick_entry", "translatable", "collapsible", "show_dashboard",
            "read_only_depends_on", "mandatory_depends_on", "fetch_if_null",
            "is_system_generated", "if_owner", "permlevel", "docstatus",
            "read", "write", "create", "delete", "submit", "cancel", "amend",
            "report", "export", "import", "share", "print", "email", "select",
        ):
            out[k] = v
        else:
            out[k] = v
    return out


def _default_admin_permissions():
    """Return a fallback permission set for Administrator on a fresh site."""
    return [
        {
            "role": "Administrator",
            "permlevel": 0,
            "read": 1,
            "write": 1,
            "create": 1,
            "delete": 1,
            "submit": 1,
            "cancel": 1,
            "amend": 1,
            "report": 1,
            "export": 1,
            "import": 1,
            "share": 1,
            "print": 1,
            "email": 1,
            "select": 1,
            "if_owner": 0,
        },
        {
            "role": "System Manager",
            "permlevel": 0,
            "read": 1,
            "write": 1,
            "create": 1,
            "delete": 1,
            "submit": 1,
            "cancel": 1,
            "amend": 1,
            "report": 1,
            "export": 1,
            "import": 1,
            "share": 1,
            "print": 1,
            "email": 1,
            "select": 1,
            "if_owner": 0,
        },
        {
            "role": "All",
            "permlevel": 0,
            "read": 1,
            "write": 0,
            "create": 0,
            "delete": 0,
            "submit": 0,
            "cancel": 0,
            "amend": 0,
            "report": 0,
            "export": 0,
            "import": 0,
            "share": 0,
            "print": 0,
            "email": 0,
            "select": 0,
            "if_owner": 0,
        },
    ]


def get_meta(doctype, cached=True):
    """Load DocType meta from JSON file, falling back to the synced DB."""
    data = _load_doctype_json(doctype)
    if not data:
        data = _load_doctype_from_db(doctype)

    defaults = {
        "name": doctype,
        "description": None,
        "module": "Core",
        "custom": 0,
        "issingle": 0,
        "istable": 0,
        "is_submittable": 0,
        "read_only": 0,
        "allow_import": 0,
        "track_changes": 1,
        "fields": [],
        "permissions": [],
        "title_field": None,
        "search_fields": None,
        "sort_field": "modified",
        "sort_order": "DESC",
    }

    if data:
        proxy = _MetaProxy(data)
        for key, val in defaults.items():
            proxy.setdefault(key, val)

        # Ensure the frontend always sees a permission set. Empty permissions
        # cause perm.js to fail when it accesses rights_without_if_owner.
        if not proxy.get("permissions"):
            proxy["permissions"] = _default_admin_permissions()

        # Wrap nested dicts so attribute access (perm.role) works and inject
        # parent metadata that real Frappe's meta always carries.
        for key, parentfield in (("fields", "fields"), ("permissions", "permissions")):
            if key in proxy and isinstance(proxy[key], list):
                child_doctype = "DocField" if key == "fields" else "DocPerm"
                wrapped = []
                for idx, item in enumerate(proxy[key], 1):
                    if isinstance(item, dict):
                        item.setdefault("doctype", child_doctype)
                        item.setdefault("parent", doctype)
                        item.setdefault("parenttype", "DocType")
                        item.setdefault("parentfield", parentfield)
                        item.setdefault("idx", idx)
                        wrapped.append(_dict(item))
                    else:
                        wrapped.append(item)
                proxy[key] = wrapped
        return proxy

    # No JSON and no DB record: return defaults with admin permissions so the
    # desk doesn't crash on internal/virtual doctypes.
    proxy = _MetaProxy(defaults)
    proxy["permissions"] = _default_admin_permissions()
    for idx, item in enumerate(proxy["permissions"], 1):
        item.setdefault("doctype", "DocPerm")
        item.setdefault("parent", doctype)
        item.setdefault("parenttype", "DocType")
        item.setdefault("parentfield", "permissions")
        item.setdefault("idx", idx)
    proxy["permissions"] = [_dict(p) for p in proxy["permissions"]]
    return proxy
