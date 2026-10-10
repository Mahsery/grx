#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchPattern {
    Literal {
        text: String,
        case_sensitive: Option<bool>,
    },

    ExactLiteral(String),

    ExactMatch {
        text: String,
        case_sensitive: Option<bool>,
    },

    Fuzzy {
        text: String,
        case_sensitive: Option<bool>,
    },

    Regex(String),

    Hex(Vec<Option<u8>>),

    EmptySet,
}

impl SearchPattern {
    pub fn is_case_sensitive(&self) -> bool {
        match self {
            SearchPattern::Literal {
                text,
                case_sensitive,
            } => case_sensitive.unwrap_or_else(|| text.chars().any(|c| c.is_uppercase())),
            SearchPattern::ExactLiteral(_) => true,
            SearchPattern::ExactMatch {
                text,
                case_sensitive,
            }
            | SearchPattern::Fuzzy {
                text,
                case_sensitive,
            } => case_sensitive.unwrap_or_else(|| text.chars().any(char::is_uppercase)),
            SearchPattern::Regex(_) => true,
            SearchPattern::Hex(_) => true,
            SearchPattern::EmptySet => true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoolExpr<T> {
    Pattern(T),

    And(Box<BoolExpr<T>>, Box<BoolExpr<T>>),

    Or(Box<BoolExpr<T>>, Box<BoolExpr<T>>),

    Not(Box<BoolExpr<T>>),
}

impl<T> BoolExpr<T> {
    pub fn any(&self, predicate: &impl Fn(&T) -> bool) -> bool {
        match self {
            Self::Pattern(value) => predicate(value),
            Self::And(a, b) | Self::Or(a, b) => a.any(predicate) || b.any(predicate),
            Self::Not(inner) => inner.any(predicate),
        }
    }
}

pub type QueryExpr = BoolExpr<SearchPattern>;
pub type SelectorExpr = BoolExpr<EntryPredicate>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryPredicate {
    Path(String),
    Glob(String),
    Type(String),
    Size(SizePredicate),
    Time(TimePredicate),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum QueryMode {
    #[default]
    Discovery,
    Content,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PatternSyntax {
    #[default]
    Dsl,
    Fixed,
    Regex,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Dir,
    Link,
    Bin,
    Text,
}

pub fn parse_kind(s: &str) -> Result<EntryKind, String> {
    let val = s
        .strip_prefix("kind:")
        .or_else(|| s.strip_prefix("k:"))
        .unwrap_or(s);
    match val {
        "file" => Ok(EntryKind::File),
        "dir" => Ok(EntryKind::Dir),
        "link" => Ok(EntryKind::Link),
        "bin" => Ok(EntryKind::Bin),
        "text" => Ok(EntryKind::Text),
        other => Err(format!(
            "invalid kind '{other}' in '{s}'. Expected: file, dir, link, bin, text"
        )),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizePredicate {
    Larger(u64),
    Smaller(u64),
}

impl SizePredicate {
    #[inline]
    pub fn matches(&self, size: u64) -> bool {
        match self {
            SizePredicate::Larger(val) => size > *val,
            SizePredicate::Smaller(val) => size < *val,
        }
    }
}

pub fn parse_size_predicate(s: &str) -> Result<SizePredicate, String> {
    if let Some(rest) = s.strip_prefix("larger:") {
        parse_size_bytes(rest, s).map(SizePredicate::Larger)
    } else if let Some(rest) = s.strip_prefix("smaller:") {
        parse_size_bytes(rest, s).map(SizePredicate::Smaller)
    } else {
        Err(format!("Invalid size predicate '{s}'"))
    }
}

pub fn parse_size_bytes(val: &str, full: &str) -> Result<u64, String> {
    let val = val.trim();
    if val.is_empty() {
        return Err(format!("Empty size value in '{full}'"));
    }

    let num_end = val.find(|c: char| !c.is_ascii_digit()).unwrap_or(val.len());
    let (digits, unit) = val.split_at(num_end);
    let unit = unit.trim();

    if digits.is_empty() {
        return Err(format!(
            "Invalid size format in '{full}': expected integer with optional unit (B, K, M, G, KB, MB, GB, KiB, MiB, GiB)"
        ));
    }

    let multiplier: u64 = match unit {
        "" | "b" | "B" => 1,
        "k" | "K" | "kb" | "KB" | "Kb" | "kib" | "KiB" => 1024,
        "m" | "M" | "mb" | "MB" | "Mb" | "mib" | "MiB" => 1024 * 1024,
        "g" | "G" | "gb" | "GB" | "Gb" | "gib" | "GiB" => 1024 * 1024 * 1024,
        _ => {
            return Err(format!(
                "Invalid size format in '{full}': unknown size unit '{unit}' (expected B, K, M, G, KB, MB, GB, KiB, MiB, GiB)"
            ));
        }
    };

    let n = digits
        .parse::<u64>()
        .map_err(|_| format!("Size overflow in '{full}'"))?;
    n.checked_mul(multiplier)
        .ok_or_else(|| format!("Size overflow in '{full}'"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimePredicate {
    Newer(std::time::Duration),
    Older(std::time::Duration),
}

impl TimePredicate {
    pub fn matches(&self, mtime: std::time::SystemTime, reference: std::time::SystemTime) -> bool {
        match self {
            TimePredicate::Newer(duration) => match reference.duration_since(mtime) {
                Ok(age) => age < *duration,
                Err(_) => true,
            },
            TimePredicate::Older(duration) => match reference.duration_since(mtime) {
                Ok(age) => age > *duration,
                Err(_) => false,
            },
        }
    }
}

pub fn parse_time_predicate(s: &str) -> Result<TimePredicate, String> {
    if let Some(rest) = s.strip_prefix("newer:") {
        parse_duration_seconds(rest, s).map(TimePredicate::Newer)
    } else if let Some(rest) = s.strip_prefix("older:") {
        parse_duration_seconds(rest, s).map(TimePredicate::Older)
    } else {
        Err(format!("Invalid time predicate '{s}'"))
    }
}

pub fn parse_duration_seconds(val: &str, full: &str) -> Result<std::time::Duration, String> {
    let val = val.trim();
    if val.is_empty() {
        return Err(format!("Empty time value in '{full}'"));
    }

    let (digits, unit_secs) = if let Some(rest) = val.strip_suffix('d') {
        (rest, 86400u64)
    } else if let Some(rest) = val.strip_suffix('h') {
        (rest, 3600u64)
    } else if let Some(rest) = val.strip_suffix('m') {
        (rest, 60u64)
    } else if let Some(rest) = val.strip_suffix('s') {
        (rest, 1u64)
    } else {
        return Err(format!(
            "Invalid time format in '{full}': expected unit s, m, h, or d"
        ));
    };

    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return Err(format!(
            "Invalid time value in '{full}': expected integer followed by unit s, m, h, or d"
        ));
    }

    let n = digits
        .parse::<u64>()
        .map_err(|_| format!("Time overflow in '{full}'"))?;
    let secs = n
        .checked_mul(unit_secs)
        .ok_or_else(|| format!("Time overflow in '{full}'"))?;
    Ok(std::time::Duration::from_secs(secs))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    Size,

    SizeDesc,

    Modified,

    ModifiedDesc,

    Path,

    PathDesc,

    Len,

    LenDesc,

    LineNum,

    LineNumDesc,

    Count,

    CountDesc,
}

impl SortKey {
    pub fn reversed(self) -> Self {
        match self {
            Self::Size => Self::SizeDesc,
            Self::SizeDesc => Self::Size,
            Self::Modified => Self::ModifiedDesc,
            Self::ModifiedDesc => Self::Modified,
            Self::Path => Self::PathDesc,
            Self::PathDesc => Self::Path,
            Self::Len => Self::LenDesc,
            Self::LenDesc => Self::Len,
            Self::LineNum => Self::LineNumDesc,
            Self::LineNumDesc => Self::LineNum,
            Self::Count => Self::CountDesc,
            Self::CountDesc => Self::Count,
        }
    }
}

pub fn is_valid_sort_key(key: &str) -> bool {
    let key_lower = key.to_ascii_lowercase();
    let clean = key_lower.strip_prefix('-').unwrap_or(&key_lower);
    matches!(
        clean,
        "size"
            | "bytes"
            | "smallest"
            | "largest"
            | "modified"
            | "time"
            | "age"
            | "date"
            | "newest"
            | "recent"
            | "oldest"
            | "path"
            | "name"
            | "len"
            | "length"
            | "path-len"
            | "line-len"
            | "linelen"
            | "shortest"
            | "longest"
            | "line"
            | "line-num"
            | "linenum"
            | "count"
    )
}

pub fn parse_sort_key(key: &str, reverse: bool) -> Result<SortKey, String> {
    let key_lower = key.to_ascii_lowercase();
    let (is_rev, clean) = if let Some(stripped) = key_lower.strip_prefix('-') {
        (!reverse, stripped)
    } else {
        (reverse, key_lower.as_str())
    };
    let base = match clean {
        "size" | "bytes" | "smallest" => SortKey::Size,
        "largest" => SortKey::SizeDesc,
        "modified" | "time" | "age" | "date" | "newest" | "recent" => SortKey::Modified,
        "oldest" => SortKey::ModifiedDesc,
        "path" | "name" => SortKey::Path,
        "len" | "length" | "path-len" | "line-len" | "linelen" | "shortest" => SortKey::Len,
        "longest" => SortKey::LenDesc,
        "line" | "line-num" | "linenum" => SortKey::LineNum,
        "count" => SortKey::Count,
        _ => {
            return Err(format!(
                "unknown sort key '{key}'. Expected: size, modified, path, len, line, count"
            ));
        }
    };
    Ok(if is_rev { base.reversed() } else { base })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProximityFilter {
    pub term: String,

    pub window: usize,

    pub inverted: bool,
}
