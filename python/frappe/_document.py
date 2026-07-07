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


class _FallbackDocument(_DocProxy):
    """Minimal Document replacement used when the Rust bridge is unavailable."""

    def __init__(self, doctype, name=None, **kwargs):
        super().__init__()
        self.doctype = doctype
        self.name = name or kwargs.get("name")
        for k, v in kwargs.items():
            self[k] = v

    def to_dict(self):
        return self.as_dict()

    def save(self):
        return self

    def insert(self):
        return self

    def delete(self):
        return self

    def reload(self):
        return self

    def get(self, key, default=None):
        return dict.get(self, key, default)

    def set(self, key, value):
        self[key] = value

    def has_permission(self, permtype="read"):
        return True

    def get_permissions(self):
        return {}

    def add_comment(self, *args, **kwargs):
        return self

    def notify_update(self):
        pass


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
    # If the first argument is a dict, let the real Document implementation
    # create a new in-memory document (used by real ``new_doc``).  This must
    # be handled before the (doctype, name) branch because real Frappe's
    # dispatcher rejects a dict plus a positional name.
    if isinstance(doctype, dict):
        try:
            import frappe
            if getattr(frappe, "_real_frappe", None) is not None:
                from frappe.model.document import get_doc as _real_get_doc
                return _real_get_doc(doctype, **kwargs)
        except Exception:
            pass
        return _FallbackDocument(**doctype)

    # Prefer the real Frappe Document implementation whenever it is available.
    # Real Documents handle child tables, properties and methods that the
    # lightweight proxy cannot provide.
    try:
        import frappe
        if getattr(frappe, "_real_frappe", None) is not None:
            from frappe.model.document import get_doc as _real_get_doc
            return _real_get_doc(doctype, name, **kwargs)
    except Exception as e:
        # Missing docs are common for workspace widgets that reference records
        # we haven't seeded (Dashboard Chart, Number Card, etc.). Keep the log
        # quiet for those; only dump the traceback for unexpected failures.
        if isinstance(e, DoesNotExistError):
            pass
        else:
            import traceback
            print(f"[get_doc fallback] real get_doc failed for {doctype} {name}: {e}")
            traceback.print_exc()
        pass

    if _rust is None:
        return _FallbackDocument(doctype, name, **kwargs)

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


def _normalize_filters(filters):
    """Convert Frappe list-style filters into the dict form the Rust bridge expects.

    Supported forms:
      - dict: {"field": value} or {"field": ["operator", value]}
      - list/tuple of lists: [["field", "operator", value], ...]
      - list/tuple of dicts: [{"field": value}, ...]
      - string: treated as {"name": string}
    """
    if filters is None:
        return None
    if isinstance(filters, str):
        return {"name": filters}
    if isinstance(filters, dict):
        return filters
    if isinstance(filters, (list, tuple)):
        out = {}
        for item in filters:
            if isinstance(item, dict):
                out.update(item)
            elif isinstance(item, (list, tuple)) and len(item) >= 4:
                # [doctype, fieldname, operator, value, as_condition?].
                # If as_condition is explicitly False, skip the filter.
                if len(item) == 5 and item[4] is False:
                    continue
                _doctype, field, operator, value = item[0], item[1], item[2], item[3]
                out[field] = [operator, value]
            elif isinstance(item, (list, tuple)) and len(item) == 3:
                field, operator, value = item
                out[field] = [operator, value]
            elif isinstance(item, (list, tuple)) and len(item) == 2:
                field, value = item
                out[field] = value
            else:
                raise ValueError(f"Invalid filter item: {item}")
        return out
    return filters


def _field_keys(fields, rows):
    """Return the output keys to use for as_list=True given the requested fields."""
    if not fields:
        if rows:
            return [k for k in rows[0].keys() if k != "doctype"]
        return []
    keys = []
    for f in fields:
        if isinstance(f, dict):
            func = next((k for k in f if k != "as"), None)
            keys.append(f.get("as") or (func.lower() if func else None))
        else:
            keys.append(f)
    return keys


