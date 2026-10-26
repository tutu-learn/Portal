# Admin Portal UX

The admin portal is served at `/sebrus_mail/admin` and is protected by the framework `sid` session.

## Access control

- Requires one of these roles: `Sebrus Mail Admin`, `System Manager`, or `Administrator`.
- If the framework session is missing, the user is redirected to `/login?redirect-to=%2Fsebrus_mail%2Fadmin`.

## Sections

- **Domains** — add/delete domains, mark default, generate DKIM keys.
- **Accounts** — create/delete SebrusMail accounts tied to an enabled domain.

## DKIM key generation

- Triggered per domain.
- Produces a DKIM selector/key pair.
- DKIM DNS record is shown in the UI for manual DNS configuration.
- TLS is terminated by the ingress controller; the app does not manage TLS certificates.
