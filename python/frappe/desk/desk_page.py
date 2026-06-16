"""Shim for frappe.desk.desk_page.

The real desk_page.get expects a ``Page`` document to exist in the database.
For essential desk routes (app, Workspaces, desk) we return a lightweight proxy
so the desk can boot without pre-created Page docs.
"""

from .._types import _dict


class _PageProxy(_dict):
    def is_permitted(self):
        return True

    def load_assets(self):
        pass

    def as_dict(self, no_nulls=False):
        return _dict(self)


_real_desk_page = None


def _get_real_desk_page():
    """Load the real frappe.desk.desk_page module without self-import."""
    global _real_desk_page
    if _real_desk_page is None:
        import importlib.util
        import os

        project_root = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
        real_path = os.path.join(project_root, "apps", "frappe", "frappe", "desk", "desk_page.py")
        if os.path.isfile(real_path):
            spec = importlib.util.spec_from_file_location("_real_desk_page", real_path)
            _real_desk_page = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(_real_desk_page)
        else:
            _real_desk_page = None
    return _real_desk_page


def get(name):
    if not name:
        name = "Workspaces"
    if name in ("app", "Workspaces", "desk"):
        return _PageProxy(
            name=name,
            title=name,
            module="Desk",
            app="frappe",
            standard="Yes",
            page_name=name.lower(),
            roles=[],
            _dynamic_page=0,
        )

    # Delegate to the real frappe desk_page for everything else.
    real = _get_real_desk_page()
    if real is not None:
        return real.get(name)

    # Final fallback: return a permissive proxy so the desk doesn't crash.
    return _PageProxy(
        name=name,
        title=name,
        module="Desk",
        app="frappe",
        standard="Yes",
        page_name=name.lower(),
        roles=[],
        _dynamic_page=0,
    )


def getpage(name: str):
    doc = get(name)
    import frappe

    frappe.response.docs.append(doc)