def _aggregate_get_list(doctype, filters, fields, as_list=False):
    """Handle get_list calls with aggregate fields like [{"COUNT": "*", "as": "result"}]."""
    table = doctype.lower().replace(" ", "_")
    exprs = []
    for f in fields:
        if isinstance(f, dict):
            func = next((k for k in f if k != "as"), None)
            if func is None:
                continue
            arg = f[func]
            alias = f.get("as") or func.lower()
            arg_sql = f'"{arg}"' if arg != "*" else "*"
            exprs.append(f'{func}({arg_sql}) AS "{alias}"')
        else:
            exprs.append(f'"{f}"')
    sql = f'SELECT {", ".join(exprs)} FROM "{table}"'
    params = []
    if filters:
        where = _filters_to_sql(filters, params)
        sql += f" WHERE {where}"
    try:
        rows = _sqlite_query(sql, params)
    except Exception:
        return []
    for r in rows:
        r.setdefault("doctype", doctype)
    if as_list:
        keys = _field_keys(fields, rows)
        return [[r.get(k) for k in keys] for r in rows]
    return [_dict(r) for r in rows]


def _operator_match(value, operator, operand):
    """Return True if ``value`` matches ``operator operand``."""
    op = (operator or "=").lower()
    if op == "=":
        return value == operand
    if op == "!=":
        return value != operand
    if op in (">", ">=", "<", "<="):
        try:
            if op == ">":
                return value > operand
            if op == ">=":
                return value >= operand
            if op == "<":
                return value < operand
            if op == "<=":
                return value <= operand
        except Exception:
            return False
    if op == "in":
        return value in operand
    if op == "not in":
        return value not in operand
    if op == "like":
        pattern = str(operand).replace("%", ".*").replace("_", ".")
        import re
        return bool(re.search(pattern, str(value), re.IGNORECASE))
    if op == "not like":
        pattern = str(operand).replace("%", ".*").replace("_", ".")
        import re
        return not bool(re.search(pattern, str(value), re.IGNORECASE))
    if op == "is":
        operand_str = str(operand).lower()
        if operand_str == "set":
            return value is not None and value != ""
        if operand_str == "not set":
            return value is None or value == ""
        return value == operand
    if op == "between":
        if isinstance(operand, (list, tuple)) and len(operand) == 2:
            try:
                return operand[0] <= value <= operand[1]
            except Exception:
                return False
        return False
    return value == operand


def _row_matches_filter(row, field, condition):
    """Check if a row matches a single filter condition."""
    value = row.get(field)
    if isinstance(condition, (list, tuple)) and len(condition) == 2:
        operator, operand = condition
        return _operator_match(value, operator, operand)
    return value == condition


def _apply_filters(rows, filters, or_filters=None):
    """Apply filters/or_filters in Python (used when Rust can't handle OR)."""
    if not filters and not or_filters:
        return rows

    def matches(row):
        if filters:
            for field, condition in filters.items():
                if not _row_matches_filter(row, field, condition):
                    return False
        if or_filters:
            for field, condition in or_filters.items():
                if _row_matches_filter(row, field, condition):
                    return True
            return False
        return True

    return [r for r in rows if matches(r)]


def _filters_to_sql(filters, params):
    """Translate normalized filters into SQL WHERE clause and params."""
    conditions = []
    for field, condition in filters.items():
        if isinstance(condition, (list, tuple)) and len(condition) == 2:
            operator, operand = condition
            op = (operator or "=").lower()
            if op == "in":
                placeholders = ", ".join("?" for _ in operand)
                conditions.append(f'"{field}" IN ({placeholders})')
                params.extend(operand)
            elif op == "not in":
                placeholders = ", ".join("?" for _ in operand)
                conditions.append(f'"{field}" NOT IN ({placeholders})')
                params.extend(operand)
            elif op == "like":
                conditions.append(f'"{field}" LIKE ?')
                params.append(operand)
            elif op == "not like":
                conditions.append(f'"{field}" NOT LIKE ?')
                params.append(operand)
            elif op == "is":
                operand_str = str(operand).lower()
                if operand_str == "set":
                    conditions.append(f'("{field}" IS NOT NULL AND "{field}" != \'\')')
                elif operand_str == "not set":
                    conditions.append(f'("{field}" IS NULL OR "{field}" = \'\')')
                else:
                    conditions.append(f'"{field}" = ?')
                    params.append(operand)
            elif op == "between":
                if isinstance(operand, (list, tuple)) and len(operand) == 2:
                    conditions.append(f'"{field}" BETWEEN ? AND ?')
                    params.extend(operand)
                else:
                    conditions.append("1=0")
            else:
                conditions.append(f'"{field}" {operator} ?')
                params.append(operand)
        else:
            conditions.append(f'"{field}" = ?')
            params.append(condition)
    return " AND ".join(conditions) if conditions else "1=1"


