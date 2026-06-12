use sql_translator::{SqlTranslator, TargetDialect};

#[test]
fn test_percent_s_to_dollar() {
    let t = SqlTranslator::new(TargetDialect::Postgres);
    let out = t.translate("SELECT * FROM tabUser WHERE name = %s AND email = %s").unwrap();
    assert!(out.contains("$1"), "output: {}", out);
    assert!(out.contains("$2"), "output: {}", out);
}

#[test]
fn test_percent_s_to_question() {
    let t = SqlTranslator::new(TargetDialect::Sqlite);
    let out = t.translate("SELECT * FROM tabUser WHERE name = %s").unwrap();
    assert!(out.contains("?"), "output: {}", out);
    assert!(!out.contains("%s"), "output: {}", out);
}
