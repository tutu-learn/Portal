"""Database layer: _sqlite_query, _Database, db."""

import os
import re

from ._types import _dict

try:
    import kiff_core as _rust
except ImportError:
    _rust = None


def _sqlite_query(sql, params=None):
    """Direct Python sqlite3 query — bypasses the Rust pool entirely."""
    import sqlite3 as _sqlite3
    # Try a few candidate paths for the site database
    candidates = []
    this_dir = os.path.dirname(os.path.abspath(__file__))
    # From python/frappe/_db.py -> project root
    candidates.append(os.path.join(os.path.dirname(os.path.dirname(this_dir)), "sites", "localhost", "site.db"))
    # From python/frappe/_db.py -> one more level up (if packaged differently)
    candidates.append(os.path.join(os.path.dirname(os.path.dirname(os.path.dirname(this_dir))), "sites", "localhost", "site.db"))
    # Current working directory
    candidates.append(os.path.join(os.getcwd(), "sites", "localhost", "site.db"))
    db_path = None
    for c in candidates:
        if os.path.isfile(c):
            db_path = c
            break
    if db_path is None:
        raise FileNotFoundError(f"site.db not found. Tried: {candidates}")
    conn = _sqlite3.connect(db_path)
    conn.row_factory = _sqlite3.Row
    try:
        cur = conn.execute(sql, params or [])
        rows = [dict(r) for r in cur.fetchall()]
        return rows
    finally:
        conn.close()


