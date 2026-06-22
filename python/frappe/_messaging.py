"""Messaging, logging, queue, and realtime helpers."""

import inspect
import json

from .exceptions import ValidationError

try:
    import kiff_core as _rust
except ImportError:
    _rust = None


def throw(msg, exc=None, **kwargs):
    if exc is None:
        exc = ValidationError
    if inspect.isclass(exc):
        raise exc(msg)
    elif isinstance(exc, BaseException):
        if not exc.args:
            exc.args = (msg,)
        raise exc
    else:
        raise ValidationError(msg)


def msgprint(
    msg,
    title=None,
    raise_exception=False,
    as_table=False,
    as_list=False,
    indicator=None,
    alert=False,
    primary_action=None,
    is_minimizable=False,
    wide=False,
    realtime=False,
    allow_dangerous_html=False,
):
    print(f"[MSG] {msg}")


def log_error(title=None, message=None, reference_doctype=None, reference_name=None, defer_insert=False):
    """Log an error. Real Frappe accepts title as first positional arg or keyword."""
    label = title or message or "Error"
    body = message or title or ""
    print(f"[ERROR: {label}] {body}")


def enqueue(method, queue="default", **kwargs):
    # Match real Frappe's `now` semantics: run synchronously when requested.
    if kwargs.get("now"):
        import frappe as _frappe

        return _frappe.call(method, **{k: v for k, v in kwargs.items() if k != "now"})

    if _rust is None:
        print(f"[ENQUEUE {queue}] {method} {kwargs}")
        return

    # Drop real-Frappe-only queue options that the Rust queue doesn't consume.
    queue_kwargs = {k: v for k, v in kwargs.items() if k not in ("enqueue_after_commit",)}
    _rust.enqueue(method, queue, queue_kwargs)


def publish_realtime(
    event,
    message=None,
    room=None,
    user=None,
    doctype=None,
    docname=None,
    task_id=None,
    after_commit=False,
):
    # Real Frappe passes dict messages; serialize for the Rust bridge.
    if message is None:
        message = {}
    if not isinstance(message, str):
        try:
            message = json.dumps(message)
        except Exception:
            message = str(message)

    if _rust is None:
        print(f"[REALTIME {event}] {message}")
        return
    _rust.publish_realtime(event, message, user, room)
