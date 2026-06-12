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


class _DocProxy(_dict):
    """Lightweight document proxy returned by get_doc.

    Behaves like frappe's Document for attribute/dict access without
    triggering load_from_db, get_meta, or any DB round-trips.
    Child table fields are lists of _DocProxy instances.
    """

    def as_dict(self, no_nulls=False):
        out = _dict()
        for k, v in self.items():
            if isinstance(v, list):
                out[k] = [i.as_dict() if isinstance(i, _DocProxy) else i for i in v]
            else:
                out[k] = v
        return out

    def get_blocked_modules(self):
        bm = self.get("block_modules") or []
        return [m.get("module") for m in bm if m.get("module")]

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
    if doctype:
        try:
            from ._meta import get_meta

            meta = get_meta(doctype)
            for field in meta.get_table_fields():
                fieldname = field.get("fieldname")
                if fieldname and fieldname not in proxy:
                    proxy[fieldname] = []
        except Exception:
            pass

    return proxy


class _MetaProxy(_dict):
    """Lightweight DocType meta proxy loaded from JSON — never touches the DB."""

    def get_fields(self, fieldtypes=None):
        fields = self.get("fields") or []
        if fieldtypes:
            return [f for f in fields if f.get("fieldtype") in fieldtypes]
        return fields

    def get_field(self, fieldname):
        for f in (self.get("fields") or []):
            if f.get("fieldname") == fieldname:
                return _dict(f)
        return None

    def has_field(self, fieldname):
        return self.get_field(fieldname) is not None

    def get_table_fields(self):
        return [f for f in (self.get("fields") or []) if f.get("fieldtype") in ("Table", "Table MultiSelect")]

    def get_link_fields(self):
        return [f for f in (self.get("fields") or []) if f.get("fieldtype") == "Link"]

    @property
    def issingle(self):
        return bool(self.get("issingle"))

    @property
    def istable(self):
        return bool(self.get("istable"))

    @property
    def description(self):
        return self.get("description")
