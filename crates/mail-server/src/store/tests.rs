#[cfg(test)]
mod tests {
    use crate::store::{MailStore, OrmMailStore, SearchCriteria};
    use orm::DatabasePool;

    async fn setup_db() -> (DatabasePool, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        let pool = DatabasePool::connect_sqlite(path.to_str().unwrap()).await.unwrap();
        pool.execute_sql(
            r#"CREATE TABLE "sebrusmail_domain" (
                name TEXT PRIMARY KEY,
                enabled INTEGER,
                dkim_selector TEXT,
                dkim_private_key TEXT,
                catch_all_account TEXT,
                is_default INTEGER
            )"#,
            vec![],
        )
        .await
        .unwrap();
        pool.execute_sql(
            r#"CREATE TABLE "sebrusmail_account" (
                name TEXT PRIMARY KEY,
                email_address TEXT UNIQUE,
                username TEXT,
                domain TEXT,
                password_hash TEXT,
                quota_bytes INTEGER,
                enabled INTEGER,
                is_domain_admin INTEGER
            )"#,
            vec![],
        )
        .await
        .unwrap();
        pool.execute_sql(
            r#"CREATE TABLE "sebrusmail_alias" (
                name TEXT PRIMARY KEY,
                address TEXT,
                domain TEXT,
                destination_accounts TEXT,
                enabled INTEGER
            )"#,
            vec![],
        )
        .await
        .unwrap();
        pool.execute_sql(
            r#"CREATE TABLE "sebrusmail_folder" (
                name TEXT,
                account TEXT,
                uidvalidity INTEGER,
                uidnext INTEGER,
                PRIMARY KEY (name, account)
            )"#,
            vec![],
        )
        .await
        .unwrap();
        pool.execute_sql(
            r#"CREATE TABLE "sebrusmail_message" (
                name TEXT PRIMARY KEY,
                account TEXT,
                folder TEXT,
                uid INTEGER,
                message_id TEXT,
                subject TEXT,
                "from" TEXT,
                "to" TEXT,
                cc TEXT,
                date TEXT,
                size_bytes INTEGER,
                flags TEXT,
                raw_eml TEXT,
                body_text TEXT,
                is_read INTEGER
            )"#,
            vec![],
        )
        .await
        .unwrap();
        (pool, dir)
    }

    fn hash_password(password: &str) -> String {
        use argon2::{password_hash::SaltString, Argon2, PasswordHasher};
        use rand::rngs::OsRng;
        let salt = SaltString::generate(&mut OsRng);
        Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .unwrap()
            .to_string()
    }

    #[tokio::test]
    async fn resolve_domain_and_recipient() {
        let (pool, _dir) = setup_db().await;
        let store = OrmMailStore::new(pool);

        store
            .pool
            .execute_sql(
                r#"INSERT INTO "sebrusmail_domain" (name, enabled, is_default) VALUES ('example.com', 1, 1)"#,
                vec![],
            )
            .await
            .unwrap();
        store
            .pool
            .execute_sql(
                r#"INSERT INTO "sebrusmail_account" (name, email_address, domain, password_hash, quota_bytes, enabled, is_domain_admin)
                   VALUES ('alice@example.com', 'alice@example.com', 'example.com', '', 0, 1, 0)"#,
                vec![],
            )
            .await
            .unwrap();

        let domain = store.resolve_domain("example.com").await.unwrap();
        assert!(domain.is_some());

        let targets = store.resolve_recipient("alice@example.com").await.unwrap();
        assert_eq!(targets, vec!["alice@example.com"]);

        let targets = store.resolve_recipient("nobody@example.com").await.unwrap();
        assert!(targets.is_empty());
    }

    #[tokio::test]
    async fn catch_all_recipient() {
        let (pool, _dir) = setup_db().await;
        let store = OrmMailStore::new(pool);

        store
            .pool
            .execute_sql(
                r#"INSERT INTO "sebrusmail_domain" (name, enabled, catch_all_account) VALUES ('example.com', 1, 'admin')"#,
                vec![],
            )
            .await
            .unwrap();
        store
            .pool
            .execute_sql(
                r#"INSERT INTO "sebrusmail_account" (name, email_address, domain, password_hash, quota_bytes, enabled, is_domain_admin)
                   VALUES ('admin@example.com', 'admin@example.com', 'example.com', '', 0, 1, 0)"#,
                vec![],
            )
            .await
            .unwrap();

        let targets = store.resolve_recipient("anything@example.com").await.unwrap();
        assert_eq!(targets, vec!["admin@example.com"]);
    }

    #[tokio::test]
    async fn authenticate_account() {
        let (pool, _dir) = setup_db().await;
        let store = OrmMailStore::new(pool);

        store
            .pool
            .execute_sql(
                r#"INSERT INTO "sebrusmail_domain" (name, enabled, is_default) VALUES ('example.com', 1, 1)"#,
                vec![],
            )
            .await
            .unwrap();
        store
            .pool
            .execute_sql(
                r#"INSERT INTO "sebrusmail_account" (name, email_address, domain, password_hash, quota_bytes, enabled, is_domain_admin)
                   VALUES ('bob@example.com', 'bob@example.com', 'example.com', ?, 0, 1, 0)"#,
                vec![hash_password("secret").into()],
            )
            .await
            .unwrap();

        let account = store.authenticate("bob@example.com", "secret").await.unwrap();
        assert!(account.is_some());

        let account = store.authenticate("bob@example.com", "wrong").await.unwrap();
        assert!(account.is_none());
    }

    #[tokio::test]
    async fn append_and_fetch_message() {
        let (pool, _dir) = setup_db().await;
        let store = OrmMailStore::new(pool);

        store
            .pool
            .execute_sql(
                r#"INSERT INTO "sebrusmail_domain" (name, enabled, is_default) VALUES ('example.com', 1, 1)"#,
                vec![],
            )
            .await
            .unwrap();
        store
            .pool
            .execute_sql(
                r#"INSERT INTO "sebrusmail_account" (name, email_address, domain, password_hash, quota_bytes, enabled, is_domain_admin)
                   VALUES ('carol@example.com', 'carol@example.com', 'example.com', '', 0, 1, 0)"#,
                vec![],
            )
            .await
            .unwrap();

        let raw = b"From: sender@example.com\r\nTo: carol@example.com\r\nSubject: Hello\r\n\r\nBody";
        let uid = store.append_message("carol@example.com", "INBOX", raw).await.unwrap();
        assert_eq!(uid, 1);

        let uids = store
            .search_messages("carol@example.com", "INBOX", &SearchCriteria::All)
            .await
            .unwrap();
        assert_eq!(uids, vec![1]);

        let mut q = crate::store::FetchQuery::default();
        q.rfc822 = true;
        let msgs = store
            .fetch_messages("carol@example.com", "INBOX", &uids, &q)
            .await
            .unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].subject, "Hello");

        let folder = store
            .select_folder("carol@example.com", "INBOX")
            .await
            .unwrap();
        assert_eq!(folder.exists, 1);
        assert_eq!(folder.unseen, 1);
    }
}
