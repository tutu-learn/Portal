"""DocType metadata: _load_doctype_json, get_meta."""

import json as _json
import os

from ._types import _dict, _MetaProxy
from ._utils import scrub


_doctype_json_cache: dict = {}


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


def get_meta(doctype, cached=True):
    """Load DocType meta from JSON file — never touches the database."""
    data = _load_doctype_json(doctype)
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
    return _MetaProxy(defaults)
