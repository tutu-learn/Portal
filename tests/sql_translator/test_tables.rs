use sql_translator::{SqlTranslator, TargetDialect};

#[test]
fn test_strip_tab_prefix() {
    let t = SqlTranslator::new(TargetDialect::Sqlite);
    let out = t.translate("SELECT * FROM `tabJournal Entry`").unwrap();
    assert!(out.contains("journal_entry"), "output: {}", out);
}

#[test]
fn test_lowercase_and_underscore() {
    let t = SqlTranslator::new(TargetDialect::Sqlite);
    let out = t.translate("SELECT * FROM `tabSales Invoice`").unwrap();
    assert!(out.contains("sales_invoice"), "output: {}", out);
}
