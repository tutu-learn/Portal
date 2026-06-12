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
    if data:
        proxy = _MetaProxy(data)
        proxy.setdefault("name", doctype)
        proxy.setdefault("permissions", [])
        proxy.setdefault("fields", [])
        # Wrap nested dicts so attribute access (perm.role) works.
        for key in ("fields", "permissions"):
            if key in proxy and isinstance(proxy[key], list):
                proxy[key] = [
                    _dict(item) if isinstance(item, dict) else item for item in proxy[key]
                ]
        return proxy
    return _MetaProxy({"name": doctype, "description": None, "fields": [], "permissions": [], "issingle": 0, "istable": 0})
