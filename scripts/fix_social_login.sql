-- Run this directly against the site database.
-- This project uses SQLite, so the table name is `social_login_key`.
-- If you are on MariaDB, change it to `tabSocial Login Key`.
--
-- SQLite:
--   sqlite3 sites/localhost/site.db < scripts/fix_social_login.sql
--
-- MariaDB:
--   mysql -u root -p <database_name> < scripts/fix_social_login.sql

UPDATE "social_login_key"
SET redirect_url = 'https://logs.sebrus.dev/api/method/frappe.integrations.oauth2_logins.login_via_office365'
WHERE name = 'Office 365'
  AND redirect_url LIKE '%login_via_office365%redirect_url%';

-- To delete an old/duplicate key instead, uncomment and edit the line below:
-- DELETE FROM "social_login_key" WHERE name = 'Old Office 365';
-- DELETE FROM "__auth" WHERE doctype = 'Social Login Key' AND name = 'Old Office 365';
