//! Crash-durable log engine.
//!
//! WAL -> triggers -> Tantivy index -> filter/query.
//!
//! Durability rule: the WAL holds exactly the logs not yet committed to the
//! index. A log is written to the WAL and flushed BEFORE we touch the index, so
//! a crash between ingest and commit loses nothing -- restart replays the WAL.

mod engine;
mod error;
mod record;
mod service;
mod trigger;

pub use engine::LogEngine;
pub use error::{LogError, LogResult};
pub use record::LogRecord;
pub use service::LogService;
pub use trigger::{Alert, Trigger};
