"""Core data types used throughout the frappe shim."""


class _dict(dict):
    """dict-like object that exposes keys as attributes"""

    __slots__ = ()
    __getattr__ = dict.get
    __setattr__ = dict.__setitem__
    __delattr__ = dict.__delitem__

    def update(self, *args, **kwargs):
        super().update(*args, **kwargs)
        return self

    def copy(self):
        return _dict(self)

    def as_dict(self, no_nulls=False):
        return _dict(self)


def _filter(data, filters, limit=None):
    """Lightweight filter helper for dict lists (real BaseDocument._filter port)."""
    out = []
    for d in data:
        for k, v in filters.items():
            val = d.get(k) if isinstance(d, dict) else getattr(d, k, None)
            if isinstance(v, (list, tuple)) and len(v) == 2:
                op, operand = v[0], v[1]
                if not _compare_value(val, op, operand):
                    break
            elif isinstance(v, bool):
                if bool(val) != v:
                    break
            elif isinstance(v, str) and v.startswith("^"):
                if not str(val or "").startswith(v[1:]):
                    break
            elif val != v:
                break
        else:
            out.append(d)
            if limit and len(out) >= limit:
                break
    return out


def _compare_value(value, operator, operand):
    op = (operator or "=").lower()
    if op == "=":
        return value == operand
    if op in ("!=", "<>"):
        return value != operand
    if op == ">":
        return value is not None and operand is not None and value > operand
    if op == ">=":
        return value is not None and operand is not None and value >= operand
    if op == "<":
        return value is not None and operand is not None and value < operand
    if op == "<=":
        return value is not None and operand is not None and value <= operand
    if op == "in":
        return value in operand
    if op == "not in":
        return value not in operand
    if op == "like":
        return operand in str(value or "")
    return value == operand


class DocStatus:
    DRAFT = 0
    SUBMITTED = 1
    CANCELLED = 2

    def __init__(self, value=0):
        self.value = int(value or 0)

    def is_draft(self):
        return self.value == self.DRAFT

    def is_submitted(self):
        return self.value == self.SUBMITTED

    def is_cancelled(self):
        return self.value == self.CANCELLED

    def __int__(self):
        return self.value

    def __eq__(self, other):
        return int(self) == int(other)

    def __repr__(self):
        return f"DocStatus({self.value})"


