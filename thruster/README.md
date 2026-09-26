# Thruster local binary storage

These folders hold the raw Thruster binaries. The release itself is managed
through the **Thruster** tab in `/sebrus_apps/store`, which creates a
`Sebrus Store Release` document and exposes it at `/thruster/manifest.json`.

## Layout

```
thruster/
├── windows/   # Windows binaries / archives
├── macos/     # macOS binaries / archives
└── linux/     # Linux binaries / archives
```

## Endpoints

- `POST /api/method/thruster.upload` — upload a binary for a platform
  (multipart: `platform`, `file`). Requires Sebrus Admin / Release Manager,
  System Manager, or Administrator.
- `GET /thruster/:platform/:filename` — download a binary.
- `GET /thruster/manifest.json` — public Thruster release manifest.

## Naming tips

Thruster matches assets by filename keywords:
- Windows: `windows`, `win64`, `win32`, `win`
- macOS: `macos`, `darwin`, `osx`, `mac`
- Linux: `linux`

Arch keywords (`x86_64`, `amd64`, `arm64`, `aarch64`, etc.) are preferred but
not required.
