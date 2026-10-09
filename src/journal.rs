use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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

    pub fn to_json_line(&self) -> String {
        #[derive(serde::Serialize)]
        struct RecordHelper<'a> {
            timestamp: u64,
            args: &'a [String],
            matches: usize,
            files: usize,
            bytes: u64,
            duration_us: u128,
            exit_code: i32,
        }
        let helper = RecordHelper {
            timestamp: self.timestamp_epoch_secs,
            args: &self.query_args,
            matches: self.matches_found,
            files: self.files_searched,
            bytes: self.bytes_searched,
            duration_us: self.duration.as_micros(),
            exit_code: self.exit_code,
        };
        let mut s = serde_json::to_string(&helper).unwrap_or_default();
        s.push('\n');
        s
    }
}

pub struct Journal;

impl Journal {
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
            query_args: vec!["foo".to_string(), "np:target/".to_string()],
            matches_found: 42,
            files_searched: 10,
            bytes_searched: 1024,
            duration: Duration::from_micros(12345),
            exit_code: 0,
        };
        let line = record.to_json_line();
        assert!(line.contains("\"timestamp\":1700000000"));
        assert!(line.contains("\"args\":[\"foo\",\"np:target/\"]"));
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