def _build_log_query(normalized):
    """Convert simple equality filters into a Tantivy query string."""
    if not normalized:
        return "*"
    parts = []
    for field, condition in normalized.items():
        if field in ("doctype", "name"):
            continue
        if isinstance(condition, str):
            parts.append(f'{field}:"{condition}"')
        elif isinstance(condition, (list, tuple)) and len(condition) == 2:
            operator, value = condition
            if operator == "=":
                parts.append(f'{field}:"{value}"')
    if not parts:
        return "*"
    return " AND ".join(parts)


def _query_log_engine(q, limit, fields=None):
    """Fetch Kiff Log Entry records from the log engine HTTP API.

    The Python-shim .so has its own kiff_core statics, so the Rust log service
    initialized by the runtime is not directly reachable.  We call the local
    kiff_logger query endpoint instead, which is served by the same process.
    """
    import json as _json
    import urllib.request
    import urllib.parse

    base = os.environ.get("KIFF_SERVER_URL", "http://127.0.0.1:8000")
    params = urllib.parse.urlencode({"q": q, "limit": limit})
    url = f"{base}/kiff_logger/query?{params}"
    try:
        with urllib.request.urlopen(url, timeout=30) as resp:
            body = _json.loads(resp.read().decode("utf-8"))
    except Exception:
        return []

    records = body.get("records", [])
    if fields:
        field_set = set(fields)
        records = [{k: v for k, v in rec.items() if k in field_set or k == "name"} for rec in records]
    return records


def get_list(
    doctype,
    filters=None,
    fields=None,
    order_by=None,
    limit=None,
    or_filters=None,
    limit_start=None,
    as_list=False,
    **kwargs,
):
    if _rust is None:
        return []

    # Kiff Log Entry records live in the log engine, not the SQL database.
    # Bypass the ORM and query the kiff_logger HTTP endpoint directly.
    if doctype == "Kiff Log Entry":
        normalized = _normalize_filters(filters)
        q = _build_log_query(normalized)
        page_length = limit or kwargs.pop("limit_page_length", None) or 100
        limit_start = limit_start or kwargs.pop("start", None) or 0
        rows = _query_log_engine(q, page_length + limit_start, fields=fields)
        rows = rows[limit_start:limit_start + page_length]
        if as_list:
            keys = _field_keys(fields, rows)
            return [[r.get(k) for k in keys] for r in rows]
        result = []
        for r in rows:
            d = _dict(r)
            d.setdefault("doctype", doctype)
            result.append(d)
        return result

    normalized = _normalize_filters(filters)
    or_normalized = _normalize_filters(or_filters)
    limit = limit or kwargs.pop("limit_page_length", None) or 20
    limit_start = limit_start or kwargs.pop("start", None) or 0

    # Aggregate fields like [{"COUNT": "*", "as": "result"}] are used by
    # Number Card. The Rust ORM doesn't understand them, so run them directly
    # against SQLite and alias the result.
    if isinstance(fields, list) and fields and any(isinstance(f, dict) for f in fields):
        return _aggregate_get_list(doctype, normalized, fields, as_list=as_list)

    # When OR filters are involved, Rust can't handle them.  Fetch a larger
    # result set and filter in Python, then slice.
    fetch_limit = limit + limit_start if or_normalized else limit
    if or_normalized:
        # Safety cap so we don't pull the whole table on huge datasets.
        fetch_limit = min(fetch_limit, 5000)

    rows = []
    try:
        rows = _rust.get_list(doctype, normalized, fields, order_by, fetch_limit)
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
        if isinstance(normalized, dict) and normalized:
            where = _filters_to_sql(normalized, params)
            sql += f" WHERE {where}"
        if order_by:
            sql += f" ORDER BY {order_by}"
        if limit:
            sql += f" LIMIT {limit}"
        if limit_start:
            sql += f" OFFSET {limit_start}"
        try:
            rows = _sqlite_query(sql, params)
        except Exception:
            return []
    else:
        # Post-filter for OR conditions and apply offset.
        if or_normalized:
            rows = _apply_filters(rows, normalized, or_normalized)
        rows = rows[limit_start:limit_start + limit]

    if as_list:
        keys = _field_keys(fields, rows)
        return [[r.get(k) for k in keys] for r in rows]

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
    as_list=False,
    **kwargs,
):
    # Frappe allows ``get_all(doctype, ["name", ...])`` (fields as 2nd arg).
    if isinstance(filters, (list, tuple)) and fields is None:
        fields = filters
        filters = None

    rows = get_list(
        doctype,
        filters=filters,
        fields=fields,
        order_by=order_by,
        limit=limit or limit_page_length or 500,
        limit_start=start,
        **kwargs,
    )
    if pluck:
        return [r.get(pluck) for r in rows if r.get(pluck) is not None]
    if as_list:
        if isinstance(fields, (list, tuple)) and fields:
            return [[r.get(f) for f in fields] for r in rows]
        # No explicit fields: return a list of values in key order.
        return [list(r.values()) for r in rows]
    return rows


