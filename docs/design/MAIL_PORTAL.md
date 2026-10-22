# Mail Portal UX

The mail portal is a single-page app served at `/sebrus_mail/portal`.

## Auth behaviour

- The page is **always rendered**, even without a session.
- A login overlay is shown when any API call returns `401`.
- Credentials are SebrusMail Account credentials (email + password), the same credentials used for IMAP/SMTP.
- Successful login sets a `sebrus_mail_sid` cookie and hides the overlay.

## Main views

- **Inbox message list** — subject, sender, date, read/unread state.
- **Message detail / raw download** — `.eml` download via `/sebrus_mail/api/messages/:uid/raw`.
- **Compose** — to, subject, body; saved to Sent.

## Actions

- Refresh, Compose, Mark read, Send, Logout.
