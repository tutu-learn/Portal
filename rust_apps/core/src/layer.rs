//! `tracing` layer that forwards Frappe runtime logs into the shared log sink.
//!
//! The layer is registered by the runtime as early as possible. It does not
//! depend on `AppState`; it pushes records into the global sink created by
//! [`logging::init_log_sink`](crate::logging::init_log_sink).

use std::fmt::Write;

use log_engine::LogRecord;
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::registry::LookupSpan;

use crate::logging;

/// A `tracing` layer that emits every event as a `LogRecord` to the log engine.
#[derive(Debug, Clone, Default)]
pub struct SebrusLoggerLayer;

impl SebrusLoggerLayer {
    pub fn new() -> Self {
        Self
    }
}

impl<S> Layer<S> for SebrusLoggerLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let mut message = String::new();
        let mut visitor = MessageVisitor(&mut message);
        event.record(&mut visitor);

        let level = match event.metadata().level() {
            &Level::ERROR => "ERROR",
            &Level::WARN => "WARN",
            &Level::INFO => "INFO",
            &Level::DEBUG => "DEBUG",
            &Level::TRACE => "TRACE",
        };

        let service = event
            .metadata()
            .target()
            .split_once("::")
            .map(|(first, _)| first)
            .unwrap_or_else(|| event.metadata().target());

        let mut rec = LogRecord::new(level, service, &message);
        rec.fields.insert(
            "target".into(),
            event.metadata().target().to_string().into(),
        );
        rec.fields.insert(
            "file".into(),
            event
                .metadata()
                .file()
                .unwrap_or("unknown")
                .to_string()
                .into(),
        );
        rec.fields.insert(
            "line".into(),
            event
                .metadata()
                .line()
                .map(|l| l as i64)
                .unwrap_or(-1)
                .into(),
        );

        // Include the current span name if available.
        if let Some(span) = ctx.lookup_current() {
            rec.fields
                .insert("span".into(), span.name().to_string().into());
        }

        let _ = logging::log(rec);
    }
}

struct MessageVisitor<'a>(&'a mut String);

impl<'a> tracing::field::Visit for MessageVisitor<'a> {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            let _ = write!(self.0, "{:?}", value);
        } else {
            if !self.0.is_empty() {
                let _ = write!(self.0, " ");
            }
            let _ = write!(self.0, "{}={:?}", field.name(), value);
        }
    }
}
