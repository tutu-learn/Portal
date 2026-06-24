use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};

use tantivy::collector::TopDocs;
use tantivy::directory::MmapDirectory;
use tantivy::query::QueryParser;
use tantivy::schema::{Field, Schema, Value, FAST, INDEXED, STORED, STRING, TEXT};
use tantivy::{doc, Index, IndexReader, IndexWriter, TantivyDocument};

use crate::error::{LogError, LogResult};
use crate::record::LogRecord;
use crate::trigger::{Alert, Trigger};

/// The core synchronous log engine.
///
/// Holds a Tantivy index writer/reader, the WAL file, and trigger state.
pub struct LogEngine {
    index: Index,
    writer: IndexWriter,
    reader: IndexReader,
    f_timestamp: Field,
    f_level: Field,
    f_service: Field,
    f_message: Field,
    f_raw: Field,
    triggers: Vec<Trigger>,
    alert_tx: Sender<Alert>,
    wal: File,
    wal_path: PathBuf,
}

impl LogEngine {
    /// Open (or create) a disk-backed engine rooted at `dir`. On open, any
    /// un-committed entries left in the WAL by a previous crash are replayed
    /// back into the index. Returns the engine plus the alert receiver.
    pub fn open_or_create(dir: &Path) -> LogResult<(Self, Receiver<Alert>)> {
        std::fs::create_dir_all(dir)?;
        let index_dir = dir.join("index");
        std::fs::create_dir_all(&index_dir)?;
        let wal_path = dir.join("wal.ndjson");

        let mut sb = Schema::builder();
        let f_timestamp = sb.add_i64_field("timestamp", INDEXED | FAST);
        let f_level = sb.add_text_field("level", STRING);
        let f_service = sb.add_text_field("service", STRING);
        let f_message = sb.add_text_field("message", TEXT);
        let f_raw = sb.add_text_field("raw", STORED);
        let schema = sb.build();

        let index = Index::open_or_create(MmapDirectory::open(&index_dir)?, schema)?;
        let writer: IndexWriter = index.writer(50_000_000)?;
        let reader = index.reader()?;
        let (alert_tx, alert_rx) = channel();

        // Read pending (un-committed) WAL entries before we reopen for append.
        let pending: Vec<LogRecord> = if wal_path.exists() {
            let f = File::open(&wal_path)?;
            BufReader::new(f)
                .lines()
                .map_while(|l| l.ok())
                .filter(|l| !l.trim().is_empty())
                .filter_map(|l| serde_json::from_str::<LogRecord>(&l).ok())
                .collect()
        } else {
            Vec::new()
        };

        let wal = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&wal_path)?;

        let mut engine = LogEngine {
            index,
            writer,
            reader,
            f_timestamp,
            f_level,
            f_service,
            f_message,
            f_raw,
            triggers: Vec::new(),
            alert_tx,
            wal,
            wal_path,
        };

        // Crash recovery: re-index the pending logs (no WAL re-append, no
        // re-firing triggers), then commit -- which truncates the WAL.
        if !pending.is_empty() {
            tracing::info!("recovering {} log(s) from the WAL", pending.len());
            for rec in &pending {
                engine.index_record(rec)?;
            }
            engine.commit()?;
        }

