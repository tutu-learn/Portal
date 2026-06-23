use sqlparser::ast::{Ident, ObjectName};

pub fn rewrite_name(name: &mut ObjectName) {
    for ident in &mut name.0 {
        rewrite_ident(ident);
    }
}

fn rewrite_ident(ident: &mut Ident) {
    let raw = ident.value.clone();
    // Strip backticks (sqlparser may already do this, but we ensure it)
    let cleaned = raw.trim_matches('`').to_string();
    // Strip tab prefix (case-insensitive)
    let cleaned = if cleaned.to_lowercase().starts_with("tab") {
        cleaned[3..].to_string()
    } else {
        cleaned
    };
    // Lowercase and spaces to underscores
    let cleaned = cleaned.to_lowercase().replace(" ", "_");
    ident.value = cleaned;
    ident.quote_style = None;
}