class _DocProxy(_dict):
    """Lightweight document proxy returned by get_doc.

    Behaves like frappe's Document for attribute/dict access without
    triggering load_from_db, get_meta, or any DB round-trips.
    Child table fields are lists of _DocProxy instances.
    """

    @property
    def flags(self):
        if "flags" not in self:
            self["flags"] = _dict()
        return self["flags"]

    @property
    def meta(self):
        import frappe
        return frappe.get_meta(self.doctype)

    @property
    def docstatus(self):
        return DocStatus(self.get("docstatus", 0))

    def as_dict(self, no_nulls=False, no_default_fields=False,
                convert_dates_to_str=False, no_child_table_fields=False,
                no_private_properties=False, ignore_computed_child_tables=False):
        out = _dict()
        for k, v in self.items():
            if k.startswith("_") and no_private_properties:
                continue
            if k == "flags":
                out[k] = dict(v)
                continue
            if isinstance(v, list):
                out[k] = [i.as_dict() if isinstance(i, _DocProxy) else i for i in v]
            else:
                out[k] = v
        return out

    def get_blocked_modules(self):
        mods = self.get("block_modules") or []
        if isinstance(mods, list):
            return [
                m.get("module")
                for m in mods
                if isinstance(m, dict) and m.get("module")
            ]
        return []

    def get_link_groups(self):
        """Mirror of Workspace.get_link_groups — groups links into cards."""
        from ._misc import get_system_settings

        cards = []
        current_card = _dict({
            "label": "Link",
            "type": "Card Break",
            "icon": None,
            "hidden": False,
        })
        card_links = []
        for link in self.get("links") or []:
            link = link.as_dict() if hasattr(link, "as_dict") else _dict(link)
            if link.get("type") == "Card Break":
                if card_links and (
                    not current_card.get("only_for")
                    or current_card.get("only_for") == get_system_settings("country")
                ):
                    current_card["links"] = card_links
                    cards.append(current_card)
                current_card = link
                card_links = []
            elif not link.get("only_for") or link.get("only_for") == get_system_settings("country"):
                card_links.append(link)
        current_card["links"] = card_links
        cards.append(current_card)
        return cards

    def get(self, key, default=None):
        return dict.get(self, key, default)

    def set(self, key, value):
        self[key] = value

    def update(self, *a, **kw):
        dict.update(self, *a, **kw)
        return self

    def append(self, key, value=None):
        if key not in self or self[key] is None:
            self[key] = []
        if isinstance(value, dict):
            value = _DocProxy(value)
        self[key].append(value)
        return value

    def extend(self, key, values):
        if key not in self or self[key] is None:
            self[key] = []
        for v in values:
            self.append(key, v)

    def get_all_children(self, parenttype=None, include_computed=False):
        meta = self.meta
        out = []
        for field in meta.get_table_fields(include_computed=include_computed):
            fieldname = field.get("fieldname")
            if fieldname:
                out.extend(self.get(fieldname) or [])
        return out

    def db_set(self, fieldname, value=None, update_modified=True, notify=False, commit=False):
        import frappe
        self[fieldname] = value
        frappe.db.set_value(self.doctype, self.name, fieldname, value)

    def db_get(self, fieldname):
        import frappe
        return frappe.db.get_value(self.doctype, self.name, fieldname)

    def check_permission(self, permtype="read", throw=False):
        """Shim: lightweight proxies are assumed readable by default."""
        return True

    def has_permission(self, permtype="read", throw=False, parent_doctype=None):
        return True

    def get_permissions(self):
        return {}

    def run_method(self, method, *args, **kwargs):
        return None

    def apply_fieldlevel_read_permissions(self):
        return self

    def add_viewed(self):
        return self

    def add_seen(self):
        return self

    def reload(self):
        import frappe
        return frappe.get_doc(self.doctype, self.name)

    def save(self, ignore_permissions=None, ignore_version=None):
        import frappe
        return frappe.save_doc(self)

    def insert(self, ignore_permissions=None):
        import frappe
        return frappe.insert_doc(self)

    def delete(self, ignore_permissions=False, force=False, delete_permanently=False):
        import frappe
        frappe.delete_doc(self.doctype, self.name)
        return self


def _make_doc_proxy(raw: dict) -> "_DocProxy":
    """Recursively wrap a raw DB dict so list values become lists of _DocProxy."""
    proxy = _DocProxy()
    for k, v in raw.items():
        if isinstance(v, list):
            proxy[k] = [_make_doc_proxy(i) if isinstance(i, dict) else i for i in v]
        else:
            proxy[k] = v

    # Initialise missing child-table fields as empty lists so that code
    # like ``[d.role for d in self.doc.roles]`` doesn't explode when the
    # doc was loaded without child rows (common in the Rust shim path).
    doctype = proxy.get("doctype")
    name = proxy.get("name")
    if doctype:
        try:
            from ._meta import get_meta

            meta = get_meta(doctype)
            for field in meta.get_table_fields():
                fieldname = field.get("fieldname")
                child_doctype = field.get("options")
                if fieldname and fieldname not in proxy:
                    proxy[fieldname] = []
                # Ensure child rows have the parent metadata JS/real Frappe expect.
                rows = proxy.get(fieldname)
                if isinstance(rows, list):
                    for idx, row in enumerate(rows, 1):
                        if isinstance(row, dict):
                            row.setdefault("doctype", child_doctype or fieldname)
                            row.setdefault("parent", name)
                            row.setdefault("parenttype", doctype)
                            row.setdefault("parentfield", fieldname)
                            row.setdefault("idx", idx)
        except Exception:
            pass

    return proxy


