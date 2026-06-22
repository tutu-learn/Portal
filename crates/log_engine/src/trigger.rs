use crate::record::LogRecord;

/// A predicate evaluated against every ingested log record.
pub type Predicate = Box<dyn Fn(&LogRecord) -> bool + Send + Sync>;

/// A trigger pairs a name with a predicate.
pub struct Trigger {
    pub name: String,
    pub predicate: Predicate,
}

impl Trigger {
    pub fn new<F>(name: &str, predicate: F) -> Self
    where
        F: Fn(&LogRecord) -> bool + Send + Sync + 'static,
    {
        Self {
            name: name.into(),
            predicate: Box::new(predicate),
        }
    }
}

/// An alert emitted when a trigger predicate matches a record.
#[derive(Clone, Debug)]
pub struct Alert {
    pub trigger: String,
    pub record: LogRecord,
}
