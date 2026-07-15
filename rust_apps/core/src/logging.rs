//! Central log sink used by the tracing layer and document hooks.
//!
//! The sink is a plain `std::sync::mpsc` channel so that synchronous callers
//! (tracing layers, document hooks) can hand records to the single async
//! consumer spawned by the app that initializes the log engine. Before the sink
//! is initialized, log records are silently dropped; this covers the brief
//! window between tracing initialization and the log engine startup.

use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::OnceLock;

use log_engine::LogRecord;

use crate::AppContext;

static LOG_SINK: OnceLock<Sender<LogRecord>> = OnceLock::new();

/// Initialize the global log sink and return the consumer side.
///
/// Must be called once during application startup. Subsequent calls return a
/// fresh receiver but leave the original sender in place.
pub fn init_log_sink() -> Receiver<LogRecord> {
    let (tx, rx) = channel();
    let _ = LOG_SINK.set(tx);
    rx
}

/// Send a record to the sink if the sink is initialized.
pub fn log(rec: LogRecord) -> bool {
    if let Some(tx) = LOG_SINK.get() {
        tx.send(rec).is_ok()
    } else {
        false
    }
}

/// Spawn a background task that forwards records from the sink receiver into
/// the supplied async `LogService` in batches.
///
/// Instead of one blocking task per record, we drain the channel in chunks of
/// up to 100 records. This cuts blocking-pool churn roughly 100x and lets the
/// log engine ingest many records under a single mutex acquisition.
pub fn spawn_log_sink_consumer(logger: log_engine::LogService, log_rx: Receiver<LogRecord>) {
    let log_rx = std::sync::Arc::new(std::sync::Mutex::new(log_rx));
    const BATCH_SIZE: usize = 100;
    const DRAIN_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(25);

    tokio::spawn(async move {
        loop {
            let log_rx = std::sync::Arc::clone(&log_rx);
            let batch = match tokio::task::spawn_blocking(move || {
                let rx = log_rx.lock().unwrap();
                let mut batch = Vec::with_capacity(BATCH_SIZE);
                // Block until at least one record is available.
                match rx.recv() {
                    Ok(rec) => batch.push(rec),
                    Err(_) => return batch,
                }
                // Pull in any records that arrived while we were waking up.
                while batch.len() < BATCH_SIZE {
                    match rx.recv_timeout(DRAIN_TIMEOUT) {
                        Ok(rec) => batch.push(rec),
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout)
                        | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                }
                batch
            })
            .await
            {
                Ok(batch) => batch,
                Err(_) => break,
            };

            if batch.is_empty() {
                break;
            }

            if let Err(e) = logger.ingest_batch(batch).await {
                tracing::debug!("log sink ingest failed: {}", e);
            }
        }
    });
}

/// Build a log record for a Frappe document lifecycle event.
pub fn log_document_event(ctx: &AppContext, event: &str, doc: &orm::Document) {
    let message = format!("{} {} {}", event, doc.doctype, doc.name);
    let mut rec = LogRecord::new("INFO", "frappe.doc_event", &message);
    rec.fields
        .insert("app".into(), ctx.app_name.to_string().into());
    rec.fields
        .insert("doctype".into(), doc.doctype.clone().into());
    rec.fields.insert("docname".into(), doc.name.clone().into());
    rec.fields.insert("owner".into(), doc.owner.clone().into());
    rec.fields
        .insert("docstatus".into(), (doc.docstatus as i64).into());
    rec.fields.insert("event".into(), event.into());
    rec.fields
        .insert("modified".into(), doc.modified.to_rfc3339().into());

    // If the doctype has a meaningful status/title, include it for dashboards.
    if let Some(title) = doc.get_field("title").and_then(|v| v.as_str()) {
        rec.fields.insert("title".into(), title.into());
    }
    if let Some(status) = doc.get_field("status").and_then(|v| v.as_str()) {
        rec.fields.insert("status".into(), status.into());
    }
    if let Some(severity) = doc
        .get_field("severity_classification")
        .and_then(|v| v.as_str())
    {
        rec.fields.insert("severity".into(), severity.into());
    }

    log(rec);
}

/// Convenience helper for application-level events that are not tied to a
/// specific document.
pub fn log_app_event(
    level: &str,
    service: &str,
    message: &str,
    fields: serde_json::Map<String, serde_json::Value>,
) {
    let mut rec = LogRecord::new(level, service, message);
    rec.fields.extend(fields);
    log(rec);
}