        Ok((engine, alert_rx))
    }

    /// Register a trigger that will be evaluated on every ingested record.
    pub fn add_trigger<F>(&mut self, name: &str, predicate: F)
    where
        F: Fn(&LogRecord) -> bool + Send + Sync + 'static,
    {
        self.triggers.push(Trigger::new(name, predicate));
    }

    /// Stage a record into the index (used by both ingest and recovery).
    fn index_record(&mut self, rec: &LogRecord) -> LogResult<()> {
        let raw = serde_json::to_string(rec)?;
        self.writer.add_document(doc!(
            self.f_timestamp => rec.timestamp,
            self.f_level => rec.level.clone(),
            self.f_service => rec.service.clone(),
            self.f_message => rec.message.clone(),
            self.f_raw => raw,
        ))?;
        Ok(())
    }

    /// Ingest one log. Durability FIRST, then triggers, then indexing.
    pub fn ingest(&mut self, rec: LogRecord) -> LogResult<()> {
        // 1. Durable append: write + flush to the WAL before anything else.
        writeln!(self.wal, "{}", serde_json::to_string(&rec)?)?;
        self.wal.flush()?;

        // 2. Triggers, in real time.
        for t in &self.triggers {
            if (t.predicate)(&rec) {
                let _ = self.alert_tx.send(Alert {
                    trigger: t.name.clone(),
                    record: rec.clone(),
                });
            }
        }

        // 3. Stage into the index (not searchable until commit()).
        self.index_record(&rec)?;
        Ok(())
    }

    /// Make staged logs searchable, then checkpoint: the WAL no longer needs
    /// the entries now durably in the index, so we truncate it.
    pub fn commit(&mut self) -> LogResult<()> {
        self.writer.commit()?;
        self.reader.reload()?;
        self.wal.flush()?;
        self.wal.set_len(0)?; // append handle: next write lands at offset 0
        Ok(())
    }

    /// Query the committed index and return matching records.
    pub fn query(&self, q: &str, limit: usize) -> LogResult<Vec<LogRecord>> {
        let searcher = self.reader.searcher();
        let qp = QueryParser::for_index(
            &self.index,
            vec![self.f_message, self.f_level, self.f_service],
        );
        let query = qp.parse_query(q)?;
        let hits = searcher.search(&query, &TopDocs::with_limit(limit))?;

        let mut out = Vec::new();
        for (_score, addr) in hits {
            let d: TantivyDocument = searcher.doc(addr)?;
            if let Some(raw) = d.get_first(self.f_raw).and_then(|v| v.as_str()) {
                if let Ok(rec) = serde_json::from_str::<LogRecord>(raw) {
                    out.push(rec);
                }
            }
        }
        Ok(out)
    }

    /// Path to the write-ahead log.
    pub fn wal_path(&self) -> &Path {
        &self.wal_path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn temp_dir() -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("log_engine_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        p
    }

    #[test]
    fn ingest_commit_query() {
        let dir = temp_dir();
        let (mut engine, _alerts) = LogEngine::open_or_create(&dir).unwrap();

        engine
            .ingest(LogRecord::new("INFO", "web", "GET /health 200"))
            .unwrap();
        engine
            .ingest(LogRecord::new("ERROR", "auth", "login timeout"))
            .unwrap();
        engine.commit().unwrap();

        let hits = engine.query("level:ERROR", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].service, "auth");

        let all = engine.query("*", 10).unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn wal_recovery_after_crash() {
        let dir = temp_dir();

        // Session 1: commit one, ingest another without commit.
        {
            let (mut engine, _alerts) = LogEngine::open_or_create(&dir).unwrap();
            engine
                .ingest(LogRecord::new("INFO", "web", "committed"))
                .unwrap();
            engine.commit().unwrap();
            engine
                .ingest(LogRecord::new("WARN", "web", "uncommitted"))
                .unwrap();
            // drop without commit -> simulated crash
        }

        // Session 2: reopen, WAL replay should recover the uncommitted log.
        let (engine, _alerts) = LogEngine::open_or_create(&dir).unwrap();
        let all = engine.query("*", 10).unwrap();
        assert_eq!(all.len(), 2);
        assert!(all.iter().any(|r| r.message == "uncommitted"));
    }

    #[test]
    fn trigger_fires_alert() {
        let dir = temp_dir();
        let (mut engine, alerts) = LogEngine::open_or_create(&dir).unwrap();
        engine.add_trigger("any-error", |r| r.level == "ERROR");

        engine
            .ingest(LogRecord::new("ERROR", "billing", "card declined"))
            .unwrap();

        let alert = alerts.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(alert.trigger, "any-error");
        assert_eq!(alert.record.service, "billing");
    }

    #[test]
    fn query_by_timestamp_range() {
        let dir = temp_dir();
        let (mut engine, _alerts) = LogEngine::open_or_create(&dir).unwrap();

        let mut old = LogRecord::new("INFO", "web", "old event");
        old.timestamp = 1_000;
        let mut recent = LogRecord::new("INFO", "web", "recent event");
        recent.timestamp = 5_000;

        engine.ingest(old).unwrap();
        engine.ingest(recent).unwrap();
        engine.commit().unwrap();

        let hits = engine.query("timestamp:[4000 TO 6000]", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].message, "recent event");
    }
}
