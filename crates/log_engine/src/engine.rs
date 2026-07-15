use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::ops::Bound;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, SystemTime};

use tantivy::collector::{Count, TopDocs};
use tantivy::directory::MmapDirectory;
use tantivy::merge_policy::LogMergePolicy;
use tantivy::query::{QueryParser, RangeQuery, TermQuery};
use tantivy::schema::{
    Field, IndexRecordOption, Schema, Value, FAST, INDEXED, STORED, STRING, TEXT,
};
use tantivy::{doc, Index, IndexReader, IndexWriter, TantivyDocument, Term};

use crate::error::LogResult;
use crate::record::LogRecord;
use crate::trigger::{Alert, Trigger};

/// The core synchronous log engine.
///
/// Holds a Tantivy index writer/reader, the WAL file, trigger state, and an
/// in-memory staging buffer for records that have been indexed but not yet
/// flushed to disk. Staging lets ingest stay fast (no fsync) while queries
/// still see real-time records.
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
    /// Records indexed but not yet durably committed to disk.
    staged: Vec<LogRecord>,
    /// Approximate bytes currently held in `staged`.
    staged_bytes: usize,
    /// Commit as soon as this many records are staged, even if the time
    /// interval has not elapsed. Prevents high-frequency agents from
    /// accumulating unbounded RAM between time-based commits.
    staged_count_threshold: usize,
    /// Commit as soon as staged data reaches this size, even if the count
    /// threshold has not been reached.
    staged_bytes_threshold: usize,
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
        // On small servers (2 vCPU / 6 GiB) keep the writer heap modest. Larger
        // heaps reduce merge CPU but increase memory spikes. The staged-record
        // thresholds below are the primary memory bound; this heap just needs
        // to be big enough for normal indexing.
        let writer_heap_bytes = std::env::var("KIFF_LOG_ENGINE_HEAP_MB")
            .ok()
            .and_then(|s| s.parse().ok())
            .map(|mb: usize| mb * 1_000_000)
            .unwrap_or(64_000_000usize);
        let writer: IndexWriter = index.writer(writer_heap_bytes)?;
        let mut merge_policy = LogMergePolicy::default();
        // Require at least a few segments before Tantivy starts merging. This
        // trades a small query-time penalty for much lower disk read churn
        // when agents poll frequently.
        merge_policy.set_min_num_segments(6);
        writer.set_merge_policy(Box::new(merge_policy));
        let reader = index.reader()?;
        let (alert_tx, alert_rx) = channel();

        // Memory-bounding knobs for the staging buffer. With 3 agents calling
        // home every 10 s, a count threshold of 1000 typically commits every
        // ~30-60 s; the bytes threshold catches a single oversized batch.
        let staged_count_threshold = std::env::var("KIFF_LOG_STAGED_COUNT_THRESHOLD")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1_000usize);
        let staged_bytes_threshold = std::env::var("KIFF_LOG_STAGED_BYTES_MB")
            .ok()
            .and_then(|s| s.parse().ok())
            .map(|mb: usize| mb * 1_000_000)
            .unwrap_or(64_000_000usize);

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
            staged: Vec::new(),
            staged_bytes: 0,
            staged_count_threshold,
            staged_bytes_threshold,
        };

        // Crash recovery: re-index the pending logs (no WAL re-append, no
        // re-firing triggers), then commit the writer so the recovered records
        // become durable.
        if !pending.is_empty() {
            tracing::info!("recovering {} log(s) from the WAL", pending.len());
            for rec in &pending {
                engine.index_record(rec)?;
            }
            engine.commit_writer()?;
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
    /// Returns the serialized size of the record, which is used to bound the
    /// in-memory staging buffer.
    fn index_record(&mut self, rec: &LogRecord) -> LogResult<usize> {
        let raw = serde_json::to_string(rec)?;
        let raw_len = raw.len();
        self.writer.add_document(doc!(
            self.f_timestamp => rec.timestamp,
            self.f_level => rec.level.clone(),
            self.f_service => rec.service.clone(),
            self.f_message => rec.message.clone(),
            self.f_raw => raw,
        ))?;
        Ok(raw_len)
    }

    /// Ingest one log. Durability FIRST, then triggers, then indexing.
    pub fn ingest(&mut self, rec: LogRecord) -> LogResult<()> {
        self.ingest_batch(std::slice::from_ref(&rec))
    }

    /// Ingest a batch of logs into memory and the in-memory Tantivy index.
    ///
    /// Records are staged in RAM and become immediately queryable. They are
    /// only written to the WAL and fsync'd to disk on the next [`commit`]. This
    /// removes the per-request fsync from the hot path for high-frequency
    /// agents.
    ///
    /// To keep RAM bounded on small servers, an early commit is triggered when
    /// the staged buffer crosses a count or size threshold. The time-based
    /// commit loop remains as a fallback for low-traffic periods.
    pub fn ingest_batch(&mut self, recs: &[LogRecord]) -> LogResult<()> {
        if recs.is_empty() {
            return Ok(());
        }

        self.staged.reserve(recs.len());
        for rec in recs {
            self.staged.push(rec.clone());
            for t in &self.triggers {
                if (t.predicate)(rec) {
                    let _ = self.alert_tx.send(Alert {
                        trigger: t.name.clone(),
                        record: rec.clone(),
                    });
                }
            }
            let raw_len = self.index_record(rec)?;
            self.staged_bytes += raw_len;
        }

        if self.staged.len() >= self.staged_count_threshold
            || self.staged_bytes >= self.staged_bytes_threshold
        {
            self.commit()?;
        }
        Ok(())
    }

    /// Persist staged logs to disk and make them searchable.
    ///
    /// 1. Write all staged records to the WAL and fsync.
    /// 2. Commit the Tantivy index (fsync).
    /// 3. Truncate the WAL now that records are durable in the index.
    /// 4. Clear the in-memory staging buffer.
    pub fn commit(&mut self) -> LogResult<()> {
        // Skip empty commits so the time-based commit loop does not force
        // Tantivy segment flushes / garbage collection when there is no new
        // data. This saves CPU on small servers.
        if self.staged.is_empty() {
            return Ok(());
        }

        for rec in &self.staged {
            writeln!(self.wal, "{}", serde_json::to_string(rec)?)?;
        }
        self.wal.flush()?;

        self.commit_writer()?;
        self.wal.flush()?;
        self.wal.set_len(0)?; // append handle: next write lands at offset 0
        self.staged.clear();
        self.staged_bytes = 0;
        Ok(())
    }

    /// Commit the Tantivy writer and reload the reader without touching the
    /// WAL or staged buffer. Used for recovery and explicit index maintenance.
    fn commit_writer(&mut self) -> LogResult<()> {
        self.writer.commit()?;
        self.reader.reload()?;
        Ok(())
    }

    /// Delete committed records older than `max_age` and return how many were
    /// removed.
    ///
    /// This first commits anything currently staged so that staged records are
    /// also subject to the retention rule and so the before/after counts are
    /// accurate. Then it deletes every document whose `timestamp` is older than
    /// the cutoff and commits the deletion.
    pub fn prune_older_than(&mut self, max_age: Duration) -> LogResult<usize> {
        if !self.staged.is_empty() {
            self.commit()?;
        }

        let cutoff = SystemTime::now() - max_age;
        let cutoff_ms = cutoff
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;

        let before = self.count("*")?;
        let query: Box<dyn tantivy::query::Query> = Box::new(RangeQuery::new_i64_bounds(
            "timestamp".to_string(),
            Bound::Unbounded,
            Bound::Excluded(cutoff_ms),
        ));
        self.writer.delete_query(query)?;
        self.writer.commit()?;
        self.reader.reload()?;
        let after = self.count("*")?;

        Ok(before.saturating_sub(after))
    }

    /// Delete all committed records for a single service and return how many
    /// were removed. Used to purge high-volume telemetry history from the
    /// index when it is no longer needed.
    pub fn prune_service(&mut self, service: &str) -> LogResult<usize> {
        if !self.staged.is_empty() {
            self.commit()?;
        }

        let before = self.count("*")?;
        let term = Term::from_field_text(self.f_service, service);
        let query: Box<dyn tantivy::query::Query> =
            Box::new(TermQuery::new(term, IndexRecordOption::Basic));
        self.writer.delete_query(query)?;
        self.writer.commit()?;
        self.reader.reload()?;
        let after = self.count("*")?;

        Ok(before.saturating_sub(after))
    }

    /// Query the committed index plus any in-memory staged records.
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

        // Include staged records that match the same query.
        for rec in &self.staged {
            if staged_matches_query(rec, q) {
                out.push(rec.clone());
            }
        }

        // Newest first, then cap to the requested limit.
        out.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        out.truncate(limit);
        Ok(out)
    }

    /// Return the number of committed log records matching `q` without loading
    /// the documents. Staged (not-yet-committed) records are included as an
    /// upper-bound estimate so the desk count stays fresh.
    pub fn count(&self, q: &str) -> LogResult<usize> {
        let searcher = self.reader.searcher();
        let qp = QueryParser::for_index(
            &self.index,
            vec![self.f_message, self.f_level, self.f_service],
        );
        let query = qp.parse_query(q)?;
        let committed = searcher.search(&query, &Count)?;

        let staged = self
            .staged
            .iter()
            .filter(|rec| staged_matches_query(rec, q))
            .count();
        Ok(committed + staged)
    }

    /// Path to the write-ahead log.
    pub fn wal_path(&self) -> &Path {
        &self.wal_path
    }
}

