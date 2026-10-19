#!/usr/bin/env python3
"""Sync Cargo.toml workspace members/dependencies with rust_apps/apps.json.

Cargo cannot discover crates from a JSON file at build time, so this helper
script keeps the workspace manifest in sync with the active Rust app list.
Run it whenever you add or remove an app from rust_apps/apps.json.
"""

import json
import re
import sys
from pathlib import Path
from typing import Optional


WORKSPACE_ROOT = Path(__file__).resolve().parents[1]
APPS_JSON = WORKSPACE_ROOT / "rust_apps" / "apps.json"
ROOT_CARGO = WORKSPACE_ROOT / "Cargo.toml"
RUNTIME_CARGO = WORKSPACE_ROOT / "crates" / "runtime" / "Cargo.toml"

CORE_CRATES = [
    "crates/error",
    "crates/config",
    "crates/orm",
    "crates/sql-translator",
    "crates/python-bridge",
    "crates/permissions",
    "crates/session",
    "crates/queue",
    "crates/metadata",
    "crates/http",
    "crates/runtime",
    "crates/log_engine",
    "cli",
    "crates/kiff_logger",
    "crates/protocol",
]


def resolve_app_dir(app: str) -> Optional[str]:
    """Return the actual directory name under rust_apps/ for `app`.

    Returns ``None`` if the app directory is not present, so apps that are
    configured but not checked out are skipped rather than generating invalid
    path dependencies.
    """
    rust_apps_dir = WORKSPACE_ROOT / "rust_apps"
    if not rust_apps_dir.is_dir():
        return None
    for entry in rust_apps_dir.iterdir():
        if entry.is_dir() and entry.name.lower() == app.lower():
            return entry.name
    return None


def resolve_app_workspace_members(apps: list[str]) -> list[str]:
    """Return all workspace member paths for configured apps that are checked out.

    Includes the app crate itself and any nested crate directories directly
    underneath it that contain a ``Cargo.toml`` (e.g.
    ``rust_apps/strongroom/tauri``).
    """
    rust_apps_dir = WORKSPACE_ROOT / "rust_apps"
    if not rust_apps_dir.is_dir():
        return []

    app_dirs = {entry.name: entry for entry in rust_apps_dir.iterdir() if entry.is_dir()}

    members: list[str] = []
    for app in apps:
        dir_name = next(
            (name for name in app_dirs if name.lower() == app.lower()), None
        )
        if dir_name is None:
            continue
        members.append(f"rust_apps/{dir_name}")
        app_path = rust_apps_dir / dir_name
        for subdir in app_path.iterdir():
            if not subdir.is_dir():
                continue
            cargo_toml = subdir / "Cargo.toml"
            if not cargo_toml.is_file():
                continue
            # Skip nested crates that declare their own `[workspace]`; they
            # are built independently and must not be pulled into the main
            # monorepo workspace.
            if "[workspace]" in cargo_toml.read_text():
                continue
            members.append(f"rust_apps/{dir_name}/{subdir.name}")
    return members


def read_apps() -> list[str]:
    data = json.loads(APPS_JSON.read_text())
    apps = data.get("apps", [])
    if not isinstance(apps, list):
        raise ValueError("rust_apps/apps.json 'apps' must be a list of strings")
    return [str(a) for a in apps]


def replace_array_block(text: str, key: str, items: list[str]) -> str:
    """Replace a TOML array of strings, preserving indentation."""
    lines = items
    block = "[\n" + "".join(f'    "{item}",\n' for item in lines) + "]"
    pattern = rf"({re.escape(key)}\s*=\s*)\[[^\]]*\]"
    match = re.search(pattern, text, flags=re.DOTALL)
    if not match:
        raise ValueError(f"Could not find '{key} = [...]' in {ROOT_CARGO}")
    return text[: match.start()] + match.group(1) + block + text[match.end() :]


def replace_dependencies_table(text: str, section: str, deps: dict[str, str]) -> str:
    """Replace entries for Rust apps inside a [dependencies]-style table.

    Only app crates (with path under rust_apps/) are managed; other entries are
    left untouched.
    """
    # Match app crates: the crate name must match the final directory in the
    # rust_apps/ path. This excludes rust_apps_core, whose path is
    # rust_apps/core but crate name is rust_apps_core.
    app_pattern = re.compile(
        r'^(?P<name>[a-zA-Z0-9_]+)\s*=\s*\{\s*path\s*=\s*"(?:\.\./\.\./)?rust_apps/(?P=name)"\s*\}\s*$',
        re.MULTILINE | re.IGNORECASE,
    )

    # Remove existing app entries (and any blank lines they leave behind).
    cleaned = app_pattern.sub("", text)
    cleaned = re.sub(r"\n{3,}", "\n\n", cleaned)

    # Locate the target section header
    section_re = re.compile(rf"^\[{re.escape(section)}\]\s*$", re.MULTILINE)
    match = section_re.search(cleaned)
    if not match:
        raise ValueError(f"Could not find '[{section}]' table")

    # Insert right after the section header line, before the next non-empty line.
    header_end = match.end()
    rest = cleaned[header_end:]
    while rest.startswith("\n"):
        rest = rest[1:]
        header_end += 1

    max_name_len = max((len(name) for name in deps), default=0)
    dep_lines = ""
    for name, path in deps.items():
        dep_lines += f'{name.ljust(max_name_len)} = {{ path = "{path}" }}\n'

    return cleaned[:header_end] + dep_lines + rest


def sync_root_cargo(apps: list[str]) -> None:
    text = ROOT_CARGO.read_text()

    # Workspace members: core crates + rust_apps/core + each configured app
    # that is actually checked out, including nested crates such as
    # rust_apps/strongroom/tauri.
    app_members = resolve_app_workspace_members(apps)
    members = CORE_CRATES + ["rust_apps/core"] + app_members
    text = replace_array_block(text, "members", members)

    # Dev-dependencies: keep rust_apps_core and any configured apps that are
    # actually checked out.
    app_dev_deps = {
        a: f"rust_apps/{d}"
        for a in apps
        if (d := resolve_app_dir(a)) is not None
    }
    text = replace_dependencies_table(text, "dev-dependencies", app_dev_deps)

    ROOT_CARGO.write_text(text)


def sync_runtime_cargo(apps: list[str]) -> None:
    text = RUNTIME_CARGO.read_text()
    app_deps = {
        a: f"../../rust_apps/{d}"
        for a in apps
        if (d := resolve_app_dir(a)) is not None
    }
    text = replace_dependencies_table(text, "dependencies", app_deps)
    RUNTIME_CARGO.write_text(text)


def main() -> int:
    apps = read_apps()
    print(f"Active Rust apps from {APPS_JSON}: {apps}")

    sync_root_cargo(apps)
    print(f"Updated {ROOT_CARGO}")

    sync_runtime_cargo(apps)
    print(f"Updated {RUNTIME_CARGO}")

    print("Done. Run 'cargo check' to verify.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
