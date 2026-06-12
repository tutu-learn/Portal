"""Messaging, logging, queue, and realtime helpers."""

import inspect

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


def msgprint(msg):
    print(f"[MSG] {msg}")


def log_error(title, message=None):
    print(f"[ERROR: {title}] {message or ''}")


def enqueue(method, queue="default", **kwargs):
    if _rust is None:
        print(f"[ENQUEUE {queue}] {method} {kwargs}")
        return
    _rust.enqueue(method, queue, kwargs)


def publish_realtime(event, message, user=None, room=None):
    if _rust is None:
        print(f"[REALTIME {event}] {message}")
        return
    _rust.publish_realtime(event, message, user, room)
