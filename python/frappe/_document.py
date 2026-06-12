"""Document API: get_doc, get_list, get_all, get_value, new_doc, save/insert/delete."""

import json
import os

from .exceptions import DoesNotExistError
from ._types import _dict, _DocProxy, _make_doc_proxy
from ._db import _sqlite_query, db

try:
    import kiff_core as _rust
except ImportError:
    _rust = None

# Absolute path to the apps/ directory at the project root.
_APPS_DIR = os.path.join(os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))), "apps")


def _load_doc_from_fixture(doctype, name):
    """Return the raw dict from a Frappe fixture JSON file, or None if not found.

    Frappe stores fixture records as:
      apps/<app>/.../<doctype_scrubbed>/<name_scrubbed>/<name_scrubbed>.json
    """
    scrubbed_name = name.lower().replace(" ", "_").replace("-", "_")
    scrubbed_dt = doctype.lower().replace(" ", "_").replace("-", "_")
    for app in os.listdir(_APPS_DIR):
        app_dir = os.path.join(_APPS_DIR, app)
        if not os.path.isdir(app_dir):
            continue
        for root, _dirs, _files in os.walk(app_dir):
            if os.path.basename(root) == scrubbed_name:
                if os.path.basename(os.path.dirname(root)) == scrubbed_dt:
                    json_file = os.path.join(root, f"{scrubbed_name}.json")
                    if os.path.isfile(json_file):
                        with open(json_file) as f:
                            return json.load(f)
    return None


def get_doc(doctype, name=None, **kwargs):
    if _rust is None:
        from .model.document import Document
        return Document(doctype, name, **kwargs)
    try:
        raw = _rust.get_doc(doctype, name)
        raw.setdefault("doctype", doctype)
        proxy = _make_doc_proxy(raw)

        # Load child table rows from the database so that methods like
        # get_link_groups() (which iterate self.doc.links) work.
        try:
            from ._meta import get_meta

            meta = get_meta(doctype)
            for field in meta.get_table_fields():
                child_dt = field.get("options")
                fieldname = field.get("fieldname")
                if not child_dt or not fieldname:
                    continue
                children = get_list(
                    child_dt,
                    filters={
                        "parent": str(name),
                        "parenttype": doctype,
                        "parentfield": fieldname,
                    },
                    fields=None,
                    order_by="idx asc",
                    limit=500,
                )
                proxy[fieldname] = children
        except Exception:
            pass

        return proxy
    except Exception:
        pass
    # Fallback: Python sqlite3 directly
    table = doctype.lower().replace(" ", "_")
    try:
        rows = _sqlite_query(f'SELECT * FROM "{table}" WHERE name = ?', [name])
        if rows:
            raw = rows[0]
            raw.setdefault("doctype", doctype)
            raw.setdefault("name", name or "")
            return _make_doc_proxy(raw)
    except Exception:
        pass
    # Last resort: load from Frappe fixture JSON files in apps/
    try:
        raw = _load_doc_from_fixture(doctype, name)
        if raw is not None:
            raw.setdefault("doctype", doctype)
            raw.setdefault("name", name or "")
            return _make_doc_proxy(raw)
    except Exception:
        pass
    raise DoesNotExistError(f"{doctype} {name} not found")


def get_list(doctype, filters=None, fields=None, order_by=None, limit=None, **kwargs):
    if _rust is None:
        return []
    simple_filters = filters
    rows = []
    try:
        rows = _rust.get_list(doctype, simple_filters, fields, order_by, limit)
    except Exception:
        pass
    if not rows:
        # Fallback: Python sqlite3 directly
        table = doctype.lower().replace(" ", "_")
        col_str = "*"
        if isinstance(fields, list) and fields:
            col_str = ", ".join(f'"{f}"' for f in fields)
        sql = f'SELECT {col_str} FROM "{table}"'
        params = []
        if isinstance(simple_filters, dict) and simple_filters:
            conditions = " AND ".join(f'"{k}" = ?' for k in simple_filters)
            sql += f" WHERE {conditions}"
            params = list(simple_filters.values())
        if order_by:
            sql += f" ORDER BY {order_by}"
        if limit:
            sql += f" LIMIT {limit}"
        try:
            rows = _sqlite_query(sql, params)
        except Exception:
            return []
    # Wrap every row in _dict so attribute access (doc.name) works.
    result = []
    for r in rows:
        d = _dict(r)
        d.setdefault("doctype", doctype)
        result.append(d)
    return result


def get_all(
    doctype,
    filters=None,
    fields=None,
    order_by=None,
    limit=None,
    ignore_permissions=False,
    limit_page_length=None,
    start=None,
    pluck=None,
    distinct=False,
    **kwargs,
):
    rows = get_list(
        doctype, filters=filters, fields=fields, order_by=order_by,
        limit=limit or limit_page_length or 500, **kwargs
    )
    if pluck:
        return [r.get(pluck) for r in rows if r.get(pluck) is not None]
    return rows


def get_value(doctype, filters, fieldname):
    if _rust is None:
        return None
    return _rust.get_value(doctype, filters, fieldname)


def new_doc(doctype, parent_doc=None, parentfield=None, as_dict=False, **kwargs):
    from .model.document import Document
    doc = Document(doctype, None, **kwargs)
    if as_dict:
        return doc.to_dict()
    return doc


def set_value(doctype, docname, fieldname, value=None):
    db.set_value(doctype, docname, fieldname, value)


def save_doc(doc):
    if _rust is None:
        return doc
    if hasattr(doc, "doctype") and hasattr(doc, "name"):
        _rust.save_doc(doc.doctype, doc.name, doc._fields if hasattr(doc, "_fields") else {})
    return doc


def insert_doc(doc):
    if _rust is None:
        return doc
    if hasattr(doc, "doctype"):
        name = _rust.insert_doc(doc.doctype, doc._fields if hasattr(doc, "_fields") else {})
        doc.name = name
    return doc


def delete_doc(doctype, name):
    if _rust is not None:
        _rust.delete_doc(doctype, name)
