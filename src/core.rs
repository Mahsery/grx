use std::borrow::Cow;
use std::fmt;
use std::io;
use std::ops::Deref;
use std::path::{Path, PathBuf};

/// Filtering decision made for a directory or file during traversal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterDecision {
    /// File or directory should be processed.
    Include,
    /// Specific file should be skipped.
    Exclude,
    /// Directory subtree should be completely pruned (e.g. target/, .git/).
    SkipDir,
}

/// Compact representation of a filesystem entry during traversal.
/// Avoids eager string/path allocations until a match is confirmed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    /// Relative or canonical path to parent directory.
    pub parent: PathBuf,
    /// Entry filename as raw bytes (handles non-UTF-8 filenames losslessly).
    pub name: Vec<u8>,
    /// Whether this entry is a directory.
    pub is_dir: bool,
    /// Whether this entry is a symlink.
    pub is_symlink: bool,
    /// Optional file size in bytes (if known from readdir/stat).
    pub size: Option<u64>,
}

impl DirEntry {
    /// Construct a new DirEntry.
    pub fn new(
        parent: PathBuf,
        name: Vec<u8>,
        is_dir: bool,
        is_symlink: bool,
        size: Option<u64>,
    ) -> Self {
        Self {
            parent,
            name,
            is_dir,
            is_symlink,
            size,
        }
    }

    /// Construct a full PathBuf lazily when required for match reporting.
    pub fn full_path(&self) -> PathBuf {
        #[cfg(unix)]
        {
            use std::ffi::OsStr;
            use std::os::unix::ffi::OsStrExt;
            let os_name = OsStr::from_bytes(&self.name);
            self.parent.join(os_name)
        }
        #[cfg(not(unix))]
        {
            let s = String::from_utf8_lossy(&self.name);
            self.parent.join(s.as_ref())
        }
    }

    /// Entry name losslessly formatted for display.
    pub fn name_display(&self) -> Cow<'_, str> {
        String::from_utf8_lossy(&self.name)
    }

    /// True if this entry is a regular file (neither a directory nor a symlink).
    #[inline]
    pub fn is_file(&self) -> bool {
        !self.is_dir && !self.is_symlink
    }
}

/// Evaluation interface for determining if a traversed filesystem entry satisfies selection criteria.
pub trait EntryPredicate: Send + Sync {
    /// Evaluate whether this entry satisfies the selection criteria.
    /// Access to filesystem metadata is lazy and performed only when size or time rules require it.
    fn matches(&self, entry: &DirEntry) -> io::Result<bool>;
}

/// Consumer interface receiving discovered filesystem entries.
pub trait EntrySink: Send + Sync {
    /// Callback invoked for each discovered entry.
    fn on_entry(&mut self, entry: &DirEntry) -> io::Result<()>;
}

/// Evaluation interface for filtering paths and files.
pub trait IgnoreFilter: Send + Sync {
    /// Decide whether to include, exclude, or skip a subtree.
    fn filter(&self, parent: &Path, name: &[u8], is_dir: bool) -> FilterDecision;

    /// Optionally create a scoped child filter for a subdirectory if new ignore rules exist.
    fn for_dir(&self, dir: &Path) -> Option<std::sync::Arc<dyn IgnoreFilter>> {
        let _ = dir;
        None
    }
}

/// Traversal error type.
#[derive(Debug)]
pub enum WalkerError {
    Io(io::Error),
    General(String),
}

impl fmt::Display for WalkerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WalkerError::Io(e) => write!(f, "I/O error during traversal: {e}"),
            WalkerError::General(s) => write!(f, "Traversal error: {s}"),
        }
    }
}

impl WalkerError {
    /// Check whether this error represents a broken pipe (`EPIPE` / `SIGPIPE`).
    pub fn is_broken_pipe(&self) -> bool {
        match self {
            WalkerError::Io(e) => e.kind() == io::ErrorKind::BrokenPipe,
            WalkerError::General(_) => false,
        }
    }
}

impl std::error::Error for WalkerError {}

impl From<io::Error> for WalkerError {
    fn from(e: io::Error) -> Self {
        WalkerError::Io(e)
    }
}

/// Filesystem directory traversal interface.
pub trait DirectoryWalker: Send + Sync {
    /// Walk the target roots, applying the given ignore filter and yielding entries.
    fn walk(
        &self,
        roots: &[PathBuf],
        filter: std::sync::Arc<dyn IgnoreFilter>,
        on_entry: &(dyn Fn(DirEntry) -> Result<(), WalkerError> + Sync),
    ) -> Result<(), WalkerError>;
}

/// Buffer reference that abstracts between memory-mapped pages, thread-local pooled
/// buffers, and heap-allocated stream chunks.
pub enum BufferRef<'a> {
    /// Borrowed slice from thread-local reusable buffer.
    Borrowed(&'a [u8]),
    /// Owned buffer for stream chunks.
    Owned(Vec<u8>),
    /// Memory-mapped file slice (zero-copy).
    Mmap(memmap2::Mmap),
}

impl Deref for BufferRef<'_> {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &Self::Target {
        match self {
            BufferRef::Borrowed(s) => s,
            BufferRef::Owned(v) => v.as_slice(),
            BufferRef::Mmap(m) => m.as_ref(),
        }
    }
}

