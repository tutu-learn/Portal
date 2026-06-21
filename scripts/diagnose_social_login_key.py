#!/usr/bin/env python3
"""
Diagnose Social Login Key client_secret encryption issues.

Run inside the Frappe Python environment, e.g.:

    python3 scripts/diagnose_social_login_key.py /opt/openfrappe/sites/localhost

or for the local dev site:

    python3 scripts/diagnose_social_login_key.py ./sites/localhost
"""

import base64
import json
import sqlite3
import sys
from pathlib import Path

from cryptography.fernet import Fernet, InvalidToken


def is_valid_fernet_key(key: str) -> bool:
    try:
        decoded = base64.urlsafe_b64decode(key)
        return len(decoded) == 32
    except Exception:
        return False


def main():
    if len(sys.argv) < 2:
        print(f"Usage: {sys.argv[0]} <site_path>")
        sys.exit(1)

    site_path = sys.argv[1]
    config_path = Path(site_path) / "site_config.json"

    if not config_path.exists():
        raise FileNotFoundError(f"site_config.json not found at {config_path}")

    with open(config_path) as f:
        config = json.load(f)

    encryption_key = config.get("encryption_key", "")
    print(f"site_config.json: {config_path}")
    print(f"encryption_key: {encryption_key!r}")
    print(f"encryption_key valid: {is_valid_fernet_key(encryption_key)}")

    db_url = config.get("db_url", f"./sites/{Path(site_path).name}/site.db")
    db_path = Path(db_url)
    if not db_path.is_absolute():
        if not db_path.exists():
            db_path = (Path(site_path) / ".." / db_url.lstrip("./")).resolve()

    if not db_path.exists():
        raise FileNotFoundError(f"site database not found at {db_path}")

    print(f"database: {db_path}")
    conn = sqlite3.connect(str(db_path))

    print("\n--- Social Login Keys ---")
    rows = conn.execute(
        'SELECT name, social_login_provider, enable_social_login FROM "social_login_key"'
    ).fetchall()
    if not rows:
        print("No Social Login Keys found.")
    for name, provider, enabled in rows:
        print(f"  name={name!r}, provider={provider!r}, enable_social_login={enabled}")

    print("\n--- __auth rows for Social Login Key ---")
    rows = conn.execute(
        """SELECT name, fieldname, password, encrypted FROM "__auth"
           WHERE doctype = 'Social Login Key'"""
    ).fetchall()
    if not rows:
        print("No __auth rows found for Social Login Key.")

    cipher = Fernet(encryption_key.encode()) if is_valid_fernet_key(encryption_key) else None

    for name, fieldname, password, encrypted in rows:
        print(f"\n  name={name!r}, fieldname={fieldname!r}, encrypted={encrypted}")
        print(f"  stored value: {password!r}")
        if not cipher:
            print("  -> cannot decrypt: encryption_key is invalid")
            continue
        if not password:
            print("  -> empty password value")
            continue
        if not encrypted:
            print("  -> not encrypted (encrypted=0)")
            continue
        try:
            decrypted = cipher.decrypt(password.encode()).decode()
            masked = decrypted[:4] + "***" + decrypted[-4:] if len(decrypted) > 8 else "***"
            print(f"  -> decrypts OK: {masked}")
        except InvalidToken:
            print("  -> DECRYPT FAILED: InvalidToken (wrong encryption key)")

    conn.close()


if __name__ == "__main__":
    main()
