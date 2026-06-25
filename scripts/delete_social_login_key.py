#!/usr/bin/env python3
"""
Delete Social Login Key docs directly from the site database.

Use this when the Desk UI refuses to delete old / duplicate OAuth keys.

Usage:

    # List all keys (no deletion)
    python3 scripts/delete_social_login_key.py ./sites/localhost --list

    # Delete one or more keys by name
    python3 scripts/delete_social_login_key.py ./sites/localhost "Office 365" "Google"

    # Delete every key without prompting
    python3 scripts/delete_social_login_key.py ./sites/localhost --all --yes
"""

import argparse
import json
import sqlite3
import sys
from pathlib import Path


def load_site_config(site_path: str) -> dict:
    config_path = Path(site_path) / "site_config.json"
    if not config_path.exists():
        raise FileNotFoundError(f"site_config.json not found at {config_path}")
    with open(config_path) as f:
        return json.load(f)


def resolve_db_path(site_path: str, config: dict) -> Path:
    db_url = config.get("db_url", f"./sites/{Path(site_path).name}/site.db")
    db_path = Path(db_url)
    if not db_path.is_absolute():
        if not db_path.exists():
            db_path = (Path(site_path) / ".." / db_url.lstrip("./")).resolve()
    if not db_path.exists():
        raise FileNotFoundError(f"site database not found at {db_path}")
    return db_path


def list_keys(conn: sqlite3.Connection):
    rows = conn.execute(
        'SELECT name, social_login_provider, redirect_url, enable_social_login FROM "social_login_key"'
    ).fetchall()
    if not rows:
        print("No Social Login Keys found.")
        return rows
    print("\nSocial Login Keys:")
    for name, provider, redirect_url, enabled in rows:
        print(f"  - name={name!r}, provider={provider!r}, enabled={enabled}")
        print(f"    redirect_url={redirect_url!r}")
    return rows


def delete_keys(conn: sqlite3.Connection, names: list[str], dry_run: bool):
    cursor = conn.cursor()
    for name in names:
        print(f"[{'D' if not dry_run else 'd'}] {'would delete' if dry_run else 'deleting'} Social Login Key: {name!r}")
        if dry_run:
            continue
        cursor.execute('DELETE FROM "social_login_key" WHERE name = ?', (name,))
        cursor.execute(
            'DELETE FROM "__auth" WHERE doctype = ? AND name = ?',
            ("Social Login Key", name),
        )
    if not dry_run:
        conn.commit()
        print("[+] Database updated. Restart the Frappe bench / clear cache for changes to take effect.")


def main():
    parser = argparse.ArgumentParser(description="Delete Social Login Keys from the site database")
    parser.add_argument("site_path", help="Path to the Frappe site directory")
    parser.add_argument("names", nargs="*", help="Name(s) of the Social Login Key(s) to delete")
    parser.add_argument("--list", action="store_true", help="List keys and exit")
    parser.add_argument("--all", action="store_true", help="Delete all keys (requires --yes)")
    parser.add_argument("--yes", action="store_true", help="Skip confirmation prompt")
    parser.add_argument("--dry-run", action="store_true", help="Show what would be deleted without deleting")
    args = parser.parse_args()

    config = load_site_config(args.site_path)
    db_path = resolve_db_path(args.site_path, config)
    print(f"[+] Using database: {db_path}")

    conn = sqlite3.connect(str(db_path))
    try:
        rows = list_keys(conn)

        if args.list:
            return

        if args.all:
            names = [r[0] for r in rows]
            if not names:
                print("Nothing to delete.")
                return
        else:
            names = args.names
            if not names:
                parser.error("Provide key names to delete, or use --all")

        unknown = set(names) - {r[0] for r in rows}
        if unknown:
            print(f"[!] Unknown key names: {sorted(unknown)}")
            sys.exit(1)

        if not args.yes and not args.dry_run:
            confirm = input(f"Delete {len(names)} key(s)? [y/N] ")
            if confirm.lower() not in ("y", "yes"):
                print("Aborted.")
                return

        delete_keys(conn, names, args.dry_run)

        if not args.dry_run:
            print("\nRemaining keys:")
            list_keys(conn)
    finally:
        conn.close()


if __name__ == "__main__":
    main()
