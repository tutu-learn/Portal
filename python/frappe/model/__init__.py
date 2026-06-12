"""
Frappe model shim — extends __path__ and re-exports real frappe.model.
"""
import importlib.util
import os
import pkgutil
import sys

__path__ = pkgutil.extend_path(__path__, __name__)
_shim_model_dir = os.path.dirname(os.path.abspath(__file__))
for _p in sys.path:
    _candidate = os.path.join(os.path.abspath(_p), "frappe", "model")
    if os.path.isdir(_candidate) and os.path.abspath(_candidate) not in [os.path.abspath(x) for x in __path__]:
        __path__.append(_candidate)

# Eagerly load the real frappe.model/__init__.py and merge its namespace.
# We do NOT re-export Document here to avoid circular imports with the
# real frappe.model.document which imports frappe.desk.form.document_follow.
for _p in sys.path:
    _candidate = os.path.join(os.path.abspath(_p), "frappe", "model", "__init__.py")
    if os.path.isfile(_candidate) and os.path.abspath(os.path.dirname(_candidate)) != _shim_model_dir:
        spec = importlib.util.spec_from_file_location("_frappe_model_real", _candidate)
        mod = importlib.util.module_from_spec(spec)
        try:
            spec.loader.exec_module(mod)
            for _k, _v in vars(mod).items():
                if not _k.startswith("__") and _k != "Document":
                    globals()[_k] = _v
        except Exception:
            import traceback
            traceback.print_exc()
        break
