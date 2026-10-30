//! Document lifecycle hook helpers.
//!
//! These helpers turn Frappe document events into durable logs. The list of
//! DocTypes to observe lives with the app that owns those DocTypes; this crate
//! only provides the reusable hook factory so that all audit-trail logic is
//! centralized in one place.

use crate::logging::log_document_event;
use crate::{AppContext, DocEvent, DocHook};

/// Build a single audit hook that logs a document lifecycle event.
pub fn audit_hook(event: DocEvent, doctype: &'static str) -> DocHook {
    DocHook::new(event, doctype, move |ctx, doc| {
        log_document_event(ctx, event.as_str(), doc);
        spawn_document_history(ctx, event.as_str(), doc);
        Ok(())
    })
}

/// Build audit hooks for a list of DocTypes.
///
/// Registers `after_insert`, `on_update`, and `after_trash` hooks for each
/// supplied doctype name.
pub fn audit_hooks_for(doctypes: &[&'static str]) -> Vec<DocHook> {
    let mut hooks = Vec::new();

    for doctype in doctypes {
        hooks.push(audit_hook(DocEvent::AfterInsert, doctype));
        hooks.push(audit_hook(DocEvent::OnUpdate, doctype));
        hooks.push(audit_hook(DocEvent::AfterTrash, doctype));
    }

    hooks
}

/// Durable document history, written next to the short-lived log-engine
/// event. Rows land in the `document_history` table, which the ORM routes to
/// the `History` domain (`history.db` when sharding is enabled), so audit
/// writes never queue behind the core `site.db` writer.
///
/// Machine writers (agent/token calls — any `token:*` identity) are skipped:
/// their high-frequency handshake updates would fill the database, and they
/// already leave short-term events in the log engine. This is why the write
/// is spawned rather than awaited: the history row must never slow down or
/// break the save that triggered it.
fn spawn_document_history(ctx: &AppContext, event: &str, doc: &orm::Document) {
    let Some(user) = durable_history_user(doc) else {
        return;
    };
    let Some(pools) = ctx.state.pools.iter().next().map(|entry| entry.value().clone())
    else {
        return;
    };

    // Bundle-backed handle: insert_doc routes `document_history` to the
    // History domain automatically.
    let pool = pools.core();
    let now = chrono::Utc::now();
    let name = format!(
        "DH-{}-{}",
        now.format("%Y%m%d%H%M%S"),
        now.timestamp_subsec_nanos()
    );
    let event = event.to_string();
    let ref_doctype = doc.doctype.clone();
    let docname = doc.name.clone();
    let timestamp = now.to_rfc3339();

    tokio::spawn(async move {
        let mut row = orm::Document::new("Document History", name);
        row.set_field("event", event);
        row.set_field("ref_doctype", ref_doctype);
        row.set_field("docname", docname);
        row.set_field("user", user);
        row.set_field("timestamp", timestamp);
        if let Err(e) = pool.insert_doc(&row).await {
            tracing::warn!("failed to write document history: {}", e);
        }
    });
}

/// The acting user for a durable history row, or `None` when the change was
/// made by a machine writer (agent/token) and must stay out of the durable
/// store. Prefers `modified_by`, then the document `owner` field, then the
/// document struct's owner.
fn durable_history_user(doc: &orm::Document) -> Option<String> {
    let candidates = [
        doc.fields.get("modified_by").and_then(|v| v.as_str()),
        doc.fields.get("owner").and_then(|v| v.as_str()),
        Some(doc.owner.as_str()),
    ];
    for candidate in candidates.into_iter().flatten() {
        let user = candidate.trim();
        if user.is_empty() {
            continue;
        }
        if user.starts_with("token:") {
            return None;
        }
        return Some(user.to_string());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_writer_is_recorded() {
        let mut doc = orm::Document::new("ISMS Record", "ISMS-1");
        doc.set_field("modified_by", "admin@example.com");
        assert_eq!(
            durable_history_user(&doc).as_deref(),
            Some("admin@example.com")
        );
    }

    #[test]
    fn falls_back_to_owner_field_then_struct_owner() {
        let mut doc = orm::Document::new("ISMS Record", "ISMS-1");
        doc.set_field("owner", "jane@example.com");
        assert_eq!(durable_history_user(&doc).as_deref(), Some("jane@example.com"));

        let doc = orm::Document::new("ISMS Record", "ISMS-1");
        assert_eq!(durable_history_user(&doc).as_deref(), Some("Administrator"));
    }

    #[test]
    fn machine_writer_is_skipped() {
        let mut doc = orm::Document::new("Client Machine", "CM-1");
        doc.set_field("modified_by", "token:agent-7");
        assert_eq!(durable_history_user(&doc), None);

        let mut doc = orm::Document::new("Client Machine", "CM-1");
        doc.set_field("owner", "token:agent-7");
        assert_eq!(durable_history_user(&doc), None);
    }
}
