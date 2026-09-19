use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Structured execution record for grx journal telemetry.
#[derive(Debug, Clone)]
pub struct JournalRecord {
    pub timestamp_epoch_secs: u64,
    pub query_args: Vec<String>,
    pub matches_found: usize,
    pub files_searched: usize,
    pub bytes_searched: u64,
    pub duration: Duration,
    pub exit_code: i32,
}

impl JournalRecord {
    pub fn new(
        query_args: Vec<String>,
        matches_found: usize,
        files_searched: usize,
        duration: Duration,
        exit_code: i32,
    ) -> Self {
        Self::new_with_bytes(
            query_args,
            matches_found,
            files_searched,
            0,
            duration,
            exit_code,
        )
    }

    pub fn new_with_bytes(
        query_args: Vec<String>,
        matches_found: usize,
        files_searched: usize,
        bytes_searched: u64,
        duration: Duration,
        exit_code: i32,
    ) -> Self {
        let epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self {
            timestamp_epoch_secs: epoch,
            query_args,
            matches_found,
            files_searched,
            bytes_searched,
            duration,
            exit_code,
        }
    }

    /// Render this record as a compact JSON Line.
    pub fn to_json_line(&self) -> String {
        let escaped_args: Vec<String> = self
            .query_args
            .iter()
            .map(|a| {
                let mut s = String::with_capacity(a.len() + 8);
                s.push('"');
                for c in a.chars() {
                    match c {
                        '\\' => s.push_str("\\\\"),
                        '\"' => s.push_str("\\\""),
                        '\n' => s.push_str("\\n"),
                        '\r' => s.push_str("\\r"),
                        '\t' => s.push_str("\\t"),
                        c if (c as u32) < 0x20 => {
                            use std::fmt::Write;
                            let _ = write!(s, "\\u{:04x}", c as u32);
                        }
                        c => s.push(c),
                    }
                }
                s.push('"');
                s
            })
            .collect();
        format!(
            "{{\"timestamp\":{},\"args\":[{}],\"matches\":{},\"files\":{},\"bytes\":{},\"duration_us\":{},\"exit_code\":{}}}\n",
            self.timestamp_epoch_secs,
            escaped_args.join(","),
            self.matches_found,
            self.files_searched,
            self.bytes_searched,
            self.duration.as_micros(),
            self.exit_code,
        )
    }
}

/// Append-only structured execution logger.
pub struct Journal;

impl Journal {
    /// Append an execution record to the journal file.
    /// Ensures parent directories are created in the distro-appropriate location.
    pub fn append(path: &Path, record: &JournalRecord) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new().create(true).append(true).open(path)?;
        file.write_all(record.to_json_line().as_bytes())?;
        file.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_journal_record_json_serialization() {
        let record = JournalRecord {
            timestamp_epoch_secs: 1700000000,
            query_args: vec!["foo".to_string(), "no:target/".to_string()],
            matches_found: 42,
            files_searched: 10,
            bytes_searched: 1024,
            duration: Duration::from_micros(12345),
            exit_code: 0,
        };
        let line = record.to_json_line();
        assert!(line.contains("\"timestamp\":1700000000"));
        assert!(line.contains("\"args\":[\"foo\",\"no:target/\"]"));
        assert!(line.contains("\"matches\":42"));
        assert!(line.contains("\"files\":10"));
        assert!(line.contains("\"bytes\":1024"));
        assert!(line.contains("\"duration_us\":12345"));
        assert!(line.contains("\"exit_code\":0"));
        assert!(line.ends_with('\n'));
    }

    #[test]
    fn test_journal_record_json_escaping_newlines_and_tabs() {
        let record = JournalRecord {
            timestamp_epoch_secs: 1700000000,
            query_args: vec![
                "foo\nbar".to_string(),
                "quote\"and\\tab\t".to_string(),
                "crlf\r\n".to_string(),
            ],
            matches_found: 1,
            files_searched: 1,
            bytes_searched: 100,
            duration: Duration::from_micros(100),
            exit_code: 0,
        };
        let line = record.to_json_line();
        // JSONL records must be formatted on a single line
        assert_eq!(line.lines().count(), 1);
        assert!(line.contains("\"foo\\nbar\""));
        assert!(line.contains("\"quote\\\"and\\\\tab\\t\""));
        assert!(line.contains("\"crlf\\r\\n\""));
    }

    #[test]
    fn test_journal_append_creates_file_and_dirs() {
        let temp_dir = tempfile::tempdir().unwrap();
        let journal_path = temp_dir.path().join("nested/dir/journal.jsonl");

        let record =
            JournalRecord::new(vec!["test".to_string()], 5, 2, Duration::from_millis(10), 0);
        Journal::append(&journal_path, &record).expect("append should succeed");

        assert!(journal_path.is_file());
        let content = std::fs::read_to_string(&journal_path).unwrap();
        assert!(content.contains("\"matches\":5"));
    }
}