/// Lightweight matcher that decides whether an in-memory staged record should
/// be included in query results for `query`.
///
/// The log engine keeps records in RAM until the next commit, but queries are
/// expressed in Tantivy syntax. We do not run a full in-memory Tantivy index;
/// instead we handle the query shapes the app actually uses: `OR`/`AND`
/// keywords, parenthesized groups, `field:value` terms, inclusive range terms
/// like `timestamp:[a TO b]` (with `*` for an unbounded side), and free text.
fn staged_matches_query(rec: &LogRecord, q: &str) -> bool {
    let q = q.trim();
    if q.is_empty() || q == "*" {
        return true;
    }

    // `OR` has the lowest precedence: a record matches when any clause does.
    let mut clauses: Vec<Vec<String>> = vec![Vec::new()];
    for token in tokenize_query(q) {
        if token == "OR" {
            clauses.push(Vec::new());
        } else {
            clauses.last_mut().expect("non-empty clauses").push(token);
        }
    }
    clauses
        .iter()
        .any(|clause| staged_matches_and_clause(rec, clause))
}

/// Tokenize a query on whitespace, keeping bracketed ranges (`[a TO b]`) and
/// parenthesized groups intact even though they contain spaces.
fn tokenize_query(q: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut depth = 0i32;
    for c in q.chars() {
        match c {
            '(' | '[' => {
                depth += 1;
                current.push(c);
            }
            ')' | ']' => {
                depth -= 1;
                current.push(c);
            }
            c if c.is_whitespace() && depth == 0 => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(c),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

/// All terms in a clause must match (`AND` keywords are simply dropped).
fn staged_matches_and_clause(rec: &LogRecord, tokens: &[String]) -> bool {
    tokens
        .iter()
        .filter(|t| t.as_str() != "AND")
        .all(|token| staged_matches_term(rec, token))
}

fn staged_matches_term(rec: &LogRecord, term: &str) -> bool {
    let term = term.trim();
    if term.is_empty() {
        return true;
    }
    // Parenthesized group: recurse on the inner query.
    if term.starts_with('(') && term.ends_with(')') && term.len() > 2 {
        return staged_matches_query(rec, &term[1..term.len() - 1]);
    }

    if let Some((field, value)) = term.split_once(':') {
        let field = field.trim();
        let value = value.trim();
        // Inclusive range term, e.g. timestamp:[4000 TO 6000] or [4000 TO *].
        if value.starts_with('[') && value.ends_with(']') {
            return staged_matches_range(rec, field, &value[1..value.len() - 1]);
        }
        match field.to_lowercase().as_str() {
            "service" => rec.service.eq_ignore_ascii_case(value),
            "level" => rec.level.eq_ignore_ascii_case(value),
            "message" => rec.message.to_lowercase().contains(&value.to_lowercase()),
            _ => rec
                .fields
                .get(field)
                .and_then(|v| v.as_str())
                .map(|s| s.eq_ignore_ascii_case(value))
                .unwrap_or(false),
        }
    } else {
        // Free-text term: match message, service, level, or any string field.
        let term_lower = term.to_lowercase();
        rec.message.to_lowercase().contains(&term_lower)
            || rec.service.to_lowercase().contains(&term_lower)
            || rec.level.to_lowercase().contains(&term_lower)
            || rec.fields.values().any(|v| {
                v.as_str()
                    .map(|s| s.to_lowercase().contains(&term_lower))
                    .unwrap_or(false)
            })
    }
}

/// Match an inclusive `a TO b` range against a numeric record field. `*` leaves
/// that side unbounded. Only `timestamp` is an indexed numeric field today.
fn staged_matches_range(rec: &LogRecord, field: &str, range: &str) -> bool {
    if !field.eq_ignore_ascii_case("timestamp") {
        return false;
    }
    let Some((lo, hi)) = range.split_once(" TO ") else {
        return false;
    };
    let lo = lo.trim();
    let hi = hi.trim();
    let lo_ok = lo == "*" || lo.parse::<i64>().map(|v| rec.timestamp >= v).unwrap_or(false);
    let hi_ok = hi == "*" || hi.parse::<i64>().map(|v| rec.timestamp <= v).unwrap_or(false);
    lo_ok && hi_ok
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn temp_dir() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let mut p = std::env::temp_dir();
        p.push(format!(
            "log_engine_test_{}_{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
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

        // Session 1: commit one, stage another without commit.
        {
            let (mut engine, _alerts) = LogEngine::open_or_create(&dir).unwrap();
            engine
                .ingest(LogRecord::new("INFO", "web", "committed"))
                .unwrap();
            engine.commit().unwrap();
            engine
                .ingest(LogRecord::new("WARN", "web", "uncommitted"))
                .unwrap();
            // drop without commit -> staged record is only in RAM, lost
        }

        // Session 2: reopen. Only the committed record is recovered from WAL.
        let (engine, _alerts) = LogEngine::open_or_create(&dir).unwrap();
        let all = engine.query("*", 10).unwrap();
        assert_eq!(all.len(), 1);
        assert!(all.iter().any(|r| r.message == "committed"));
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

    #[test]
    fn committed_query_supports_unbounded_timestamp_range() {
        let dir = temp_dir();
        let (mut engine, _alerts) = LogEngine::open_or_create(&dir).unwrap();

        let mut old = LogRecord::new("INFO", "web", "old event");
        old.timestamp = 1_000;
        let mut recent = LogRecord::new("INFO", "web", "recent event");
        recent.timestamp = 5_000;

        engine.ingest(old).unwrap();
        engine.ingest(recent).unwrap();
        engine.commit().unwrap();

        let hits = engine.query("timestamp:[4000 TO *]", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].message, "recent event");
    }

    #[test]
    fn staged_match_supports_or_groups_and_timestamp_range() {
        let dir = temp_dir();
        let (mut engine, _alerts) = LogEngine::open_or_create(&dir).unwrap();

        // Shape used by the ops portal overview: OR'd services AND a 24h range.
        let q = "(service:audit_ready.telemetry OR service:audit_ready.telemetry.compliance) \
                 AND timestamp:[4000 TO *]";

        let mut fresh = LogRecord::new("ERROR", "audit_ready.telemetry.compliance", "check failed");
        fresh.timestamp = 5_000;
        engine.ingest(fresh).unwrap();

        let mut stale = LogRecord::new("ERROR", "audit_ready.telemetry.compliance", "old failure");
        stale.timestamp = 1_000;
        engine.ingest(stale).unwrap();

        let mut other_service = LogRecord::new("INFO", "web", "unrelated");
        other_service.timestamp = 5_000;
        engine.ingest(other_service).unwrap();

        // No commit: all three records are staged. Only the fresh compliance
        // record matches both the OR group and the range.
        let hits = engine.query(q, 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].message, "check failed");
    }

    #[test]
    fn prune_older_than_removes_old_records() {
        let dir = temp_dir();
        let (mut engine, _alerts) = LogEngine::open_or_create(&dir).unwrap();

        let mut old = LogRecord::new("INFO", "web", "old event");
        old.timestamp = 1_000;
        let mut recent = LogRecord::new("INFO", "web", "recent event");
        recent.timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        engine.ingest(old).unwrap();
        engine.ingest(recent).unwrap();
        engine.commit().unwrap();
        assert_eq!(engine.count("*").unwrap(), 2);

        let deleted = engine.prune_older_than(Duration::from_secs(1)).unwrap();
        assert_eq!(deleted, 1);
        assert_eq!(engine.count("*").unwrap(), 1);

        let hits = engine.query("*", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].message, "recent event");
    }

    #[test]
    fn staged_records_are_queryable_before_commit() {
        let dir = temp_dir();
        let (mut engine, _alerts) = LogEngine::open_or_create(&dir).unwrap();

        engine
            .ingest(LogRecord::new("INFO", "audit_ready.telemetry", "snapshot"))
            .unwrap();

        // Without committing, the staged record should still be visible.
        let hits = engine.query("service:audit_ready.telemetry", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].message, "snapshot");

        // Non-matching queries should not return it.
        let hits = engine.query("level:ERROR", 10).unwrap();
        assert!(hits.is_empty());
    }

    #[test]
    fn commit_clears_staged_and_wal() {
        let dir = temp_dir();
        let (mut engine, _alerts) = LogEngine::open_or_create(&dir).unwrap();

        engine
            .ingest(LogRecord::new("INFO", "web", "staged then committed"))
            .unwrap();
        engine.commit().unwrap();

        // After commit the record is in the durable index.
        let hits = engine.query("service:web", 10).unwrap();
        assert_eq!(hits.len(), 1);

        // WAL should be empty now.
        let wal_size = std::fs::metadata(&engine.wal_path)
            .map(|m| m.len())
            .unwrap_or(0);
        assert_eq!(wal_size, 0);
    }

    #[test]
    fn uncommitted_records_are_lost_on_reopen() {
        let dir = temp_dir();
        {
            let (mut engine, _alerts) = LogEngine::open_or_create(&dir).unwrap();
            engine
                .ingest(LogRecord::new("INFO", "web", "never committed"))
                .unwrap();
            // drop without commit -> record is only in RAM, lost
        }

        let (engine, _alerts) = LogEngine::open_or_create(&dir).unwrap();
        let hits = engine.query("*", 10).unwrap();
        assert!(hits.is_empty());
    }

    #[test]
    fn count_returns_committed_and_staged_matches() {
        let dir = temp_dir();
        let (mut engine, _alerts) = LogEngine::open_or_create(&dir).unwrap();

        engine
            .ingest(LogRecord::new("INFO", "web", "committed 1"))
            .unwrap();
        engine
            .ingest(LogRecord::new("ERROR", "web", "committed 2"))
            .unwrap();
        engine.commit().unwrap();

        engine
            .ingest(LogRecord::new("ERROR", "web", "staged"))
            .unwrap();

        assert_eq!(engine.count("*").unwrap(), 3);
        assert_eq!(engine.count("level:ERROR").unwrap(), 2);
        assert_eq!(engine.count("service:auth").unwrap(), 0);
    }
}
