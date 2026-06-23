//! Document lifecycle hook helpers.
//!
//! These helpers turn Frappe document events into durable logs. The list of
//! DocTypes to observe lives with the app that owns those DocTypes; this crate
//! only provides the reusable hook factory so that all audit-trail logic is
//! centralized in one place.

use crate::logging::log_document_event;
use crate::{DocEvent, DocHook};

/// Build a single audit hook that logs a document lifecycle event.
pub fn audit_hook(event: DocEvent, doctype: &'static str) -> DocHook {
    DocHook::new(event, doctype, move |ctx, doc| {
        log_document_event(ctx, event.as_str(), doc);
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
