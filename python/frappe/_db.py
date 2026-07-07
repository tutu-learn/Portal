"""Database layer: _sqlite_query, _Database, db."""

import os
import re
from collections import defaultdict

from ._types import _dict
from ._utils import getdate, get_datetime
from .exceptions import OperationalError, ProgrammingError, TableMissingError


_NAMED_PARAM_RE = re.compile(r"%\((\w+)\)s")

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


class _CallbackManager:
    """Minimal callback manager for before/after commit/rollback hooks."""

    def __init__(self):
        self._callbacks = []

    def add(self, fn):
        self._callbacks.append(fn)

    def run(self):
        for fn in self._callbacks:
            try:
                fn()
            except Exception:
                pass
        self._callbacks = []

    def reset(self):
        self._callbacks = []

    def __iter__(self):
        return iter(self._callbacks)


class _Database:
    db_type = "sqlite"
    db_name = "site.db"

    # Real Frappe expects these class attributes.
    DEFAULT_COLUMNS = ("name", "owner", "creation", "modified", "modified_by", "docstatus", "idx")
    OPTIONAL_COLUMNS = ("_user_tags", "_comments", "_assign", "_liked_by")
    STANDARD_VARCHAR_COLUMNS = ("name", "owner", "modified_by")
    CHILD_TABLE_COLUMNS = ("name", "idx", "parent", "parenttype", "parentfield")
    VARCHAR_LEN = 140
    MAX_COLUMN_LENGTH = 64
    MAX_WRITES_PER_TRANSACTION = 200_000

    OperationalError = OperationalError
    ProgrammingError = ProgrammingError
    TableMissingError = TableMissingError
    InternalError = OperationalError
    DataError = OperationalError
    InterfaceError = OperationalError
    SQLError = OperationalError

    def __init__(self):
        self.value_cache = defaultdict(dict)
        self.transaction_writes = 0
        self.auto_commit_on_many_writes = False
        self._disable_transaction_control = False
        self.before_commit = _CallbackManager()
        self.after_commit = _CallbackManager()
        self.before_rollback = _CallbackManager()
        self.after_rollback = _CallbackManager()
        self.setup_type_map()

    def setup_type_map(self):
        """Match frappe.database.sqlite.database.Database.setup_type_map()."""
        self.db_type = "sqlite"
        self.type_map = {
            "Currency": ("REAL", None),
            "Int": ("INTEGER", None),
            "Long Int": ("INTEGER", None),
            "Float": ("REAL", None),
            "Percent": ("REAL", None),
            "Check": ("INTEGER", None),
            "Small Text": ("TEXT", None),
            "Long Text": ("TEXT", None),
            "Code": ("TEXT", None),
            "Text Editor": ("TEXT", None),
            "Markdown Editor": ("TEXT", None),
            "HTML Editor": ("TEXT", None),
            "Date": ("DATE", None),
            "Datetime": ("TIMESTAMP", None),
            "Time": ("TIME", None),
            "Text": ("TEXT", None),
            "Data": ("TEXT", None),
            "Link": ("TEXT", None),
            "Dynamic Link": ("TEXT", None),
            "Password": ("TEXT", None),
            "Select": ("TEXT", None),
            "Rating": ("REAL", None),
            "Read Only": ("TEXT", None),
            "Attach": ("TEXT", None),
            "Attach Image": ("TEXT", None),
            "Signature": ("TEXT", None),
            "Color": ("TEXT", None),
            "Barcode": ("TEXT", None),
            "Geolocation": ("TEXT", None),
            "Duration": ("REAL", None),
            "Icon": ("TEXT", None),
            "Phone": ("TEXT", None),
            "Autocomplete": ("TEXT", None),
            "JSON": ("TEXT", None),
            # UI / no-value field types that real Frappe skips in get_valid_dict
            # but may still appear when the shim Document is used.
            "Section Break": ("TEXT", None),
            "Column Break": ("TEXT", None),
            "Tab Break": ("TEXT", None),
            "HTML": ("TEXT", None),
            "Button": ("TEXT", None),
            "Image": ("TEXT", None),
            "Fold": ("TEXT", None),
            "Heading": ("TEXT", None),
            "Table": ("TEXT", None),
            "Table MultiSelect": ("TEXT", None),
        }

    def escape(self, s, percent=True):
        """Minimal SQL string escaping for compatibility with real Frappe."""
        out = str(s).replace("'", "''")
        if percent:
            out = out.replace("%", "%%")
        return f"'{out}'"

    def is_table_missing(self, e):
        msg = str(e).lower()
        return "no such table" in msg or "does not exist" in msg

    def is_missing_column(self, e):
        msg = str(e).lower()
        return "no such column" in msg or "unknown column" in msg

    def is_duplicate_entry(self, e):
        msg = str(e).lower()
        return "unique" in msg or "duplicate" in msg

    def is_syntax_error(self, e):
        msg = str(e).lower()
        return "syntax" in msg or "near" in msg

    def is_missing_table_or_column(self, e):
        return self.is_table_missing(e) or self.is_missing_column(e)

    def is_deadlocked(self, e):
        return False

    def is_timedout(self, e):
        msg = str(e).lower()
        return "timeout" in msg

    def is_read_only_mode_error(self, e):
        return False

    def is_primary_key_violation(self, e):
        msg = str(e).lower()
        return "primary key" in msg

    def is_unique_key_violation(self, e):
        return self.is_duplicate_entry(e)

    def is_interface_error(self, e):
        return False

    def is_data_too_long(self, e):
        msg = str(e).lower()
        return "too long" in msg or "data too long" in msg

    def is_statement_timeout(self, e):
        return self.is_timedout(e)

    def cant_drop_field_or_key(self, e):
        return False

    def format_date(self, date):
        return getdate(date).strftime("%Y-%m-%d")

    def format_datetime(self, datetime):
        if not datetime:
            return "0001-01-01 00:00:00.000000"
        return get_datetime(datetime).strftime("%Y-%m-%d %H:%M:%S.%f")

    def add_index(self, doctype, fields, index_name=None):
        pass

    def drop_index(self, table_name, index_name):
        pass

    def describe(self, doctype):
        return []

    def commit(self):
        pass

    def rollback(self):
        pass

    def after_commit(self):
        pass

    def before_commit(self):
        pass

    def after_rollback(self):
        pass

    def before_rollback(self):
        pass

    # ------------------------------------------------------------------
    # sql + query translation
    # ------------------------------------------------------------------
    def sql(self, query, values=None, as_dict=False, as_list=False, **kwargs):
        if _rust is None:
            rows = self._sqlite_fallback(query, values)
            return self._wrap_rows(rows, as_dict, as_list)

        translated = self._translate_query(query)

        # PRAGMA introspection is handled directly by sqlite3; the Rust SQL
        # pool may return empty results or raise for these statements.
        stripped = translated.strip().upper()
        if stripped.startswith("PRAGMA"):
            rows = _sqlite_query(translated, values or [])
            return self._wrap_rows(rows, as_dict, as_list)

        # Convert Frappe %(name)s dict params to positional values in the
        # same order as the remaining ? placeholders.
        if isinstance(values, dict):
            values = self._ordered_values(query, values)

        try:
            rows = _rust.db_sql(translated, values or [])
        except Exception as e:
            err = str(e).lower()
            is_missing_table = "no such table" in err
            if stripped.startswith("SELECT") or stripped.startswith("SHOW"):
                # For SELECT/SHOW, try the translated query via raw sqlite3 first.
                try:
                    rows = _sqlite_query(translated, values or [])
                except Exception:
                    pass
                else:
                    return self._wrap_rows(rows, as_dict, as_list)

                # If the failure looks like a missing table and the original query
                # referenced a real Frappe "tabXxx" table, try the original query
                # verbatim. _translate_query strips the "tab" prefix for kiff's
                # DocType tables, but tables like "tabSessions" actually exist with
                # the prefix and SQLite accepts the backtick-quoted original.
                if is_missing_table and "tab" in query.lower():
                    try:
                        rows = _sqlite_query(query, values or [])
                    except Exception:
                        pass
                    else:
                        return self._wrap_rows(rows, as_dict, as_list)
                return []
            raise

        return self._wrap_rows(rows, as_dict, as_list)

    def _ordered_values(self, query, values):
        names = [m.group(1) for m in _NAMED_PARAM_RE.finditer(query)]
        if names:
            return [values.get(n) for n in names]
        return list(values.values())

    def _wrap_rows(self, rows, as_dict=False, as_list=False):
        if not isinstance(rows, list) or not rows:
            return rows
        if as_dict:
            return [_dict(r) for r in rows]
        if as_list:
            keys = list(rows[0].keys())
            return [[r.get(k) for k in keys] for r in rows]
        # Real Frappe default is tuple rows. The Rust bridge returns dicts
        # without guaranteed column order, so single-column selects become
        # one-element tuples (matches COUNT(*) / version() patterns) and
        # multi-column selects stay as dicts for safety.
        if len(rows[0]) == 1:
            return [(next(iter(r.values())),) for r in rows]
        return rows

    def _sqlite_fallback(self, query, values=None):
        try:
            return _sqlite_query(self._translate_query(query), values or [])
        except Exception:
            return []

    def _sqlite_get_value(self, doctype, filters, fieldname):
        table = doctype.lower().replace(" ", "_")
        params = []
        where = _filters_to_sql(filters or {"name": ""}, params) if filters else "1=1"
        # Ensure we have a name filter if none was provided.
        if not filters:
            return None
        rows = _sqlite_query(f'SELECT "{fieldname}" FROM "{table}" WHERE {where} LIMIT 1', params)
        if rows:
            return rows[0].get(fieldname)
        return None

    def _sqlite_get_list(self, doctype, filters, fields, order_by, limit):
        table = doctype.lower().replace(" ", "_")
        params = []
        if fields:
            col_str = ", ".join(f'"{f}"' for f in fields)
        else:
            col_str = "*"
        where = _filters_to_sql(filters or {}, params)
        sql = f'SELECT {col_str} FROM "{table}" WHERE {where}'
        if order_by:
            sql += f" ORDER BY {order_by}"
        if limit:
            sql += f" LIMIT {limit}"
        try:
            return _sqlite_query(sql, params)
        except Exception:
            return []

    def _translate_query(self, sql: str) -> str:
        # %(name)s / %s → ?
        sql = re.sub(r"%\(\w+\)s", "?", sql)
        sql = re.sub(r"%s", "?", sql)
        # backticks → double quotes
        sql = re.sub(r"`([^`]+)`", r'"\1"', sql)
        # "tabFoo Bar" → "foo_bar"
        sql = re.sub(
            r'"tab([^"]+)"',
            lambda m: '"' + m.group(1).lower().replace(" ", "_") + '"',
            sql,
        )
        # bare tabFooBar → foo_bar (require capital after tab to avoid mutilating "table_info")
        sql = re.sub(r"\btab([A-Z][A-Za-z0-9_]*)\b", lambda m: m.group(1).lower().replace(" ", "_"), sql)

        # Lower-case quoted identifiers that contain capitals or spaces, but
        # keep each quoted token separate so column aliases survive.
        def _quoted_repl(m):
            content = m.group(1)
            if any(c.isupper() or c.isspace() for c in content):
                return '"' + content.lower().replace(" ", "_") + '"'
            return m.group(0)

        sql = re.sub(r'"([^"]+)"', _quoted_repl, sql)

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
    def get(
        self,
        doctype,
        filters=None,
        fieldname="name",
        ignore=False,
        as_dict=False,
        debug=False,
        order_by=None,
        cache=False,
        for_update=False,
        run=True,
        pluck=False,
        distinct=False,
        skip_locked=False,
        wait=True,
        **kwargs,
    ):
        return self.get_value(
            doctype,
            filters=filters,
            fieldname=fieldname,
            ignore=ignore,
            as_dict=as_dict,
            debug=debug,
            order_by=order_by,
            cache=cache,
            for_update=for_update,
            run=run,
            pluck=pluck,
            distinct=distinct,
            skip_locked=skip_locked,
            wait=wait,
            **kwargs,
        )

    def get_value(
        self,
        doctype,
        filters=None,
        fieldname="name",
        ignore=False,
        as_dict=False,
        debug=False,
        order_by=None,
        cache=False,
        for_update=False,
        run=True,
        pluck=False,
        distinct=False,
        skip_locked=False,
        wait=True,
        **kwargs,
    ):
        if _rust is None:
            return None if not as_dict else _dict()

        # Single DocType path: filters is None or the doctype name itself.
        if filters is None or (isinstance(filters, str) and filters == doctype):
            single = self.get_singles_dict(doctype)
            if fieldname == "*":
                return _dict(single) if as_dict else list(single.values())
            if isinstance(fieldname, (list, tuple)):
                values = [single.get(f) for f in fieldname]
                if as_dict:
                    return _dict(dict(zip(fieldname, values)))
                return tuple(values) if isinstance(fieldname, tuple) else values
            val = single.get(fieldname)
            if as_dict:
                return _dict({fieldname: val})
            return val

        norm_filters = self._filters_to_dict(filters)

        if fieldname == "*":
            rows = _rust.get_list(doctype, norm_filters, None, order_by, 1)
            if not rows:
                rows = self._sqlite_get_list(doctype, norm_filters, None, order_by, 1)
            if not rows:
                return None if not as_dict else _dict()
            doc = rows[0]
            if as_dict:
                return _dict(doc)
            return list(doc.values())

        # Frappe API: fieldname can be a string, list or tuple.
        if isinstance(fieldname, (list, tuple)):
            fields = list(fieldname)
            rows = _rust.get_list(doctype, norm_filters, fields, order_by, 1)
            if not rows:
                rows = self._sqlite_get_list(doctype, norm_filters, fields, order_by, 1)
            if not rows:
                return None
            row = rows[0]
            values = [row.get(f) for f in fields]
            if as_dict:
                return _dict(dict(zip(fieldname, values)))
            return tuple(values) if isinstance(fieldname, tuple) else values

        val = _rust.get_value(doctype, norm_filters, fieldname)
        if val is None:
            val = self._sqlite_get_value(doctype, norm_filters, fieldname)
        if pluck:
            return val
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
        rows = self.sql(f'SELECT * FROM "{table}" WHERE name = ?', [doctype], as_dict=True)
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
        if isinstance(filters, (list, tuple)) and fields is None:
            fields = filters
            filters = None
        limit = kwargs.pop("limit", None) or limit_page_length or 500
        rows = get_list(doctype, filters=filters, fields=fields, order_by=order_by, limit=limit, **kwargs)
        if as_list:
            return [list(r.fields.values()) for r in rows]
        return rows

    def get_list(self, doctype, filters=None, fields=None, order_by=None, limit_page_length=None, limit_start=None, as_list=False, with_link_fields=False, debug=False, ignore_permissions=False, user=None, **kwargs):
        from ._document import get_list
        if isinstance(filters, (list, tuple)) and fields is None:
            fields = filters
            filters = None
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
            return None
        # Real Frappe returns the matching docname or None.
        if dn is None:
            # Single doctype or table existence check.
            if self.table_exists(dt):
                return dt
            return None
        if isinstance(dn, str):
            if _rust.db_exists(dt, dn):
                return dn
            return None
        if isinstance(dn, dict):
            # Support {"doctype": "...", ...} form.
            if "doctype" in dn:
                dn = dict(dn)
                dn.pop("doctype", None)
            table = dt.lower().replace(" ", "_")
            if table.startswith("tab"):
                table = table[3:]
            if not self.table_exists(dt):
                return None
            params = []
            where = _filters_to_sql(dn, params)
            rows = self.sql(
                f'SELECT name FROM "{table}" WHERE {where} LIMIT 1',
                params,
                as_dict=True,
            )
            if rows:
                return rows[0].get("name")
            return None
        return None

    def count(self, dt, filters=None, debug=False, cache=False, distinct=True, **kwargs):
        if _rust is None:
            return 0

        # The query builder passes a pypika Table and Criterion instead of plain
        # strings/dicts.  Extract the doctype name and, for complex expressions,
        # run a hand-built COUNT query.
        doctype = dt
        if hasattr(doctype, "_table_name"):
            doctype = doctype._table_name

        if filters is None or isinstance(filters, dict):
            return _rust.db_count(doctype, filters)

        # pypika / query-builder criterion
        if hasattr(filters, "get_sql"):
            where = filters.get_sql()
        else:
            where = str(filters)

        table = doctype.lower().replace(" ", "_")
        if table.startswith("tab"):
            table = table[3:]
        rows = self.sql(
            f'SELECT COUNT(*) as c FROM "{table}" WHERE {where}',
            as_dict=True,
        )
        if rows:
            return rows[0].get("c") or 0
        return 0

    def estimate_count(self, doctype, **kwargs):
        return self.count(doctype)

    # ------------------------------------------------------------------
    # Defaults
    # ------------------------------------------------------------------
    _DEFAULT_DEFAULTS = {
        "desktop:home_page": "Workspaces",
        "date_format": "yyyy-mm-dd",
        "time_format": "HH:mm:ss",
        "float_precision": 3,
        "currency_precision": 2,
        "currency": "USD",
        "hide_currency_symbol": "No",
        "rounding_method": "Banker's Rounding (legacy)",
        "setup_complete": 1,
    }

    def get_default(self, key, parent=None, parenttype="__default"):
        """Return a default value from the DefaultValue table or fallbacks."""
        try:
            if self.table_exists("Default Value"):
                rows = self.sql(
                    'SELECT "defvalue" FROM "tabDefault Value" WHERE "defkey" = ? AND "parent" = ? LIMIT 1',
                    [key, parent or parenttype],
                    as_dict=True,
                )
                if rows:
                    return rows[0].get("defvalue")
        except Exception:
            pass
        return self._DEFAULT_DEFAULTS.get(key)

    get_global = get_default

    def get_defaults(self, key=None, parent=None, parenttype="__default"):
        defaults = _dict(self._DEFAULT_DEFAULTS)
        try:
            if self.table_exists("Default Value"):
                rows = self.sql(
                    'SELECT "defkey", "defvalue" FROM "tabDefault Value" WHERE "parent" = ?',
                    [parent or parenttype],
                    as_dict=True,
                )
                for row in rows:
                    defaults[row.get("defkey")] = row.get("defvalue")
        except Exception:
            pass
        if key:
            return {key: defaults.get(key)}
        return defaults

    def set_default(self, key, val, parent=None, parenttype="__default"):
        pass

    # ------------------------------------------------------------------
    # DDL / schema helpers
    # ------------------------------------------------------------------
    def table_exists(self, doctype, cached=True, **kwargs):
        table = doctype.lower().replace(" ", "_")
        if table.startswith("tab"):
            table = table[3:]
        try:
            rows = self.sql("SELECT name FROM sqlite_master WHERE type='table' AND name=?", [table])
            return len(rows) > 0
        except Exception:
            return False

    def get_table_columns(self, doctype):
        """Return list of column names for a doctype."""
        table = doctype.lower().replace(" ", "_")
        if table.startswith("tab"):
            table = table[3:]
        try:
            rows = self.sql(f'PRAGMA table_info("{table}")', as_dict=True)
            columns = [r.get("name") for r in rows if r.get("name")]
            if columns:
                return columns
        except Exception:
            pass
        raise Exception(f"TableMissingError: DocType {doctype}")

    def has_column(self, doctype, column):
        return column in self.get_table_columns(doctype)

    def field_exists(self, dt, fn, **kwargs):
        return self.has_column(dt, fn)

    def get_tables(self, cached=True, **kwargs):
        try:
            rows = self.sql("SELECT name FROM sqlite_master WHERE type='table'", as_dict=True)
            tables = []
            for r in rows:
                name = r.get("name", "")
                if name:
                    # SQLite stores lower_case names; real Frappe returns tab-prefixed Doctype names.
                    tables.append("tab" + self._doctype_name_from_table(name))
            return tables
        except Exception:
            return []

    def _doctype_name_from_table(self, table_name):
        """Convert sqlite table name back to a Frappe doctype name."""
        # e.g. "default_value" -> "Default Value"
        parts = table_name.replace("_", " ").split()
        return " ".join(p.capitalize() for p in parts)

    def a_row_exists(self, doctype, **kwargs):
        table = doctype.lower().replace(" ", "_")
        if table.startswith("tab"):
            table = table[3:]
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
        rows = self.sql(query, values or [], as_dict=True)
        return [r.get(list(r.keys())[0]) for r in rows if r]

    def sql_ddl(self, query, debug=False, **kwargs):
        return self.sql(query)

    def multisql(self, sql_dict, values=(), **kwargs):
        """Execute the SQL variant matching db_type (shim only supports sqlite)."""
        query = sql_dict.get(self.db_type) or sql_dict.get("sqlite") or sql_dict.get("mariadb") or next(iter(sql_dict.values()))
        return self.sql(query, values, **kwargs)

    def get_routines(self):
        return []

    def get_creation_count(self, doctype, minutes):
        return 0

    def get_descendants(self, doctype, name):
        return []

    def bulk_insert(self, doctype, fields, values, **kwargs):
        pass

    def bulk_update(self, doctype, doc_updates, **kwargs):
        pass

    def close(self):
        pass

    def unbuffered_cursor(self):
        return self

    def get_database_size(self):
        return 0

    def get_system_setting(self, key):
        return self.get_single_value("System Settings", key)

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
        self.before_commit.run()
        if _rust is not None:
            _rust.db_commit()
        self.after_commit.run()

    def rollback(self, save_point=None, **kwargs):
        self.before_rollback.run()
        if _rust is not None:
            _rust.db_rollback()
        self.after_rollback.run()

    def savepoint(self, save_point, **kwargs):
        pass

    def release_savepoint(self, save_point, **kwargs):
        pass

    # ------------------------------------------------------------------
    # delete
    # ------------------------------------------------------------------
    def delete(self, doctype, filters=None, debug=False, **kwargs):
        if _rust is None:
            return
        if isinstance(filters, str):
            _rust.delete_doc(doctype, filters)
        elif isinstance(filters, dict):
            # Resolve matching names and delete them.
            names = self.get_values(doctype, filters, "name", limit=500)
            for row in names:
                name = row[0] if isinstance(row, (list, tuple)) else row
                if name:
                    _rust.delete_doc(doctype, name)
        elif filters is None and kwargs.get("conditions"):
            # conditions is a raw SQL fragment; unsupported in shim.
            raise NotImplementedError("delete with conditions not supported by kiff shim")
        else:
            _rust.delete_doc(doctype, filters or "")

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


def _filters_to_sql(filters, params):
    """Translate a dict of filters into SQL WHERE clause and params list."""
    conditions = []
    for field, condition in filters.items():
        if condition is None:
            conditions.append(f'"{field}" IS NULL')
            continue
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


db = _Database()
