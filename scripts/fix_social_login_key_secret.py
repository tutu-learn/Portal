#!/usr/bin/env python3
"""
Manually insert / re-insert a Social Login Key client_secret into the __auth table.

Use this if the Desk form save does not store the password in __auth, or if the
site encryption_key was invalid when the key was saved.

The script will:
  1. Validate/regenerate the site encryption_key to a valid Fernet key.
  2. Look up the Social Login Key name (auto-detect by provider, or use the
     name you supply).
  3. Encrypt the provided client_secret.
  4. Insert or replace the row in __auth for Social Login Key / <key_name>.

Run inside the Frappe Python environment (the same env that has the
`cryptography` package installed), e.g.:

    python3 scripts/fix_social_login_key_secret.py /opt/openfrappe/sites/localhost "YOUR_CLIENT_SECRET"

To target a specific Social Login Key by name (e.g. "microsoft"):

    python3 scripts/fix_social_login_key_secret.py /opt/openfrappe/sites/localhost "YOUR_CLIENT_SECRET" microsoft

or for the local dev site:

    python3 scripts/fix_social_login_key_secret.py ./sites/localhost "YOUR_CLIENT_SECRET"
"""

import base64
import json
import sqlite3
import sys
from pathlib import Path

from cryptography.fernet import Fernet


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

    # In clustered / stateless deployments the encryption key should be pinned
    # via the FRAPPE_ENCRYPTION_KEY environment variable so it survives
    # redeploys. If set and valid, prefer it and persist it to site_config.json.
    env_key = os.environ.get("FRAPPE_ENCRYPTION_KEY", "")
    if env_key:
        if is_valid_fernet_key(env_key):
            if config.get("encryption_key") != env_key:
                print("[+] Using FRAPPE_ENCRYPTION_KEY from environment")
                config["encryption_key"] = env_key
                with open(config_path, "w") as f:
                    json.dump(config, f, indent=2)
                    f.write("\n")
                print(f"[+] encryption_key written to {config_path}")
            else:
                print(f"[+] encryption_key matches FRAPPE_ENCRYPTION_KEY")
            return config
        else:
            print(f"[!] FRAPPE_ENCRYPTION_KEY is invalid; falling back to site_config.json")

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


def scrub(txt: str) -> str:
    return str(txt).lower().replace(" ", "_").replace("-", "_")


def resolve_key_name(conn: sqlite3.Connection, provider_slug: str, explicit_name: str | None) -> str:
    if explicit_name:
        print(f"[+] Using explicit Social Login Key name: {explicit_name}")
        return explicit_name

    # Try to find the key by provider slug. Frappe stores the chosen provider in
    # social_login_provider (e.g. "Office 365") and the key name in `name`.
    cursor = conn.execute(
        """SELECT name, social_login_provider FROM "social_login_key"
           WHERE social_login_provider IS NOT NULL"""
    )
    matches = []
    for name, provider in cursor.fetchall():
        if scrub(provider) == provider_slug or scrub(name) == provider_slug:
            matches.append((name, provider))

    if not matches:
        print(f"[!] No Social Login Key found for provider '{provider_slug}'")
        print(f"[!] Falling back to default name '{provider_slug}'")
        return provider_slug

    if len(matches) == 1:
        name, provider = matches[0]
        print(f"[+] Found Social Login Key: name={name!r}, provider={provider!r}")
        return name

    print("[!] Multiple Social Login Keys matched:")
    for name, provider in matches:
        print(f"    - name={name!r}, provider={provider!r}")
    print(f"[!] Using the first match: {matches[0][0]!r}")
    return matches[0][0]


def main():
    if len(sys.argv) < 3:
        print(f"Usage: {sys.argv[0]} <site_path> <client_secret> [key_name]")
        sys.exit(1)

    site_path = sys.argv[1]
    client_secret = sys.argv[2]
    explicit_key_name = sys.argv[3] if len(sys.argv) > 3 else None

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

    conn = sqlite3.connect(str(db_path))
    try:
        ensure_auth_table(conn)

        key_name = resolve_key_name(conn, "office_365", explicit_key_name)

        encrypted = encrypt_secret(client_secret, encryption_key)
        print(f"[+] Encrypted client_secret ({len(encrypted)} chars)")

        conn.execute(
            '''INSERT OR REPLACE INTO "__auth" (doctype, name, fieldname, password, encrypted)
               VALUES (?, ?, ?, ?, ?)''',
            ("Social Login Key", key_name, "client_secret", encrypted, 1),
        )
        conn.commit()
        print(f"[+] Secret stored in __auth table for Social Login Key / {key_name}")
    finally:
        conn.close()


if __name__ == "__main__":
    main()
