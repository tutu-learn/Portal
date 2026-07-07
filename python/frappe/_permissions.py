"""Permission helpers: get_roles, has_permission, _SimpleUserPermissions, get_user."""

from ._context import _local, session

try:
    import kiff_core as _rust
except ImportError:
    _rust = None


def get_roles(user=None):
    user = user or session.user
    if user == "Administrator":
        return ["Administrator", "System Manager", "All"]
    if user == "Guest":
        return ["Guest", "All"]
    if _rust is None:
        return ["Guest"]

    import frappe

    roles = []
    try:
        rows = frappe.get_all(
            "Has Role",
            filters={"parenttype": "User", "parent": user},
            fields=["role"],
        )
        roles = [r["role"] for r in rows if r.get("role")]
    except Exception:
        pass

    # Add automatic roles.
    for auto in ("All", "Guest"):
        if auto not in roles:
            roles.append(auto)

    # System users also get the implicit Desk User role.
    try:
        user_type = frappe.db.get_value("User", user, "user_type")
        if user_type == "System User" and "Desk User" not in roles:
            roles.append("Desk User")
    except Exception:
        pass

    return roles


def has_permission(doctype, ptype="read", doc=None, user=None, throw=False, **kwargs):
    user = user or session.user
    if _rust is not None:
        return _rust.has_permission(doctype, ptype, doc, user)
    # Without the Kiff runtime we cannot make a real permission decision.
    # Default to deny rather than the old unconditional allow.
    return False


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
        self.can_share = []
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

        # Mirror the "read" list for the secondary ptypes so the desk doesn't
        # see empty can_select / can_report / can_print lists when Kiff has
        # already decided the user can read these DocTypes.
        self.can_select = list(self.can_read)
        self.can_get_report = list(self.can_read)
        self.can_export = list(self.can_read)
        self.can_import = list(self.can_read)
        self.can_print = list(self.can_read)
        self.can_email = list(self.can_read)
        self.can_share = list(self.can_read)

        # Respect per-user and Module Profile block lists.
        try:
            import frappe as _frappe
            user_doc = _frappe.get_doc("User", self.name)
            blocked = set(user_doc.get_blocked_modules() or [])
            if blocked:
                self.allow_modules = [m for m in self.allow_modules if m not in blocked]
        except Exception:
            pass

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

    def load_user(self):
        """Return a dict compatible with frappe.utils.user.User.load_user()."""
        from ._document import get_value
        from ._types import _dict

        fields = [
            "creation",
            "desk_theme",
            "code_editor_type",
            "document_follow_notify",
            "email",
            "email_signature",
            "first_name",
            "full_name",
            "language",
            "last_name",
            "mute_sounds",
            "show_absolute_datetime_in_timeline",
            "send_me_a_copy",
            "user_image",
            "user_type",
            "onboarding_status",
            "default_workspace",
        ]
        d = get_value("User", self.name, fields, as_dict=True) or _dict()
        if not d:
            d = _dict({
                "email": self.name,
                "first_name": self.name,
                "full_name": self.name,
                "last_name": "",
                "user_type": "System User",
                "desk_theme": "Light",
                "language": "en",
            })

        if not d.get("full_name"):
            d.full_name = " ".join(filter(None, [d.get("first_name"), d.get("last_name")])) or self.name

        if d.get("default_workspace"):
            try:
                from ._document import get_cached_doc
                ws = get_cached_doc("Workspace", d.default_workspace)
                d.default_workspace = {
                    "name": ws.name,
                    "public": ws.public,
                    "title": ws.title,
                }
            except Exception:
                d.default_workspace = None

        d.name = self.name
        d.onboarding_status = {}  # stub
        d.roles = self.get_roles()
        d.defaults = self.get_defaults()

        if not self.can_read:
            self.build_permissions()

        for key in (
            "can_select",
            "can_create",
            "can_write",
            "can_read",
            "can_submit",
            "can_cancel",
            "can_delete",
            "can_get_report",
            "allow_modules",
            "all_read",
            "can_search",
            "in_create",
            "can_export",
            "can_import",
            "can_print",
            "can_email",
            "permitted_modules",
        ):
            d[key] = list(set(getattr(self, key, [])))
        return d

    def has_role(self, role):
        roles = self.get_roles()
        if isinstance(role, str):
            return role in roles
        return bool(set(role) & set(roles))


def get_user():
    if _local.get("user_perms") is None:
        _local["user_perms"] = _SimpleUserPermissions(session.user)
    return _local["user_perms"]
