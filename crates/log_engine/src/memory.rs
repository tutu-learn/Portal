//! Portable RSS reader used to enforce an in-process memory budget.
//!
//! The log engine runs on small Kubernetes nodes where an out-of-memory
//! allocation produces SIGBUS and crashes the whole pod. Reading our own
//! resident set size before expensive Tantivy operations lets us back off
//! gracefully instead.

/// Try to read the current process RSS in bytes.
///
/// On Linux this parses `/proc/self/status`. On Unix platforms it falls back
/// to `getrusage(RUSAGE_SELF)`, which reports the maximum RSS and may be less
/// accurate but is good enough for a safety check. On Windows and other
/// unsupported platforms it returns `None`, so the memory budget check is
/// skipped.
pub fn current_rss_bytes() -> Option<usize> {
    #[cfg(target_os = "linux")]
    {
        if let Some(kb) = rss_kb_from_proc_self_status() {
            return Some(kb * 1024);
        }
    }

    #[cfg(unix)]
    {
        return rss_bytes_from_getrusage();
    }

    #[cfg(not(unix))]
    {
        None
    }
}

#[cfg(target_os = "linux")]
fn rss_kb_from_proc_self_status() -> Option<usize> {
    let contents = std::fs::read_to_string("/proc/self/status").ok()?;
    for line in contents.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            return rest
                .trim()
                .split_whitespace()
                .next()
                .and_then(|s| s.parse::<usize>().ok());
        }
    }
    None
}

#[cfg(unix)]
fn rss_bytes_from_getrusage() -> Option<usize> {
    let mut usage = unsafe { std::mem::zeroed() };
    let rc = unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) };
    if rc != 0 {
        return None;
    }
    // ru_maxrss is in kilobytes on Linux, bytes on macOS.
    #[cfg(target_os = "macos")]
    {
        Some(usage.ru_maxrss as usize)
    }
    #[cfg(not(target_os = "macos"))]
    {
        Some((usage.ru_maxrss as usize).saturating_mul(1024))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rss_is_non_zero_when_readable() {
        if let Some(rss) = current_rss_bytes() {
            assert!(rss > 0, "RSS should be positive");
        }
    }
}