class _MetaProxy(_dict):
    """Lightweight DocType meta proxy loaded from JSON — never touches the DB."""

    # Frappe default columns present on every data table.
    _default_columns = ("name", "owner", "creation", "modified", "modified_by", "docstatus", "idx")

    @property
    def default_fields(self):
        return self._default_columns

    def get(self, key, filters=None, limit=None, default=None):
        value = dict.get(self, key, None)
        if filters is not None and isinstance(value, list):
            return _filter(value, filters, limit=limit)
        if value is None:
            return default
        if limit is not None and isinstance(value, (list, tuple)) and len(value) > limit:
            return value[:limit]
        return value

    def getone(self, key, filters=None):
        result = self.get(key, filters=filters, limit=1)
        if isinstance(result, (list, tuple)) and result:
            return result[0]
        return result

    def _fieldmap(self):
        return {f.get("fieldname"): f for f in (self.get("fields") or []) if f.get("fieldname")}

    def get_fields(self, fieldtypes=None):
        fields = self.get("fields") or []
        if fieldtypes:
            if isinstance(fieldtypes, str):
                fieldtypes = (fieldtypes,)
            return [f for f in fields if f.get("fieldtype") in fieldtypes]
        return fields

    def get_field(self, fieldname):
        for f in (self.get("fields") or []):
            if f.get("fieldname") == fieldname:
                return _dict(f)
        return None

    def has_field(self, fieldname):
        return self.get_field(fieldname) is not None

    def get_table_fields(self, include_computed=False):
        return [f for f in (self.get("fields") or []) if f.get("fieldtype") in ("Table", "Table MultiSelect")]

    def get_link_fields(self):
        return [f for f in (self.get("fields") or []) if f.get("fieldtype") == "Link"]

    def get_dynamic_link_fields(self):
        return [f for f in (self.get("fields") or []) if f.get("fieldtype") == "Dynamic Link"]

    def get_select_fields(self):
        return [f for f in (self.get("fields") or []) if f.get("fieldtype") == "Select"]

    def get_phone_fields(self):
        return [f for f in (self.get("fields") or []) if f.get("fieldtype") == "Phone"]

    def get_data_fields(self):
        return [f for f in (self.get("fields") or []) if f.get("fieldtype") == "Data"]

    def get_code_fields(self):
        return [f for f in (self.get("fields") or []) if f.get("fieldtype") == "Code"]

    def get_fields_to_fetch(self, fieldname):
        """Return fields that have ``fetch_from`` pointing to ``fieldname``."""
        return [f for f in (self.get("fields") or []) if f.get("fetch_from", "").startswith(f"{fieldname}.")]

    def get_title_field(self):
        if self.get("title_field"):
            return self.get("title_field")
        if self.has_field("title"):
            return "title"
        return "name"

    def get_workflow(self):
        try:
            from frappe.model.workflow import get_workflow_name
            return get_workflow_name(self.name)
        except Exception:
            return None

    def get_set_only_once_fields(self):
        return [f for f in (self.get("fields") or []) if f.get("set_only_once")]

    def get_high_permlevel_fields(self):
        return [f for f in (self.get("fields") or []) if (f.get("permlevel") or 0) > 0]

    @property
    def high_permlevel_fields(self):
        return self.get_high_permlevel_fields()

    def get_masked_fields(self):
        return []

    def get_naming_series_options(self):
        field = self.get_field("naming_series")
        if field:
            return (field.get("options") or "").split("\n")
        return []

    def get_dashboard_data(self):
        return _dict(transactions=[], non_standard_fieldnames={}, internal_links={})

    def is_nested_set(self):
        return self.has_field("lft") and self.has_field("rgt")

    def get_permissions(self, parenttype=None):
        if self.get("istable") and parenttype:
            import frappe
            return frappe.get_meta(parenttype).get("permissions") or []
        return self.get("permissions") or []

    def get_permlevel_access(self, permission_type="read", parenttype=None, user=None):
        import frappe
        roles = set(frappe.get_roles(user))
        access = []
        for perm in self.get_permissions(parenttype):
            if perm.get("role") in roles and perm.get(permission_type) and perm.get("permlevel") not in access:
                access.append(perm.get("permlevel"))
        return access

    def get_permitted_fieldnames(self, parenttype=None, user=None, permission_type="read", with_virtual_fields=True):
        return self.get_fieldnames_with_value()

    def get_fieldnames_with_value(self, with_field_meta=False, with_virtual_fields=False):
        no_value = {"Section Break", "Column Break", "Tab Break", "HTML", "Table", "Table MultiSelect", "Button", "Image", "Fold", "Heading"}
        fields = [f for f in (self.get("fields") or []) if f.get("fieldtype") not in no_value]
        if with_field_meta:
            return fields
        return [f.get("fieldname") for f in fields]

    def get_global_search_fields(self):
        return [f for f in (self.get("fields") or []) if f.get("in_global_search")]

    def get_list_fields(self):
        no_value = {"Section Break", "Column Break", "Tab Break", "HTML", "Table", "Table MultiSelect", "Button", "Image", "Fold", "Heading"}
        fields = ["name"]
        fields += [f.get("fieldname") for f in (self.get("fields") or []) if f.get("in_list_view") and f.get("fieldtype") not in no_value]
        if self.get("title_field") and self.get("title_field") not in fields:
            fields.append(self.get("title_field"))
        return fields

    def get_custom_fields(self):
        return [f for f in (self.get("fields") or []) if f.get("is_custom_field")]

    def get_image_fields(self):
        return [f for f in (self.get("fields") or []) if f.get("fieldtype") == "Attach Image"]

    def get_link_doctype(self, fieldname):
        f = self.get_field(fieldname)
        if not f:
            return None
        if f.get("fieldtype") == "Link":
            return f.get("options")
        if f.get("fieldtype") == "Dynamic Link":
            return self.get_options(f.get("options"))

    def get_options(self, fieldname):
        f = self.get_field(fieldname)
        return f.get("options") if f else None

    def get_search_fields(self):
        fields = [f.strip() for f in (self.get("search_fields") or "name").split(",")]
        if "name" not in fields:
            fields.append("name")
        return fields

    def get_translatable_fields(self):
        return [f.get("fieldname") for f in (self.get("fields") or []) if f.get("translatable")]

    def is_translatable(self, fieldname):
        f = self.get_field(fieldname)
        return bool(f and f.get("translatable"))

    def get_web_template(self, suffix=""):
        return None

    def get_row_template(self):
        return self.get_web_template(suffix="_row")

    def get_list_template(self):
        return self.get_web_template(suffix="_list")

    def get_label(self, fieldname):
        df = self.get_field(fieldname)
        if df:
            return df.get("label") or fieldname
        return fieldname

    def get_valid_fields(self):
        # Match real Frappe: only fields that have database columns.
        from frappe.model import data_fieldtypes

        fields = list(self._default_columns) + [
            f.get("fieldname")
            for f in (self.get("fields") or [])
            if f.get("fieldname") and f.get("fieldtype") in data_fieldtypes
        ]

        # Child-table rows store their parent linkage in these implicit columns.
        # Real Frappe includes them in get_valid_fields/get_valid_columns for table Doctypes.
        if self.get("istable"):
            for col in ("parent", "parenttype", "parentfield"):
                if col not in fields:
                    fields.append(col)

        return fields

    def get_valid_columns(self):
        return self.get_valid_fields()

    @property
    def _fields(self):
        return {f.get("fieldname"): _dict(f) for f in (self.get("fields") or []) if f.get("fieldname")}

    @property
    def _table_fields(self):
        return self.get_table_fields(include_computed=True)

    @property
    def _non_computed_table_fields(self):
        return self.get_table_fields()

    @property
    def _table_doctypes(self):
        return {f.get("fieldname"): f.get("options") for f in self._table_fields if f.get("fieldname") and f.get("options")}

    @property
    def _non_computed_table_doctypes(self):
        return {f.get("fieldname"): f.get("options") for f in self._non_computed_table_fields if f.get("fieldname") and f.get("options")}

    @property
    def autoname(self):
        return self.get("autoname")

    @property
    def issingle(self):
        return bool(self.get("issingle"))

    @property
    def istable(self):
        return bool(self.get("istable"))

    @property
    def is_submittable(self):
        return bool(self.get("is_submittable"))

    @property
    def description(self):
        return self.get("description")
