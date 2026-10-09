use super::ast::{
    EntryKind, ProximityFilter, SearchPattern, SizePredicate, SortKey, TimePredicate,
};
use super::parser::DslParser;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    Sort(SortKey),

    Reverse,

    And,
    LeftParen,
    RightParen,
    ContentPattern(SearchPattern),

    Or,

    Not,

    NegativeTerm(Vec<SearchPattern>),

    PathExclude(Vec<String>),

    PathInclude(Vec<String>),

    Kind(EntryKind),

    SizePredicate(SizePredicate),

    TimePredicate(TimePredicate),

    TypeExclude(Vec<String>),

    TypeInclude(Vec<String>),

    Target(PathBuf),

    Stdin,

    FollowSymlinks(bool),

    Depth(usize),

    MaxCount(usize),

    Head(usize),

    TailCount(usize),

    Nth(isize),

    Pick,

    LineNumber(bool),

    Context(usize),

    BinaryStrings(usize),

    Pattern(SearchPattern),

    Proximity(ProximityFilter),

    Action(crate::ops::ActionKind),

    DryRun,
}

impl DslParser {
    pub fn is_known_type_shorthand(ext: &str) -> bool {
        matches!(
            ext.to_ascii_lowercase().as_str(),
            "rs" | "rust"
                | "c"
                | "cpp"
                | "c++"
                | "py"
                | "python"
                | "go"
                | "golang"
                | "js"
                | "javascript"
                | "ts"
                | "typescript"
                | "toml"
                | "json"
                | "yaml"
                | "yml"
                | "md"
                | "markdown"
                | "sh"
                | "shell"
                | "bash"
                | "fish"
                | "zsh"
                | "java"
                | "kotlin"
                | "kt"
                | "zig"
                | "lua"
                | "sql"
                | "html"
                | "css"
                | "web"
                | "code"
                | "data"
                | "doc"
        )
    }

    pub(crate) fn is_short_cli_flag(s: &str) -> bool {
        if !s.starts_with('-') || s.starts_with("--") || s.len() <= 1 {
            return false;
        }
        let rest = &s[1..];

        if rest.starts_with('@') || rest.starts_with("w:") {
            return false;
        }

        const BOOLEAN_FLAGS: &[char] = &[
            'a', 'b', 'c', 'h', 'i', 'l', 'n', 'o', 'p', 'q', 'r', 's', 'u', 'v', 'w', 'x', 'E',
            'F', 'H', 'I', 'L', 'N', 'R', 'S', '0',
        ];

        if rest.chars().all(|c| BOOLEAN_FLAGS.contains(&c)) {
            return true;
        }

        let chars: Vec<char> = rest.chars().collect();
        let mut idx = 0;
        while idx < chars.len() && BOOLEAN_FLAGS.contains(&chars[idx]) {
            idx += 1;
        }

        if idx < chars.len() {
            let flag = chars[idx];
            let remaining: String = chars[idx + 1..].iter().collect();

            if remaining.is_empty()
                && matches!(
                    flag,
                    'm' | 'C' | 'B' | 'A' | 'd' | 'j' | 'M' | 't' | 'T' | 'g' | 'e' | 'f' | 'Z'
                )
            {
                return true;
            }

            if matches!(flag, 'm' | 'C' | 'B' | 'A' | 'd' | 'j' | 'M') {
                let val = remaining.strip_prefix('=').unwrap_or(&remaining);
                if !val.is_empty() && val.chars().all(|c| c.is_ascii_digit()) {
                    return true;
                }
            }

            if matches!(
                flag,
                'm' | 'C' | 'B' | 'A' | 'd' | 'j' | 'M' | 't' | 'T' | 'g' | 'e' | 'f' | 'Z'
            ) && remaining.starts_with('=')
                && remaining.len() > 1
            {
                return true;
            }
        }

        false
    }

    pub fn compile_term_to_pattern(mut term: &str, is_whole_word: bool) -> SearchPattern {
        let anchor_start = if term.starts_with('^') && term.len() > 1 {
            term = &term[1..];
            true
        } else {
            false
        };
        let anchor_end = if term.ends_with('$') && term.len() > 1 {
            term = &term[..term.len() - 1];
            true
        } else {
            false
        };

        let has_wildcard = term.contains("..");

        if !anchor_start && !anchor_end && !is_whole_word && !has_wildcard {
            return SearchPattern::Literal {
                text: term.to_string(),
                case_sensitive: None,
            };
        }

        let has_upper = term.chars().any(|c| c.is_uppercase());
        let mut flags = String::from("(?");
        if !has_upper && !is_whole_word {
            flags.push('i');
        }
        if anchor_start || anchor_end {
            flags.push('m');
        }
        flags.push(')');
        let case_prefix = if flags == "(?)" { "" } else { &flags };

        let body = if has_wildcard {
            let parts: Vec<String> = term
                .split("..")
                .map(|s| format!("({})", regex::escape(s)))
                .collect();
            parts.join(".*")
        } else {
            regex::escape(term)
        };

        let starts_with_word = term
            .chars()
            .next()
            .is_some_and(|c| c.is_alphanumeric() || c == '_');
        let ends_with_word = term
            .chars()
            .last()
            .is_some_and(|c| c.is_alphanumeric() || c == '_');

        let mut pattern = String::new();
        pattern.push_str(case_prefix);

        if anchor_start {
            pattern.push('^');
        } else if is_whole_word && starts_with_word {
            pattern.push_str(r"\b");
        }

        pattern.push_str(&body);

        if anchor_end {
            pattern.push('$');
        } else if is_whole_word && ends_with_word {
            pattern.push_str(r"\b");
        }

        SearchPattern::Regex(pattern)
    }

    pub(crate) fn parse_hex_pattern(s: &str) -> Option<Vec<Option<u8>>> {
        let clean = s.replace([' ', '\t', '\n'], "");
        if !clean.is_ascii() || !clean.len().is_multiple_of(2) {
            return None;
        }
        let mut result = Vec::new();

        let mut chars = clean.chars().peekable();
        while let (Some(c1), Some(c2)) = (chars.next(), chars.next()) {
            if c1 == '?' && c2 == '?' {
                result.push(None);
            } else {
                let hex_pair = format!("{c1}{c2}");
                let byte = u8::from_str_radix(&hex_pair, 16).ok()?;
                result.push(Some(byte));
            }
        }

        if result.is_empty() {
            None
        } else {
            Some(result)
        }
    }
}
