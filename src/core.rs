use std::borrow::Cow;
use std::fmt;
use std::io;
use std::ops::Deref;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterDecision {
    Include,

    Exclude,

    SkipDir,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    pub parent: PathBuf,

    pub name: Vec<u8>,

    pub is_dir: bool,

    pub is_symlink: bool,

    pub size: Option<u64>,
}

impl DirEntry {
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

    pub fn name_display(&self) -> Cow<'_, str> {
        String::from_utf8_lossy(&self.name)
    }

    #[inline]
    pub fn is_file(&self) -> bool {
        !self.is_dir && !self.is_symlink
    }
}

pub trait EntryPredicate: Send + Sync {
    fn matches(&self, entry: &DirEntry) -> io::Result<bool>;
}

pub trait EntrySink: Send + Sync {
    fn on_entry(&mut self, entry: &DirEntry) -> io::Result<()>;
}

pub trait IgnoreFilter: Send + Sync {
    fn filter(&self, parent: &Path, name: &[u8], is_dir: bool) -> FilterDecision;

    fn for_dir(&self, dir: &Path) -> Option<std::sync::Arc<dyn IgnoreFilter>> {
        let _ = dir;
        None
    }
}

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

pub trait DirectoryWalker: Send + Sync {
    fn walk(
        &self,
        roots: &[PathBuf],
        filter: std::sync::Arc<dyn IgnoreFilter>,
        on_entry: &(dyn Fn(DirEntry) -> Result<(), WalkerError> + Sync),
    ) -> Result<(), WalkerError>;
}

pub enum BufferRef<'a> {
    Borrowed(&'a [u8]),

    Owned(Vec<u8>),

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

pub trait ContentReader: Send + Sync {
    fn read<'a>(&self, path: &Path, scratch: &'a mut Vec<u8>) -> io::Result<BufferRef<'a>>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextOrigin {
    pub primary_line: usize,
    pub near_spans: Vec<(usize, usize)>,
}

macro_rules! primary_selection {
    () => {
        pub fn retain_for_primary(&mut self, selected: &std::collections::BTreeSet<usize>) -> bool {
            self.context_origins
                .retain(|origin| selected.contains(&origin.primary_line));
            self.near_spans = self
                .context_origins
                .iter()
                .flat_map(|origin| origin.near_spans.iter().copied())
                .collect();
            self.near_spans.sort_unstable();
            self.near_spans.dedup();
            if !self.is_context && selected.contains(&self.line_number) {
                return true;
            }
            if self.context_origins.is_empty() {
                return false;
            }
            self.is_context = true;
            self.match_spans.clear();
            self.occurrence_count = 0;
            true
        }
    };
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchRecord<'a> {
    pub line_number: usize,

    pub line_byte_offset: usize,

    pub line_bytes: &'a [u8],
    pub source_line_bytes: &'a [u8],
    pub bytes_searched: Option<usize>,

    pub match_spans: Vec<(usize, usize)>,

    pub occurrence_count: usize,

    pub near_spans: Vec<(usize, usize)>,
    pub context_origins: Vec<ContextOrigin>,

    pub is_context: bool,
}

impl<'a> MatchRecord<'a> {
    primary_selection!();
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
            source_line_bytes: line_bytes,
            bytes_searched: None,
            occurrence_count: match_spans.len().max(1),
            match_spans,
            near_spans: Vec::new(),
            context_origins: Vec::new(),
            is_context: false,
        }
    }

    pub fn with_near_spans(mut self, near_spans: Vec<(usize, usize)>) -> Self {
        self.near_spans = near_spans;
        self
    }

    pub fn context(line_number: usize, line_byte_offset: usize, line_bytes: &'a [u8]) -> Self {
        Self {
            line_number,
            line_byte_offset,
            line_bytes,
            source_line_bytes: line_bytes,
            bytes_searched: None,
            match_spans: Vec::new(),
            occurrence_count: 0,
            near_spans: Vec::new(),
            context_origins: Vec::new(),
            is_context: true,
        }
    }

    pub fn to_owned(&self) -> OwnedMatchRecord {
        OwnedMatchRecord {
            line_number: self.line_number,
            line_byte_offset: self.line_byte_offset,
            line_bytes: self.line_bytes.to_vec(),
            source_line_bytes: self.source_line_bytes.to_vec(),
            bytes_searched: self.bytes_searched,
            match_spans: self.match_spans.clone(),
            occurrence_count: self.occurrence_count,
            near_spans: self.near_spans.clone(),
            context_origins: self.context_origins.clone(),
            is_context: self.is_context,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedMatchRecord {
    pub line_number: usize,
    pub line_byte_offset: usize,
    pub line_bytes: Vec<u8>,
    pub source_line_bytes: Vec<u8>,
    pub bytes_searched: Option<usize>,
    pub match_spans: Vec<(usize, usize)>,

    pub occurrence_count: usize,
    pub near_spans: Vec<(usize, usize)>,
    pub context_origins: Vec<ContextOrigin>,
    pub is_context: bool,
}

impl OwnedMatchRecord {
    primary_selection!();
}

pub trait MatchSink<'a> {
    fn on_match(&mut self, record: MatchRecord<'a>) -> io::Result<()>;
}

pub trait Matcher: Send + Sync {
    fn find_matches<'a>(&self, buffer: &'a [u8], sink: &mut dyn MatchSink<'a>)
    -> io::Result<usize>;
}

pub trait Printer: Send + Sync {
    fn print_file_matches(&mut self, path: &Path, matches: &[MatchRecord<'_>]) -> io::Result<()>;

    fn print_summary(&mut self, total_matches: usize, total_files: usize) -> io::Result<()>;
}
