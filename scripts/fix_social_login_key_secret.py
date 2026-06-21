#!/usr/bin/env python3
"""
Manually insert / re-insert a Social Login Key client_secret into the __auth table.

Use this if the Desk form save does not store the password in __auth, or if the
site encryption_key was invalid when the key was saved.

The script will:
  1. Validate/regenerate the site encryption_key to a valid Fernet key.
  2. Encrypt the provided client_secret.
  3. Insert or replace the row in __auth for Social Login Key / office_365.

Run inside the Frappe Python environment (the same env that has the
`cryptography` package installed), e.g.:

    python3 scripts/fix_social_login_key_secret.py /opt/openfrappe/sites/localhost "YOUR_CLIENT_SECRET"

or for the local dev site:

    python3 scripts/fix_social_login_key_secret.py ./sites/localhost "YOUR_CLIENT_SECRET"
"""

import base64
import json
import os
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


def generate_fernet_key() -> str:
    return Fernet.generate_key().decode()


def load_or_fix_site_config(site_path: str) -> dict:
    config_path = Path(site_path) / "site_config.json"
    if not config_path.exists():
        raise FileNotFoundError(f"site_config.json not found at {config_path}")

    with open(config_path) as f:
        config = json.load(f)

    key = config.get("encryption_key", "")
    if not is_valid_fernet_key(key):
        print(f"[!] encryption_key is invalid ({key!r}); regenerating...")
        config["encryption_key"] = generate_fernet_key()
        with open(config_path, "w") as f:
            json.dump(config, f, indent=2)
            f.write("\n")
        print(f"[+] New encryption_key written to {config_path}")
    else:
        print(f"[+] encryption_key is valid")

    return config


def encrypt_secret(secret: str, key: str) -> str:
    cipher = Fernet(key.encode())
    return cipher.encrypt(secret.encode()).decode()


def ensure_auth_table(conn: sqlite3.Connection) -> None:
    conn.execute(
        '''CREATE TABLE IF NOT EXISTS "__auth" (
            name TEXT,
            doctype TEXT,
            fieldname TEXT,
            password TEXT,
            encrypted INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (name, doctype, fieldname)
        )'''
    )


def main():
    if len(sys.argv) < 3:
        print(f"Usage: {sys.argv[0]} <site_path> <client_secret>")
        sys.exit(1)

    site_path = sys.argv[1]
    client_secret = sys.argv[2]

    config = load_or_fix_site_config(site_path)
    encryption_key = config["encryption_key"]

    db_url = config.get("db_url", f"./sites/{Path(site_path).name}/site.db")
    db_path = Path(db_url)
    if not db_path.is_absolute():
        # Relative db_url is relative to the project root. Assume the script is
        # run from the project root, otherwise fall back to resolving against
        # the site path's parent (sites/).
        if not db_path.exists():
            db_path = (Path(site_path) / ".." / db_url.lstrip("./")).resolve()

    if not db_path.exists():
        raise FileNotFoundError(f"site database not found at {db_path}")

    print(f"[+] Using database: {db_path}")

    encrypted = encrypt_secret(client_secret, encryption_key)
    print(f"[+] Encrypted client_secret ({len(encrypted)} chars)")

    conn = sqlite3.connect(str(db_path))
    try:
        ensure_auth_table(conn)
        conn.execute(
            '''INSERT OR REPLACE INTO "__auth" (doctype, name, fieldname, password, encrypted)
               VALUES (?, ?, ?, ?, ?)''',
            ("Social Login Key", "office_365", "client_secret", encrypted, 1),
        )
        conn.commit()
        print("[+] Secret stored in __auth table")
    finally:
        conn.close()


if __name__ == "__main__":
    main()
