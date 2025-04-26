use std::path::Path;
use uuid::Uuid;
use anyhow::Result;
use tracing::{info, warn, error};

/// Generates a unique ID for various entities
pub fn generate_id() -> String {
    Uuid::new_v4().to_string()
}

/// Check if a file exists and is readable
pub fn file_exists_and_readable(path: &str) -> bool {
    let path = Path::new(path);
    path.exists() && path.is_file() && path.metadata().map(|m| m.permissions().readonly()).unwrap_or(true)
}

/// Check if a directory exists and is readable
pub fn dir_exists_and_readable(path: &str) -> bool {
    let path = Path::new(path);
    path.exists() && path.is_dir() && path.metadata().map(|m| m.permissions().readonly()).unwrap_or(true)
}

/// Creates a string with the specified pattern repeated n times
pub fn repeat_str(s: &str, n: usize) -> String {
    std::iter::repeat(s).take(n).collect()
}

/// Formats a duration in milliseconds to a human-readable string
pub fn format_duration_ms(millis: u64) -> String {
    if millis < 1000 {
        format!("{}ms", millis)
    } else if millis < 60_000 {
        format!("{:.2}s", millis as f64 / 1000.0)
    } else {
        format!("{:.2}m", millis as f64 / 60_000.0)
    }
}

/// Truncates a string to a maximum length and adds an ellipsis if needed
pub fn truncate_string(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[0..max_len.saturating_sub(3)])
    }
}

/// Sanitizes a string for logging (removes newlines, tabs, etc.)
pub fn sanitize_for_logging(s: &str) -> String {
    s.replace('\n', "\\n").replace('\r', "\\r").replace('\t', "\\t")
}