def get_value(doctype, filters, fieldname, as_dict=False):
    if _rust is None:
        return None
    # Normalize filters: string -> {"name": string}
    if isinstance(filters, str):
        filters = {"name": filters}
    # Frappe API: fieldname can be a string, list or tuple.
    # String -> single value; list/tuple -> list/tuple of values.
    if isinstance(fieldname, (list, tuple)):
        fields = list(fieldname)
        rows = _rust.get_list(doctype, filters, fields, None, 1)
        if not rows:
            return None
        row = rows[0]
        values = [row.get(f) for f in fields]
        if as_dict:
            return _dict(dict(zip(fieldname, values)))
        return tuple(values) if isinstance(fieldname, tuple) else values
    val = _rust.get_value(doctype, filters, fieldname)
    if as_dict:
        return _dict({fieldname: val})
    return val


def new_doc(doctype, parent_doc=None, parentfield=None, as_dict=False, **kwargs):
    try:
        from frappe.model.document import new_doc as _real_new_doc

        return _real_new_doc(
            doctype,
            parent_doc=parent_doc,
            parentfield=parentfield,
            as_dict=as_dict,
            **kwargs,
        )
    except Exception:
        pass
    doc = _FallbackDocument(doctype, None, **kwargs)
    if as_dict:
        return doc.to_dict()
    return doc


def set_value(doctype, docname, fieldname, value=None):
    db.set_value(doctype, docname, fieldname, value)


def _doc_fields(doc):
    """Return the mutable field map for a document proxy or fallback."""
    if hasattr(doc, "_fields"):
        return doc._fields
    if hasattr(doc, "as_dict"):
        d = doc.as_dict()
        d.pop("doctype", None)
        d.pop("name", None)
        return d
    return {k: v for k, v in doc.items() if k not in ("doctype", "name")}


def save_doc(doc):
    if _rust is None:
        return doc
    if hasattr(doc, "doctype") and hasattr(doc, "name"):
        _rust.save_doc(doc.doctype, doc.name, _doc_fields(doc))
    return doc


def insert_doc(doc):
    if _rust is None:
        return doc
    if hasattr(doc, "doctype"):
        name = _rust.insert_doc(doc.doctype, _doc_fields(doc))
        doc.name = name
    return doc


def delete_doc(doctype, name):
    if _rust is not None:
        _rust.delete_doc(doctype, name)


def _run_document_onload(doctype, doc, user=None):
    """Run a real Document controller's ``onload`` hook for an in-memory doc.

    The native Rust ``getdoc`` path calls this so framework DocTypes that rely
    on ``__onload`` data (e.g. ``User`` needs ``__onload.all_modules``) still
    work without running the full Python getdoc flow.

    Args:
        doctype: DocType name (used for logging/dispatch).
        doc: Document data as a dict (must contain ``doctype``).
        user: Optional session user for the request context.

    Returns:
        A plain dict with the contents of ``doc.get_onload()``.
    """
    import frappe
    from frappe import _dict

    user = user or "Guest"
    frappe._set_request_context(_dict(), user=user)

    try:
        # ``frappe.get_doc`` is patched to the real Frappe implementation when
        # available, so ``doc_obj`` is a real Document subclass with onload().
        doc_obj = frappe.get_doc(doc)
        if hasattr(doc_obj, "onload") and callable(doc_obj.onload):
            doc_obj.onload()
        return dict(doc_obj.get_onload() or {})
    except Exception:
        import traceback

        traceback.print_exc()
        return {}