class _Database:
    # ------------------------------------------------------------------
    # sql + query translation
    # ------------------------------------------------------------------
    def sql(self, query, values=None, as_dict=False, as_list=False, **kwargs):
        if _rust is None:
            rows = self._sqlite_fallback(query, values)
            return self._wrap_rows(rows, as_dict, as_list)

        translated = self._translate_query(query)

        try:
            rows = _rust.db_sql(translated, values or [])
        except Exception as e:
            stripped = translated.strip().upper()
            if stripped.startswith("SELECT") or stripped.startswith("SHOW"):
                try:
                    rows = _sqlite_query(translated, values or [])
                except Exception:
                    pass
                else:
                    return self._wrap_rows(rows, as_dict, as_list)
                print(f"[DB WARNING] SELECT failed, returning []: {e}\n  Query: {translated[:200]}")
                return []
            raise

        return self._wrap_rows(rows, as_dict, as_list)

    def _wrap_rows(self, rows, as_dict=False, as_list=False):
        if not isinstance(rows, list) or not rows:
            return rows
        if as_dict:
            return [_dict(r) for r in rows]
        if as_list:
            keys = list(rows[0].keys())
            return [[r.get(k) for k in keys] for r in rows]
        return rows

    def _sqlite_fallback(self, query, values=None):
        try:
            return _sqlite_query(self._translate_query(query), values or [])
        except Exception:
            return []

    def _translate_query(self, sql: str) -> str:
        # %(name)s → ?
        sql = re.sub(r"%\(\w+\)s", "?", sql)
        # backticks → double quotes
        sql = re.sub(r"`([^`]+)`", r'"\1"', sql)
        # "tabFoo Bar" → "foo_bar"
        sql = re.sub(
            r'"tab([^"]+)"',
            lambda m: '"' + m.group(1).lower().replace(" ", "_") + '"',
            sql,
        )
        # bare tabFooBar → foo_bar
        sql = re.sub(r"\btab([A-Za-z_][A-Za-z0-9_]*)\b", r"\1", sql)
        # "Single Word" or "Multi Word" names → lowercase+underscore
        sql = re.sub(
            r'"([A-Z][^"]*|[^"]*\s[^"]*)"',
            lambda m: '"' + m.group(1).lower().replace(" ", "_") + '"',
            sql,
        )
        # LIMIT offset,count
        sql = re.sub(
            r"LIMIT\s+(\d+)\s*,\s+(\d+)",
            r"LIMIT \2 OFFSET \1",
            sql,
            flags=re.IGNORECASE,
        )
        # IFNULL → COALESCE
        sql = re.sub(r"\bIFNULL\b", "COALESCE", sql, flags=re.IGNORECASE)
        # UNIX_TIMESTAMP → strftime
        sql = re.sub(r"\bUNIX_TIMESTAMP\(\)\b", "strftime('%s', 'now')", sql, flags=re.IGNORECASE)
        sql = re.sub(r"\bUNIX_TIMESTAMP\(([^)]+)\)\b", r"strftime('%s', \1)", sql, flags=re.IGNORECASE)

        return sql

    # ------------------------------------------------------------------
    # get_value
    # ------------------------------------------------------------------
    def get_value(self, doctype, filters=None, fieldname="name", as_dict=False, order_by=None, for_update=False, **kwargs):
        if _rust is None:
            return None if not as_dict else _dict()

        if fieldname == "*":
            rows = _rust.get_list(doctype, self._filters_to_dict(filters), None, order_by, 1)
            if not rows:
                return None if not as_dict else _dict()
            doc = rows[0]
            if as_dict:
                return _dict(doc)
            return list(doc.values())

        val = _rust.get_value(doctype, self._filters_to_dict(filters), fieldname)
        if as_dict:
            return _dict({fieldname: val})
        return val

    # ------------------------------------------------------------------
    # get_values
    # ------------------------------------------------------------------
    def get_values(self, doctype, filters=None, fieldname="name", as_dict=False, order_by=None, limit=None, for_update=False, **kwargs):
        if _rust is None:
            return []

        fields = None if fieldname == "*" else ([fieldname] if isinstance(fieldname, str) else list(fieldname))
        rows = _rust.get_list(doctype, self._filters_to_dict(filters), fields, order_by, limit)

        if as_dict:
            return [_dict(r) for r in rows]
        if fieldname == "*" or isinstance(fieldname, (list, tuple)):
            return [[r.get(f) for f in (fieldname if isinstance(fieldname, (list, tuple)) else [fieldname])] for r in rows]
        return [[r.get(fieldname)] for r in rows]

    # ------------------------------------------------------------------
    # Singles
    # ------------------------------------------------------------------
    def get_singles_dict(self, doctype, debug=False, for_update=False, cast=False, **kwargs):
        table = doctype.lower().replace(" ", "_")
        rows = self.sql(f'SELECT * FROM "{table}" WHERE name = ?', [doctype])
        if rows:
            return _dict(rows[0])
        return _dict()

    def get_single_value(self, doctype, fieldname, cache=True, **kwargs):
        return self.get_singles_dict(doctype).get(fieldname)

    def get_singles_value(self, *args, **kwargs):
        return self.get_single_value(*args, **kwargs)

    # ------------------------------------------------------------------
    # get_all / get_list
    # ------------------------------------------------------------------
    def get_all(self, doctype, filters=None, fields=None, order_by=None, limit_page_length=None, limit_start=None, as_list=False, with_link_fields=False, debug=False, ignore_permissions=False, user=None, **kwargs):
        from ._document import get_list
        limit = kwargs.pop("limit", None) or limit_page_length or 500
        rows = get_list(doctype, filters=filters, fields=fields, order_by=order_by, limit=limit, **kwargs)
        if as_list:
            return [list(r.fields.values()) for r in rows]
        return rows

    def get_list(self, doctype, filters=None, fields=None, order_by=None, limit_page_length=None, limit_start=None, as_list=False, with_link_fields=False, debug=False, ignore_permissions=False, user=None, **kwargs):
        from ._document import get_list
        limit = kwargs.pop("limit", None) or limit_page_length or 20
        return get_list(doctype, filters=filters, fields=fields, order_by=order_by, limit=limit, **kwargs)

    # ------------------------------------------------------------------
    # set_value / set_single_value
    # ------------------------------------------------------------------
    def set_value(self, doctype, name, field, value=None, **kwargs):
        if _rust is None:
            return
        if isinstance(field, dict):
            _rust.db_set_values(doctype, name, field)
        else:
            _rust.db_set_values(doctype, name, {field: value})

    def set_single_value(self, doctype, fieldname, value=None, **kwargs):
        table = doctype.lower().replace(" ", "_")
        self.sql(f'UPDATE "{table}" SET "{fieldname}" = ? WHERE name = ?', [value, doctype])

    # ------------------------------------------------------------------
    # exists / count
    # ------------------------------------------------------------------
    def exists(self, dt, dn=None, cache=False, **kwargs):
        if _rust is None:
            return False
        return _rust.db_exists(dt, dn)

    def count(self, dt, filters=None, debug=False, cache=False, distinct=True, **kwargs):
        if _rust is None:
            return 0
        return _rust.db_count(dt, filters)

    def estimate_count(self, doctype, **kwargs):
        return self.count(doctype)

    # ------------------------------------------------------------------
    # Defaults
    # ------------------------------------------------------------------
    def get_default(self, key, parent=None, parenttype="__default"):
        """Return a default value — shim returns None for everything."""
        return None

    def get_defaults(self, key=None, parent=None, parenttype="__default"):
        if key:
            return {key: self.get_default(key, parent, parenttype)}
        return {}

    def set_default(self, key, val, parent=None, parenttype="__default"):
        pass

    # ------------------------------------------------------------------
    # DDL / schema helpers
    # ------------------------------------------------------------------
    def table_exists(self, doctype, cached=True, **kwargs):
        table = doctype.lower().replace(" ", "_").lstrip("tab")
        try:
            rows = self.sql("SELECT name FROM sqlite_master WHERE type='table' AND name=?", [table])
            return len(rows) > 0
        except Exception:
            return False

    def field_exists(self, dt, fn, **kwargs):
        table = dt.lower().replace(" ", "_").lstrip("tab")
        try:
            rows = self.sql(f'PRAGMA table_info("{table}")')
            return any(r.get("name") == fn for r in rows)
        except Exception:
            return False

    def get_tables(self, cached=True, **kwargs):
        try:
            rows = self.sql("SELECT name FROM sqlite_master WHERE type='table'")
            return [r.get("name") for r in rows]
        except Exception:
            return []

    def a_row_exists(self, doctype, **kwargs):
        table = doctype.lower().replace(" ", "_").lstrip("tab")
        try:
            rows = self.sql(f'SELECT 1 FROM "{table}" LIMIT 1')
            return len(rows) > 0
        except Exception:
            return False

    def has_table(self, doctype, **kwargs):
        return self.table_exists(doctype)

    # ------------------------------------------------------------------
    # sql helpers
    # ------------------------------------------------------------------
    def sql_list(self, query, values=(), debug=False, **kwargs):
        rows = self.sql(query, values or [])
        return [list(r.values())[0] for r in rows if r]

    def sql_ddl(self, query, debug=False, **kwargs):
        return self.sql(query)

    def mogrify(self, query, values=None, **kwargs):
        if values:
            q = query
            for v in (values if isinstance(values, (list, tuple)) else [values]):
                q = re.sub(r"\?", repr(v), q, count=1)
            return q
        return query

    # ------------------------------------------------------------------
    # Transactions
    # ------------------------------------------------------------------
    def begin(self, read_only=False, **kwargs):
        pass

    def commit(self, **kwargs):
        if _rust is None:
            return
        _rust.db_commit()

    def rollback(self, save_point=None, **kwargs):
        if _rust is None:
            return
        _rust.db_rollback()

    def savepoint(self, save_point, **kwargs):
        pass

    def release_savepoint(self, save_point, **kwargs):
        pass

    # ------------------------------------------------------------------
    # delete
    # ------------------------------------------------------------------
    def delete(self, doctype, name, **kwargs):
        if _rust is None:
            return
        _rust.delete_doc(doctype, name)

    # ------------------------------------------------------------------
    # internal helper
    # ------------------------------------------------------------------
    def _filters_to_dict(self, filters):
        if filters is None:
            return None
        if isinstance(filters, dict):
            return filters
        if isinstance(filters, str):
            return {"name": filters}
        return None


db = _Database()
