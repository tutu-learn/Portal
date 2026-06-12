use sql_translator::{SqlTranslator, TargetDialect};

#[test]
fn test_ifnull_to_coalesce() {
    let t = SqlTranslator::new(TargetDialect::Postgres);
    let out = t.translate("SELECT IFNULL(amount, 0) FROM tabInvoice").unwrap();
    assert!(out.contains("COALESCE"), "output: {}", out);
}

#[test]
fn test_now_to_current_timestamp() {
    let t = SqlTranslator::new(TargetDialect::Sqlite);
    let out = t.translate("SELECT NOW()").unwrap();
    assert!(out.contains("CURRENT_TIMESTAMP"), "output: {}", out);
}
