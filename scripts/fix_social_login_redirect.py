#!/usr/bin/env python3
"""
Fix malformed redirect_url values on Social Login Key docs.

The Desk UI occasionally concatenates placeholder text like " redirect_url"
onto the redirect URL, which breaks OAuth token exchange with providers such
as Microsoft / Office 365.

Usage:

    # Auto-clean every Social Login Key in the local dev site
    python3 scripts/fix_social_login_redirect.py ./sites/localhost

    # Set a specific clean URL for the "Office 365" key on the production site
    python3 scripts/fix_social_login_redirect.py /opt/openfrappe/sites/localhost \
        --name "Office 365" \
        --url "https://logs.sebrus.dev/api/method/frappe.integrations.oauth2_logins.login_via_office365"

    # Preview changes without writing to the database
    python3 scripts/fix_social_login_redirect.py ./sites/localhost --dry-run
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


def clean_redirect_url(url: str) -> str:
    """Strip trailing placeholder text / whitespace that providers reject."""
    url = str(url or "").strip()
    # Remove any trailing word that looks like a field label added by mistake.
    for suffix in (" redirect_url", "redirect_url", " redirect_uri", "redirect_uri"):
        if url.lower().endswith(suffix):
            url = url[: -len(suffix)].rstrip()
    return url


def list_keys(conn: sqlite3.Connection):
    rows = conn.execute(
        'SELECT name, social_login_provider, redirect_url, enable_social_login FROM "social_login_key"'
    ).fetchall()
    if not rows:
        print("No Social Login Keys found.")
        return []
    print("\nSocial Login Keys:")
    for name, provider, redirect_url, enabled in rows:
        print(f"  - name={name!r}, provider={provider!r}, enabled={enabled}")
        print(f"    redirect_url={redirect_url!r}")
    return rows


def update_key(conn: sqlite3.Connection, name: str | None, url: str | None, dry_run: bool):
    cursor = conn.cursor()
    if name:
        rows = conn.execute(
            'SELECT name, redirect_url FROM "social_login_key" WHERE name = ?', (name,)
        ).fetchall()
    else:
        rows = conn.execute(
            'SELECT name, redirect_url FROM "social_login_key"'
        ).fetchall()

    if not rows:
        print(f"[!] No Social Login Key matched name={name!r}")
        return

    for key_name, current_url in rows:
        new_url = url if url is not None else clean_redirect_url(current_url)
        if new_url == current_url:
            print(f"[ ] {key_name}: no change ({current_url!r})")
            continue
        action = "would update" if dry_run else "updating"
        print(f"[{action[0].upper()}] {key_name}: {current_url!r} -> {new_url!r}")
        if not dry_run:
            cursor.execute(
                'UPDATE "social_login_key" SET redirect_url = ? WHERE name = ?',
                (new_url, key_name),
            )
    if not dry_run:
        conn.commit()
        print("[+] Database updated. Restart the Frappe bench / clear cache for changes to take effect.")


def main():
    parser = argparse.ArgumentParser(description="Fix Social Login Key redirect_url values")
    parser.add_argument("site_path", help="Path to the Frappe site directory")
    parser.add_argument("--name", help="Only act on the named Social Login Key")
    parser.add_argument("--url", help="Set redirect_url to this exact value instead of auto-cleaning")
    parser.add_argument("--list", action="store_true", help="List keys and exit")
    parser.add_argument("--dry-run", action="store_true", help="Show changes without writing")
    args = parser.parse_args()

    config = load_site_config(args.site_path)
    db_path = resolve_db_path(args.site_path, config)
    print(f"[+] Using database: {db_path}")

    conn = sqlite3.connect(str(db_path))
    try:
        if args.list:
            list_keys(conn)
            return
        update_key(conn, args.name, args.url, args.dry_run)
        print("\nCurrent keys after update:")
        list_keys(conn)
    finally:
        conn.close()


if __name__ == "__main__":
    main()