/// I/O abstraction for reading file contents safely and efficiently.
pub trait ContentReader: Send + Sync {
    /// Read content into a BufferRef using the optimal I/O mechanism (mmap or pooled buffer).
    fn read<'a>(&self, path: &Path, scratch: &'a mut Vec<u8>) -> io::Result<BufferRef<'a>>;
}

/// A matched line within a file, containing byte offsets and matched spans.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchRecord<'a> {
    /// 1-indexed line number.
    pub line_number: usize,
    /// Absolute byte offset in file where line begins.
    pub line_byte_offset: usize,
    /// Raw byte content of the line (excluding trailing newline).
    pub line_bytes: &'a [u8],
    /// Match spans within this line: (start_byte_index, end_byte_index).
    pub match_spans: Vec<(usize, usize)>,
    /// True if this record is a surrounding context line rather than an active match.
    pub is_context: bool,
}

impl<'a> MatchRecord<'a> {
    pub fn new(
        line_number: usize,
        line_byte_offset: usize,
        line_bytes: &'a [u8],
        match_spans: Vec<(usize, usize)>,
    ) -> Self {
        Self {
            line_number,
            line_byte_offset,
            line_bytes,
            match_spans,
            is_context: false,
        }
    }

    pub fn context(line_number: usize, line_byte_offset: usize, line_bytes: &'a [u8]) -> Self {
        Self {
            line_number,
            line_byte_offset,
            line_bytes,
            match_spans: Vec::new(),
            is_context: true,
        }
    }

    /// Convert to an owned record with an allocated byte buffer.
    pub fn to_owned(&self) -> OwnedMatchRecord {
        OwnedMatchRecord {
            line_number: self.line_number,
            line_byte_offset: self.line_byte_offset,
            line_bytes: self.line_bytes.to_vec(),
            match_spans: self.match_spans.clone(),
            is_context: self.is_context,
        }
    }
}

/// Owned version of MatchRecord for long-term storage or inter-thread messaging.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedMatchRecord {
    pub line_number: usize,
    pub line_byte_offset: usize,
    pub line_bytes: Vec<u8>,
    pub match_spans: Vec<(usize, usize)>,
    pub is_context: bool,
}

/// Consumer interface receiving match events from the search loop.
pub trait MatchSink<'a> {
    /// Callback invoked for each matching line found.
    fn on_match(&mut self, record: MatchRecord<'a>) -> io::Result<()>;
}

/// Core search engine interface.
pub trait Matcher: Send + Sync {
    /// Execute search over raw byte buffer, emitting matches to sink.
    /// Returns total number of matches found.
    fn find_matches<'a>(&self, buffer: &'a [u8], sink: &mut dyn MatchSink<'a>)
    -> io::Result<usize>;
}

/// Presentation and output rendering interface.
pub trait Printer: Send + Sync {
    /// Render formatted matches for a single file.
    fn print_file_matches(&mut self, path: &Path, matches: &[MatchRecord<'_>]) -> io::Result<()>;

    /// Render execution summary (counts of matches and files).
    fn print_summary(&mut self, total_matches: usize, total_files: usize) -> io::Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dir_entry_lazy_path() {
        let entry = DirEntry::new(
            PathBuf::from("/home/user"),
            b"test.rs".to_vec(),
            false,
            false,
            Some(1024),
        );
        assert_eq!(entry.full_path(), PathBuf::from("/home/user/test.rs"));
        assert_eq!(entry.name_display(), "test.rs");
    }

    #[test]
    fn test_buffer_ref_deref() {
        let data = b"hello world";
        let buf_borrowed = BufferRef::Borrowed(data);
        assert_eq!(&*buf_borrowed, b"hello world");

        let buf_owned = BufferRef::Owned(b"owned slice".to_vec());
        assert_eq!(&*buf_owned, b"owned slice");
    }

    #[test]
    fn test_dir_entry_is_file() {
        let file_entry = DirEntry::new(PathBuf::from("."), b"file.rs".to_vec(), false, false, None);
        assert!(file_entry.is_file());

        let dir_entry = DirEntry::new(PathBuf::from("."), b"src".to_vec(), true, false, None);
        assert!(!dir_entry.is_file());

        let link_entry = DirEntry::new(PathBuf::from("."), b"link.rs".to_vec(), false, true, None);
        assert!(!link_entry.is_file());
    }

    struct MockFileOnlyPredicate;
    impl EntryPredicate for MockFileOnlyPredicate {
        fn matches(&self, entry: &DirEntry) -> io::Result<bool> {
            Ok(entry.is_file())
        }
    }

    #[test]
    fn test_entry_predicate_contract() {
        let predicate = MockFileOnlyPredicate;
        let file_entry = DirEntry::new(PathBuf::from("."), b"main.rs".to_vec(), false, false, None);
        let dir_entry = DirEntry::new(PathBuf::from("."), b"target".to_vec(), true, false, None);

        assert!(predicate.matches(&file_entry).unwrap());
        assert!(!predicate.matches(&dir_entry).unwrap());
    }
}
