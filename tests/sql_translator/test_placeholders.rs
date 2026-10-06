use sql_translator::{SqlTranslator, TargetDialect};

#[test]
fn test_percent_s_to_question() {
    let t = SqlTranslator::new(TargetDialect::Sqlite);
    let out = t
        .translate("SELECT * FROM tabUser WHERE name = %s")
        .unwrap();
    assert!(out.contains("?"), "output: {}", out);
    assert!(!out.contains("%s"), "output: {}", out);
}
