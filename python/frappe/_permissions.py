"""Permission helpers: get_roles, has_permission, _SimpleUserPermissions, get_user."""

from ._context import _local, session

try:
    import kiff_core as _rust
except ImportError:
    _rust = None


def get_roles(user=None):
    if _rust is None:
        return ["Guest"]
    return _rust.get_roles(user or session.user)


def has_permission(doctype, ptype="read", doc=None, user=None, throw=False, **kwargs):
    if _rust is None:
        return True
    return _rust.has_permission(doctype, ptype, doc, user or session.user)


class _SimpleUserPermissions:
    """Lightweight user permissions object that avoids real frappe's complex
    permission system (which requires Meta / DocType DB round-trips)."""

    def __init__(self, user):
        self.name = user
        self.roles = []
        self.all_read = []
        self.can_create = []
        self.can_select = []
        self.can_read = []
        self.can_write = []
        self.can_submit = []
        self.can_cancel = []
        self.can_delete = []
        self.can_search = []
        self.can_get_report = []
        self.can_import = []
        self.can_export = []
        self.can_print = []
        self.can_email = []
        self.allow_modules = []
        self.in_create = []
        self.doc = None
        self.shared = []
        self.defaults = None
        self.perm_map = {}
        self.doctype_map = {}

    def get_roles(self):
        if not self.roles:
            self.roles = get_roles(session.user)
        return self.roles

    def build_permissions(self):
        self.allow_modules = list(_local.get("module_app", {}).keys())
        if _rust:
            try:
                rows = _rust.get_list("DocType", None, ["name"], None, 500)
                self.can_read = [r.get("name") for r in rows if r.get("name")]
            except Exception:
                pass
            # Populate allow_modules with all known modules so workspace
            # permission checks don't silently filter everything out.
            try:
                mod_rows = _rust.get_list("Module Def", None, ["name"], None, 500)
                for r in mod_rows:
                    mod = r.get("name")
                    if mod and mod not in self.allow_modules:
                        self.allow_modules.append(mod)
            except Exception:
                pass
        self.can_write = list(self.can_read)
        self.can_create = list(self.can_read)
        self.can_search = list(self.can_read)
        self.all_read = list(self.can_read)

    def build_doctype_map(self):
        pass

    def build_perm_map(self):
        pass

    def _get(self, key):
        if not self.can_read:
            self.build_permissions()
        return getattr(self, key)

    def get_can_read(self):
        if not self.can_read:
            self.build_permissions()
        return self.can_read

    def get_defaults(self):
        return {}

    def has_role(self, role):
        roles = self.get_roles()
        if isinstance(role, str):
            return role in roles
        return bool(set(role) & set(roles))


def get_user():
    if _local.get("user_perms") is None:
        _local["user_perms"] = _SimpleUserPermissions(session.user)
    return _local["user_perms"]
