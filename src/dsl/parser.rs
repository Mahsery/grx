use super::ast::{
    BoolExpr, EntryKind, EntryPredicate, PatternSyntax, ProximityFilter, QueryExpr, QueryMode,
    SearchPattern, parse_kind, parse_size_predicate, parse_sort_key, parse_time_predicate,
};
use super::query::{Query, assign_setting};
use super::token::Token;
use std::path::PathBuf;

#[derive(Clone)]
enum Operand {
    Bare(SearchPattern),
    Content(SearchPattern),
    Selector(super::ast::SelectorExpr),
}
#[derive(Clone)]
enum ScopedToken {
    Atom(Operand),
    And,
    Or,
    Not,
    Open,
    Close,
}
type ResolvedExpressions = (
    Option<QueryExpr>,
    Option<QueryExpr>,
    Option<super::ast::SelectorExpr>,
);

#[derive(Clone)]
enum ExprToken<T> {
    Atom(BoolExpr<T>),
    And,
    Or,
    Not,
    Open,
    Close,
}

pub struct DslParser;

impl DslParser {
    fn parse_filter_values(value: &str, argument: &str) -> Result<Vec<String>, String> {
        if ["re:", "lit:", "fz:", "hex:"]
            .iter()
            .any(|prefix| value.starts_with(prefix))
        {
            if value
                .split_once(':')
                .is_some_and(|(_, rest)| rest.is_empty())
            {
                return Err(format!("Empty filter value in '{argument}'"));
            }
            return Ok(vec![value.to_owned()]);
        }
        let mut values = Vec::new();
        let mut current = String::new();
        let mut chars = value.chars().peekable();
        while let Some(ch) = chars.next() {
            match ch {
                '\\' if chars.peek() == Some(&'?') => {
                    chars.next();
                    current.push('?');
                }
                '?' => {
                    if current.is_empty() {
                        return Err(format!("Empty filter alternative in '{argument}'"));
                    }
                    if values.len() >= 128 {
                        return Err("Expression complexity exceeds the supported limit".into());
                    }
                    values.push(std::mem::take(&mut current));
                }
                _ => current.push(ch),
            }
        }
        if current.is_empty() {
            return Err(format!("Empty filter alternative in '{argument}'"));
        }
        values.push(current);
        Ok(values)
    }

    fn parse_filetypes(value: &str, argument: &str) -> Result<Vec<String>, String> {
        Self::parse_filter_values(value, argument)?
            .into_iter()
            .map(|piece| {
                let kind = piece.trim().trim_start_matches('.');
                if kind.is_empty() {
                    Err(format!("Empty filetype alternative in '{argument}'"))
                } else {
                    Ok(kind.to_string())
                }
            })
            .collect()
    }

    fn owns_argument_value(s: &str) -> bool {
        s.starts_with('=')
            || s.split_once(':').is_some_and(|(prefix, _)| {
                crate::syntax::DSL_SYNTAX
                    .iter()
                    .any(|entry| entry.prefix == prefix)
            })
    }

    fn lex_grouped_argument(
        s: &str,
        group_depth: &mut usize,
        config: Option<&crate::config::Config>,
        compile: &impl Fn(&str, bool) -> SearchPattern,
    ) -> Result<Vec<(Token, String)>, String> {
        let mut tokens = Vec::new();
        let mut atom = String::new();
        let mut atom_started = false;
        let mut atom_depth = 0usize;
        let mut quote = None;

        let mut regex_classes = Vec::<bool>::new();
        let mut chars = s.chars().peekable();
        let flush = |atom: &mut String,
                     started: &mut bool,
                     tokens: &mut Vec<(Token, String)>|
         -> Result<(), String> {
            if *started {
                let value = std::mem::take(atom);
                tokens.push((
                    Self::classify_with_compiler_and_config(&value, config, compile)?,
                    value,
                ));
                *started = false;
            }
            Ok(())
        };
        while let Some(ch) = chars.next() {
            if let Some(delimiter) = quote {
                if ch == delimiter {
                    quote = None;
                } else if ch == '\\' {
                    let next = chars.next().ok_or("Trailing escape in quoted expression")?;
                    if next != delimiter && next != '\\' {
                        atom.push(ch);
                    }
                    atom.push(next);
                } else {
                    atom.push(ch);
                }
                continue;
            }
            if ch == '\\' {
                atom_started = true;
                if let Some(first) = regex_classes.last_mut() {
                    *first = false;
                }
                atom.push(ch);
                if let Some(next) = chars.next() {
                    atom.push(next);
                }
                continue;
            }
            if let Some(first) = regex_classes.last().copied() {
                atom.push(ch);
                if ch == '[' && !first {
                    regex_classes.push(true);
                } else if ch == ']' && !first {
                    regex_classes.pop();
                } else if ch != '^' || !first {
                    *regex_classes.last_mut().expect("class exists") = false;
                }
                continue;
            }
            match ch {
                '\'' | '"' => {
                    quote = Some(ch);
                    atom_started = true;
                }
                '(' if !atom_started => {
                    *group_depth += 1;
                    tokens.push((Token::LeftParen, "(".into()));
                }
                '(' => {
                    atom_depth += 1;
                    atom.push(ch);
                }
                ')' if atom_depth > 0 => {
                    atom_depth -= 1;
                    atom.push(ch);
                }
                ')' => {
                    flush(&mut atom, &mut atom_started, &mut tokens)?;
                    if *group_depth == 0 {
                        return Err("Unmatched closing parenthesis".into());
                    }
                    *group_depth -= 1;
                    tokens.push((Token::RightParen, ")".into()));
                }
                '[' if ["re:", "in:re:", "p:re:", "np:re:", "ns:re:"]
                    .iter()
                    .any(|prefix| atom.starts_with(prefix)) =>
                {
                    atom_started = true;
                    regex_classes.push(true);
                    atom.push(ch);
                }
                ch if ch.is_whitespace() && atom_depth == 0 => {
                    flush(&mut atom, &mut atom_started, &mut tokens)?
                }
                _ => {
                    atom_started = true;
                    atom.push(ch);
                }
            }
        }
        if quote.is_some() {
            return Err("Unclosed quote in grouped expression".into());
        }
        if atom_depth != 0 || !regex_classes.is_empty() {
            return Err("Unclosed pattern delimiter in grouped expression".into());
        }
        flush(&mut atom, &mut atom_started, &mut tokens)?;
        Ok(tokens)
    }

    pub fn parse<I, S>(args: I) -> Result<Query, String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        Self::parse_with_external_expr(args, None)
    }

    pub fn parse_with_external_expr<I, S>(
        args: I,
        external_expr: Option<QueryExpr>,
    ) -> Result<Query, String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        Self::parse_with_compiler(args, external_expr, Self::compile_term_to_pattern)
    }

    pub fn parse_with_compiler<I, S, F>(
        args: I,
        external_expr: Option<QueryExpr>,
        compile: F,
    ) -> Result<Query, String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
        F: Fn(&str, bool) -> SearchPattern,
    {
        Self::parse_with_compiler_and_base(args, external_expr, Query::default(), compile)
    }

    pub fn parse_with_compiler_and_base<I, S, F>(
        args: I,
        external_expr: Option<QueryExpr>,
        query: Query,
        compile: F,
    ) -> Result<Query, String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
        F: Fn(&str, bool) -> SearchPattern,
    {
        Self::parse_with_compiler_base_and_config(args, external_expr, query, None, compile)
    }

    pub fn parse_with_compiler_base_and_config<I, S, F>(
        args: I,
        external_expr: Option<QueryExpr>,
        mut query: Query,
        config: Option<&crate::config::Config>,
        compile: F,
    ) -> Result<Query, String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
        F: Fn(&str, bool) -> SearchPattern,
    {
        let mut raw_tokens = Vec::new();
        let mut after_double_dash = false;
        let mut group_depth = 0usize;
        let mut iter = args.into_iter().peekable();
        while let Some(arg) = iter.next() {
            let s = arg.as_ref();
            if !after_double_dash && s == "--" {
                after_double_dash = true;
                continue;
            }
            if !after_double_dash && s == "--exec" {
                let cmd: Vec<String> = iter.map(|a| a.as_ref().to_string()).collect();
                if cmd.is_empty() {
                    return Err("--exec requires at least one command argument".into());
                }
                query.exec = cmd;
                break;
            }
            if !after_double_dash && (s == "-X" || s == "--exec-batch") {
                let cmd: Vec<String> = iter.map(|a| a.as_ref().to_string()).collect();
                if cmd.is_empty() {
                    return Err("-X/--exec-batch requires at least one command argument".into());
                }
                query.exec_batch = cmd;
                break;
            }
            if !after_double_dash && s == "--nth" {
                if let Some(next_arg) = iter.next() {
                    let n: isize = next_arg.as_ref().parse().map_err(|_| {
                        "Invalid index in '--nth': expected non-zero integer".to_string()
                    })?;
                    if n == 0 {
                        return Err("--nth index cannot be 0".into());
                    }
                    raw_tokens.push((Token::Nth(n), "--nth".into()));
                    continue;
                } else {
                    return Err("--nth requires an index argument".into());
                }
            }
            if !after_double_dash && s == "--pick" {
                raw_tokens.push((Token::Pick, "--pick".into()));
                continue;
            }
            if !after_double_dash && s == "--sort" {
                if let Some(next_arg) = iter.peek() {
                    let next_s = next_arg.as_ref();
                    if crate::dsl::is_valid_sort_key(next_s) {
                        let is_reversed = next_s.starts_with('-');
                        let key_str = next_s.trim_start_matches('-');
                        let key = parse_sort_key(key_str, is_reversed)?;
                        raw_tokens.push((Token::Sort(key), format!("--sort {next_s}")));
                        iter.next();
                        continue;
                    }
                }
                raw_tokens.push((Token::Sort(crate::dsl::SortKey::Path), "--sort".into()));
                continue;
            }
            if !after_double_dash && (s == "--reverse" || s == "--sort-reverse") {
                raw_tokens.push((Token::Reverse, s.to_string()));
                continue;
            }
            if !after_double_dash && (s == "--line-number" || s == "-n") {
                raw_tokens.push((Token::LineNumber(true), s.to_string()));
                continue;
            }
            if !after_double_dash && (s == "--no-line-number" || s == "-N") {
                raw_tokens.push((Token::LineNumber(false), s.to_string()));
                continue;
            }
            if !after_double_dash
                && (s == "("
                    || s == ")"
                    || (query.pattern_syntax == PatternSyntax::Dsl
                        && (s.starts_with('(')
                            || (group_depth > 0
                                && s.ends_with(')')
                                && !Self::owns_argument_value(s)))))
            {
                raw_tokens.extend(Self::lex_grouped_argument(
                    s,
                    &mut group_depth,
                    config,
                    &compile,
                )?);
                continue;
            }
            let tok = if after_double_dash {
                Token::Pattern(SearchPattern::Literal {
                    text: s.into(),
                    case_sensitive: None,
                })
            } else {
                Self::classify_with_compiler_and_config(s, config, &compile)?
            };
            raw_tokens.push((tok, s.to_string()));
        }

        if raw_tokens.len() > 256 {
            return Err("Expression complexity exceeds the supported limit".into());
        }
        let mut expressions = Vec::new();
        let base_has_type = query.has_type_predicates();
        if let Some(expr) = query.selector_expr.take() {
            expressions.push(ScopedToken::Atom(Operand::Selector(expr)));
        }
        let mut sort_reversed = false;
        let mut has_negative_content = false;
        let mut has_positive_content = external_expr.is_some();
        let mut has_type = base_has_type;
        let mut explicit_content =
            external_expr.is_some() || query.mode == QueryMode::Content || query.empty_pattern_set;
        for (token, _) in raw_tokens {
            let negative_path = matches!(&token, Token::PathExclude(_));
            let negative_type = matches!(&token, Token::TypeExclude(_));
            use Token::*;
            match token {
                Pattern(p) => expressions.push(ScopedToken::Atom(Operand::Bare(p))),
                ContentPattern(p) => {
                    has_positive_content = true;
                    explicit_content = true;
                    expressions.push(ScopedToken::Atom(Operand::Content(p)));
                }
                NegativeTerm(patterns) => {
                    has_negative_content = true;
                    explicit_content = true;
                    expressions.push(ScopedToken::Not);
                    expressions.push(ScopedToken::Open);
                    for (index, pattern) in patterns.into_iter().enumerate() {
                        if index > 0 {
                            expressions.push(ScopedToken::Or);
                        }
                        expressions.push(ScopedToken::Atom(Operand::Content(pattern)));
                    }
                    expressions.push(ScopedToken::Close);
                }
                And => expressions.push(ScopedToken::And),
                Or => expressions.push(ScopedToken::Or),
                Not => expressions.push(ScopedToken::Not),
                LeftParen => expressions.push(ScopedToken::Open),
                RightParen => expressions.push(ScopedToken::Close),
                PathInclude(paths) | PathExclude(paths) => {
                    let negative = negative_path;
                    let mut leaves = paths
                        .into_iter()
                        .map(|path| BoolExpr::Pattern(EntryPredicate::Path(path)));
                    let first = leaves.next().ok_or("Empty path constraint")?;
                    let expr = leaves.fold(first, |left, right| {
                        BoolExpr::Or(Box::new(left), Box::new(right))
                    });
                    expressions.push(ScopedToken::Atom(Operand::Selector(if negative {
                        BoolExpr::Not(Box::new(expr))
                    } else {
                        expr
                    })));
                }
                TypeInclude(types) | TypeExclude(types) => {
                    let negative = negative_type;
                    has_type = true;
                    let mut leaves = types
                        .into_iter()
                        .map(|value| BoolExpr::Pattern(EntryPredicate::Type(value)));
                    let first = leaves.next().ok_or("Empty filetype constraint")?;
                    let expr = leaves.fold(first, |left, right| {
                        BoolExpr::Or(Box::new(left), Box::new(right))
                    });
                    expressions.push(ScopedToken::Atom(Operand::Selector(if negative {
                        BoolExpr::Not(Box::new(expr))
                    } else {
                        expr
                    })));
                }
                SizePredicate(value) => {
                    expressions.push(ScopedToken::Atom(Operand::Selector(BoolExpr::Pattern(
                        EntryPredicate::Size(value),
                    ))));
                }
                TimePredicate(value) => {
                    expressions.push(ScopedToken::Atom(Operand::Selector(BoolExpr::Pattern(
                        EntryPredicate::Time(value),
                    ))));
                }
                Kind(kind) => {
                    assign_setting(&mut query.kind, kind, "kind")?;
                }
                Target(path) => query.targets.push(path),
                Stdin => {
                    query.stdin = true;
                    explicit_content = true;
                }
                Action(action) => {
                    assign_setting(&mut query.action, action, "file action")?;
                }
                DryRun => query.dry_run = true,
                Sort(key) => assign_setting(&mut query.sort, key, "sort")?,
                Reverse => sort_reversed = true,
                FollowSymlinks(value) => {
                    assign_setting(&mut query.follow_symlinks, value, "symlink following")?
                }
                Depth(value) => assign_setting(&mut query.max_depth, value, "depth")?,
                MaxCount(value) => {
                    assign_setting(&mut query.max_count, value, "maximum matches per file")?
                }
                Head(value) => assign_setting(&mut query.head, value, "head")?,
                TailCount(value) => assign_setting(&mut query.tail, value, "tail")?,
                Nth(value) => assign_setting(&mut query.nth, value, "nth")?,
                Pick => query.pick = true,
                LineNumber(value) => {
                    assign_setting(&mut query.line_number, value, "line numbering")?
                }
                Context(value) => assign_setting(&mut query.context, value, "context")?,
                BinaryStrings(value) => {
                    assign_setting(
                        &mut query.binary_strings_min_len,
                        value,
                        "binary string minimum length",
                    )?;
                    query.include_binaries = true;
                    explicit_content = true;
                }
                Proximity(value) => query.proximity_filters.push(value),
            }
        }

        let names = query.kind.is_some();
        let has_bare = expressions
            .iter()
            .any(|token| matches!(token, ScopedToken::Atom(Operand::Bare(_))));
        if has_negative_content && !has_positive_content && !(has_bare && !names) {
            return Err("ns: requires a base content expression".into());
        }
        query.mode = if explicit_content || (has_bare && !names) {
            QueryMode::Content
        } else {
            QueryMode::Discovery
        };
        let (name_expr, content_expr, selector_expr) =
            Self::resolve_expressions(expressions, names)?;
        query.name_expr = name_expr;
        query.expr = match (external_expr, content_expr) {
            (Some(a), Some(b)) => Some(BoolExpr::And(Box::new(a), Box::new(b))),
            (a, b) => a.or(b),
        };
        query.selector_expr = selector_expr;
        if let Some(kind) = query.kind {
            if matches!(kind, EntryKind::Dir | EntryKind::Link)
                && (has_type || query.has_size_predicates() || query.mode == QueryMode::Content)
            {
                return Err(
                    "Directories and links cannot have filetype, size, or content constraints"
                        .into(),
                );
            }
            if kind == EntryKind::Bin {
                query.include_binaries = true;
                query.only_binaries = true;
            }
            if kind == EntryKind::Text {
                query.include_binaries = false;
            }
        } else if has_type {
            query.kind = Some(EntryKind::File);
        }
        if query.is_discovery()
            && (query.max_count.is_some()
                || query.context.is_some()
                || !query.proximity_filters.is_empty()
                || matches!(
                    query.sort,
                    Some(
                        crate::dsl::SortKey::Count
                            | crate::dsl::SortKey::CountDesc
                            | crate::dsl::SortKey::LineNum
                            | crate::dsl::SortKey::LineNumDesc
                    )
                ))
        {
            return Err(
                "Match limits, context, proximity, and line/count sorting require content search"
                    .into(),
            );
        }
        if !query.proximity_filters.is_empty() && query.expr.is_none() {
            return Err("Proximity requires a content expression".into());
        }
        if query.head.is_some() && query.tail.is_some() {
            return Err("Cannot combine head and tail".into());
        }
        if query.nth.is_some() && (query.head.is_some() || query.tail.is_some() || query.pick) {
            return Err("Cannot combine nth with head, tail, or pick".into());
        }
        if sort_reversed {
            query.sort = Some(query.sort.unwrap_or(crate::dsl::SortKey::Path).reversed());
        }
        if query.expr.as_ref().is_some_and(Self::contains_hex) {
            query.include_binaries = true;
        }
        Ok(query)
    }

    pub fn parse_search_pattern(s: &str) -> Result<SearchPattern, String> {
        Self::parse_search_pattern_with_compiler(s, &Self::compile_term_to_pattern)
    }

    pub(crate) fn parse_search_pattern_with_compiler(
        s: &str,
        compile: &impl Fn(&str, bool) -> SearchPattern,
    ) -> Result<SearchPattern, String> {
        if let Some(text) = s.strip_prefix("lit:") {
            return Ok(SearchPattern::Literal {
                text: text.into(),
                case_sensitive: None,
            });
        }
        if s.starts_with('@') {
            return Err("@ matching is retired; use = or lit:".into());
        }
        if let Some(text) = s.strip_prefix('=') {
            return Ok(SearchPattern::ExactMatch {
                text: text.into(),
                case_sensitive: None,
            });
        }
        if let Some(text) = s.strip_prefix("fz:") {
            if text.is_empty() {
                return Err("fz: requires a nonempty subsequence".into());
            }
            return Ok(SearchPattern::Fuzzy {
                text: text.into(),
                case_sensitive: None,
            });
        }
        if let Some(pattern) = s.strip_prefix("re:") {
            return Ok(SearchPattern::Regex(pattern.into()));
        }
        if let Some(bytes) = s.strip_prefix("hex:") {
            return Self::parse_hex_pattern(bytes)
                .filter(|bytes| !bytes.is_empty())
                .map(SearchPattern::Hex)
                .ok_or_else(|| format!("Invalid hex pattern '{s}': use byte pairs or ??"));
        }
        Ok(compile(s, false))
    }

    pub fn classify_token(s: &str) -> Result<Token, String> {
        Self::classify_with_compiler(s, &Self::compile_term_to_pattern)
    }

    pub fn classify_token_with_config(
        s: &str,
        config: Option<&crate::config::Config>,
    ) -> Result<Token, String> {
        Self::classify_with_compiler_and_config(s, config, &Self::compile_term_to_pattern)
    }

    pub(crate) fn classify_with_compiler(
        s: &str,
        compile: &impl Fn(&str, bool) -> SearchPattern,
    ) -> Result<Token, String> {
        Self::classify_with_compiler_and_config(s, None, compile)
    }

    pub(crate) fn classify_with_compiler_and_config(
        s: &str,
        _config: Option<&crate::config::Config>,
        compile: &impl Fn(&str, bool) -> SearchPattern,
    ) -> Result<Token, String> {
        if s.starts_with("ni:") {
            return Err("ni: is retired; use NOT with a name expression in discovery mode".into());
        }
        if !s.starts_with('=')
            && let Some((prefix, _)) = s.split_once(':')
            && !crate::syntax::DSL_SYNTAX
                .iter()
                .any(|entry| entry.prefix == prefix)
        {
            return Err(format!(
                "unknown prefix '{prefix}:'; use lit:{s} to match it literally"
            ));
        }
        if s == "NEAR" {
            return Err("NEAR infix is retired; use near:N,pattern or lit:NEAR".into());
        }
        if s == "(" {
            return Ok(Token::LeftParen);
        }
        if s == ")" {
            return Ok(Token::RightParen);
        }
        if let Some(value) = s.strip_prefix("lit:") {
            return Ok(Token::Pattern(SearchPattern::Literal {
                text: value.into(),
                case_sensitive: None,
            }));
        }
        if let Some(value) = s.strip_prefix("root:") {
            if value.is_empty() {
                return Err("root: requires an exact traversal location".into());
            }
            return Ok(Token::Target(crate::config::Config::expand_tilde(
                PathBuf::from(value),
            )));
        }
        if let Some(value) = s.strip_prefix("in:") {
            if value.is_empty() {
                return Err("in: requires a content expression".into());
            }
            return Ok(Token::ContentPattern(
                Self::parse_search_pattern_with_compiler(value, compile)?,
            ));
        }
        if s.starts_with('@') {
            return Err(
                "@ matching is retired; use = for exact matching or lit: to search @ literally"
                    .into(),
            );
        }

        if s == "AND" {
            return Ok(Token::And);
        }
        if s == "OR" {
            return Ok(Token::Or);
        }
        if s == "NOT" {
            return Ok(Token::Not);
        }

        if s.starts_with("fz:") {
            return Ok(Token::Pattern(Self::parse_search_pattern(s)?));
        }

        if let Some(rest) = s.strip_prefix("no-near:") {
            let (window, term) = Self::parse_proximity_args(rest)?;
            return Ok(Token::Proximity(ProximityFilter {
                term,
                window,
                inverted: true,
            }));
        }

        if let Some(rest) = s.strip_prefix("near:") {
            let (window, term) = Self::parse_proximity_args(rest)?;
            return Ok(Token::Proximity(ProximityFilter {
                term,
                window,
                inverted: false,
            }));
        }

        if let Some(rest) = s.strip_prefix("d:") {
            if rest.is_empty()
                || rest.starts_with('\\')
                || rest.starts_with('/')
                || rest.starts_with(':')
                || !rest.chars().all(|c| c.is_ascii_digit())
            {
            } else {
                return rest
                    .parse::<usize>()
                    .map(Token::Depth)
                    .map_err(|_| format!("Expected a non-negative integer in '{s}'"));
            }
        }

        if let Some(rest) = s.strip_prefix("max:") {
            return rest
                .parse::<usize>()
                .map(Token::MaxCount)
                .map_err(|_| format!("Expected a non-negative integer in '{s}'"));
        }

        if let Some(rest) = s.strip_prefix("head:") {
            if rest.starts_with(':') {
            } else {
                return rest
                    .parse::<usize>()
                    .map(Token::Head)
                    .map_err(|_| format!("Expected a non-negative integer in '{s}'"));
            }
        }

        if let Some(rest) = s.strip_prefix("tail:") {
            if rest.starts_with(':') {
            } else {
                return rest
                    .parse::<usize>()
                    .map(Token::TailCount)
                    .map_err(|_| format!("Expected a non-negative integer in '{s}'"));
            }
        }

        if let Some(rest) = s.strip_prefix("nth:") {
            if rest.starts_with(':') {
            } else {
                let n = rest
                    .parse::<isize>()
                    .map_err(|_| format!("Expected a non-zero integer in '{s}'"))?;
                if n == 0 {
                    return Err(format!(
                        "nth index cannot be 0 in '{s}' (use 1 for first, -1 for last)"
                    ));
                }
                return Ok(Token::Nth(n));
            }
        }

        if s == "pick:" {
            return Ok(Token::Pick);
        }

        if let Some(rest) = s.strip_prefix("ctx:") {
            if rest.starts_with(':') {
            } else {
                return rest
                    .parse::<usize>()
                    .map(Token::Context)
                    .map_err(|_| format!("Expected a non-negative integer in '{s}'"));
            }
        }

        if let Some(rest) = s.strip_prefix("str:") {
            if rest.is_empty() || rest.starts_with(':') || !rest.chars().all(|c| c.is_ascii_digit())
            {
            } else {
                return rest
                    .parse::<usize>()
                    .map(Token::BinaryStrings)
                    .map_err(|_| format!("Expected a non-negative integer in '{s}'"));
            }
        }

        if let Some(rest) = s.strip_prefix("sort:") {
            if rest.is_empty() {
                return Err("Expected sort key after sort:".into());
            }
            let is_reversed = rest.starts_with('-');
            let key_str = rest.trim_start_matches('-');
            let key = parse_sort_key(key_str, is_reversed)?;
            return Ok(Token::Sort(key));
        }

        if let Some(rest) = s.strip_prefix("t:") {
            return Ok(Token::TypeInclude(Self::parse_filetypes(rest, s)?));
        }

        if s.starts_with("kind:") || s.starts_with("k:") {
            let kind = parse_kind(s)?;
            return Ok(Token::Kind(kind));
        }

        if s.starts_with("larger:") || s.starts_with("smaller:") {
            let pred = parse_size_predicate(s)?;
            return Ok(Token::SizePredicate(pred));
        }

        if s.starts_with("newer:") || s.starts_with("older:") {
            let pred = parse_time_predicate(s)?;
            return Ok(Token::TimePredicate(pred));
        }

        if let Some(dest) = s.strip_prefix("mv:") {
            if dest.is_empty() {
                return Err(format!("Expected destination directory after '{s}'"));
            }
            return Ok(Token::Action(crate::ops::ActionKind::Move(
                crate::config::Config::expand_tilde(PathBuf::from(dest)),
            )));
        }
        if let Some(dest) = s.strip_prefix("cp:") {
            if dest.is_empty() {
                return Err(format!("Expected destination directory after '{s}'"));
            }
            return Ok(Token::Action(crate::ops::ActionKind::Copy(
                crate::config::Config::expand_tilde(PathBuf::from(dest)),
            )));
        }
        if let Some(rest) = s.strip_prefix("trash:") {
            if rest.is_empty() {
                return Ok(Token::Action(crate::ops::ActionKind::Trash));
            } else {
                return Err(format!(
                    "'{s}' takes no value. Use 'kind:file {rest} trash:'."
                ));
            }
        }
        if let Some(rest) = s.strip_prefix("rename:") {
            let (pat, rep) = if let Some((p, r)) = rest.split_once('/') {
                (p, r)
            } else if let Some((p, r)) = rest.split_once("->") {
                (p, r)
            } else {
                return Err(format!(
                    "Expected 'old/new' or 'old->new' pattern after 'rename:'. Got '{rest}'"
                ));
            };
            return Ok(Token::Action(crate::ops::ActionKind::Rename {
                pattern: pat.to_string(),
                replacement: rep.to_string(),
            }));
        }
        if let Some(mode) = s.strip_prefix("chmod:") {
            if mode.is_empty() {
                return Err(format!("Expected permissions mode after '{s}'"));
            }
            return Ok(Token::Action(crate::ops::ActionKind::Chmod(
                mode.to_string(),
            )));
        }
        if s == "dry:" || s == "--dry-run" || s == "--dry" {
            return Ok(Token::DryRun);
        }

        if let Some(rest) = s.strip_prefix("np:") {
            if rest.is_empty() {
                return Err(format!("Expected path after '{s}'"));
            }
            let paths = Self::parse_filter_values(rest, s)?;
            return Ok(Token::PathExclude(paths));
        }

        if let Some(rest) = s.strip_prefix("p:") {
            if rest.is_empty() {
                return Err(format!("Expected path after '{s}'"));
            }
            let paths = Self::parse_filter_values(rest, s)?;
            return Ok(Token::PathInclude(paths));
        }

        if let Some(rest) = s.strip_prefix("nt:") {
            return Ok(Token::TypeExclude(Self::parse_filetypes(rest, s)?));
        }

        if let Some(rest) = s.strip_prefix("ns:") {
            if rest.is_empty() {
                return Err("ns: requires a content expression".into());
            }
            let patterns = Self::parse_filter_values(rest, s)?
                .into_iter()
                .map(|value| Self::parse_search_pattern_with_compiler(&value, compile))
                .collect::<Result<Vec<_>, _>>()?;
            return Ok(Token::NegativeTerm(patterns));
        }

        if s.starts_with('=') {
            return Ok(Token::Pattern(Self::parse_search_pattern(s)?));
        }

        if s.starts_with("--") {
            if s == "--follow" {
                return Ok(Token::FollowSymlinks(true));
            } else if s == "--line-number" {
                return Ok(Token::LineNumber(true));
            } else if s == "--no-line-number" {
                return Ok(Token::LineNumber(false));
            } else if s == "--sort" {
                return Ok(Token::Sort(crate::dsl::SortKey::Path));
            } else if s == "--reverse" || s == "--sort-reverse" {
                return Ok(Token::Reverse);
            } else if let Some(rest) = s.strip_prefix("--sort=") {
                let key = parse_sort_key(rest, false)?;
                return Ok(Token::Sort(key));
            } else if let Some(rest) = s.strip_prefix("--head=") {
                let n: usize = rest
                    .parse()
                    .map_err(|_| format!("Invalid limit in '{s}': expected positive integer"))?;
                return Ok(Token::Head(n));
            } else if let Some(rest) = s.strip_prefix("--tail=") {
                let n: usize = rest
                    .parse()
                    .map_err(|_| format!("Invalid limit in '{s}': expected positive integer"))?;
                return Ok(Token::TailCount(n));
            } else if let Some(rest) = s.strip_prefix("--nth=") {
                let n: isize = rest
                    .parse()
                    .map_err(|_| format!("Invalid index in '{s}': expected non-zero integer"))?;
                if n == 0 {
                    return Err(format!("Invalid index in '{s}': nth index cannot be 0"));
                }
                return Ok(Token::Nth(n));
            } else if s == "--pick" {
                return Ok(Token::Pick);
            } else {
                return Err(format!("Unrecognized option '{s}'."));
            }
        }

        if s == "-n" {
            return Ok(Token::LineNumber(true));
        } else if s == "-N" {
            return Ok(Token::LineNumber(false));
        }
        if Self::is_short_cli_flag(s) {
            return Err(format!(
                "CLI flag '{s}' must be handled by the CLI option parser; use lit:{s} to match it literally."
            ));
        }

        if s.starts_with('-') && s.len() > 1 && !s.chars().skip(1).all(|c| c.is_ascii_digit()) {
            return Err(format!(
                "unknown command '{s}'. To search for this string, use 'lit:{s}' instead."
            ));
        }

        if s.starts_with('+') && s.len() > 1 {
            return Err(format!(
                "unknown command '{s}'. To search for this string, use 'lit:{s}' instead."
            ));
        }

        if let Some(rest) = s.strip_prefix("re:") {
            return Ok(Token::Pattern(SearchPattern::Regex(rest.to_string())));
        }

        if let Some(rest) = s.strip_prefix("hex:") {
            return Self::parse_hex_pattern(rest)
                .filter(|bytes| !bytes.is_empty())
                .map(|bytes| Token::Pattern(SearchPattern::Hex(bytes)))
                .ok_or_else(|| format!("Invalid hex pattern '{s}': use byte pairs or ??"));
        }

        if s == "-" {
            return Ok(Token::Stdin);
        }

        let is_deprecated_command = s.starts_with("%%")
            || (s.starts_with(':')
                && !s.starts_with("::")
                && Self::is_known_type_shorthand(&s[1..]))
            || (s.starts_with("*.") && Self::is_known_type_shorthand(&s[2..]))
            || (s.starts_with('!')
                && (s.starts_with("!/") || s.starts_with("!*") || s.ends_with('/')));

        if is_deprecated_command {
            return Err(format!(
                "unknown command '{s}'. To search for this string, use 'lit:{s}' instead."
            ));
        }

        if let Some((prefix, _)) = s.split_once(':') {
            return Err(format!(
                "invalid value for '{prefix}:'; use lit:{s} to match it literally"
            ));
        }

        Ok(Token::Pattern(compile(s, false)))
    }

    fn parse_proximity_args(rest: &str) -> Result<(usize, String), String> {
        let rest = rest.trim();
        if rest.is_empty() {
            return Err(
                "Proximity filter cannot be empty (expected near:pattern or near:N,pattern)"
                    .to_string(),
            );
        }
        if let Some((w_str, term_str)) = rest.split_once(',') {
            let w_trimmed = w_str.trim();
            let term_trimmed = term_str.trim();
            if let Ok(w) = w_trimmed.parse::<usize>() {
                if term_trimmed.is_empty() {
                    return Err(format!(
                        "Proximity filter '{rest}' has a window but no target term"
                    ));
                }
                return Ok((w, term_trimmed.to_string()));
            }
        }
        Ok((3, rest.to_string()))
    }

    fn resolve_expressions(
        tokens: Vec<ScopedToken>,
        names: bool,
    ) -> Result<ResolvedExpressions, String> {
        let mut needs_operand = true;
        let mut depth = 0usize;
        for token in &tokens {
            match token {
                ScopedToken::Atom(_) => needs_operand = false,
                ScopedToken::Open => {
                    depth += 1;
                    needs_operand = true;
                }
                ScopedToken::Close => {
                    if needs_operand || depth == 0 {
                        return Err("Unexpected closing parenthesis or empty group".into());
                    }
                    depth -= 1;
                    needs_operand = false;
                }
                ScopedToken::And | ScopedToken::Or => {
                    if needs_operand {
                        return Err("Boolean operator requires a preceding operand".into());
                    }
                    needs_operand = true;
                }
                ScopedToken::Not => needs_operand = true,
            }
        }
        if depth != 0 || (needs_operand && !tokens.is_empty()) {
            return Err("Unclosed group or trailing Boolean operator".into());
        }
        let mut domains: Vec<Option<u8>> = tokens
            .iter()
            .map(|token| match token {
                ScopedToken::Atom(Operand::Bare(_)) => Some(if names { 0 } else { 1 }),
                ScopedToken::Atom(Operand::Content(_)) => Some(1),
                ScopedToken::Atom(Operand::Selector(_)) => Some(2),
                _ => None,
            })
            .collect();
        let mut groups = Vec::new();
        for (index, token) in tokens.iter().enumerate() {
            match token {
                ScopedToken::Open => groups.push(index),
                ScopedToken::Close => {
                    let start = groups.pop().ok_or("Unmatched closing parenthesis")?;
                    let domain = domains[start + 1..index]
                        .iter()
                        .flatten()
                        .copied()
                        .next()
                        .ok_or("Empty expression group")?;
                    if domains[start + 1..index]
                        .iter()
                        .flatten()
                        .any(|value| *value != domain)
                    {
                        return Err("Parentheses cannot combine name, content, and selector expression domains".into());
                    }
                    domains[start] = Some(domain);
                    domains[index] = Some(domain);
                }
                _ => {}
            }
        }
        if !groups.is_empty() {
            return Err("Unclosed expression group".into());
        }
        for (index, token) in tokens.iter().enumerate() {
            if matches!(token, ScopedToken::And | ScopedToken::Or | ScopedToken::Not) {
                let right = domains[index + 1..]
                    .iter()
                    .flatten()
                    .copied()
                    .next()
                    .ok_or("Trailing Boolean operator")?;
                if matches!(token, ScopedToken::Not) {
                    domains[index] = Some(right);
                    continue;
                }
                let left = domains[..index]
                    .iter()
                    .rev()
                    .flatten()
                    .copied()
                    .next()
                    .ok_or("Boolean operator needs a left operand")?;
                if left == right {
                    domains[index] = Some(left);
                } else if matches!(token, ScopedToken::Or) {
                    return Err(
                        "OR cannot cross name, content, or selector expression domains".into(),
                    );
                }
            }
        }
        let mut name_tokens = Vec::new();
        let mut content_tokens = Vec::new();
        let mut selector_tokens = Vec::new();
        for (token, domain) in tokens.into_iter().zip(domains) {
            let Some(domain) = domain else {
                continue;
            };
            match token {
                ScopedToken::Atom(Operand::Bare(pattern)) => {
                    let target = if names {
                        &mut name_tokens
                    } else {
                        &mut content_tokens
                    };
                    target.push(ExprToken::Atom(BoolExpr::Pattern(pattern)));
                }
                ScopedToken::Atom(Operand::Content(pattern)) => {
                    content_tokens.push(ExprToken::Atom(BoolExpr::Pattern(pattern)))
                }
                ScopedToken::Atom(Operand::Selector(expr)) => {
                    selector_tokens.push(ExprToken::Atom(expr))
                }
                operator => {
                    fn convert<T>(token: ScopedToken) -> ExprToken<T> {
                        match token {
                            ScopedToken::And => ExprToken::And,
                            ScopedToken::Or => ExprToken::Or,
                            ScopedToken::Not => ExprToken::Not,
                            ScopedToken::Open => ExprToken::Open,
                            ScopedToken::Close => ExprToken::Close,
                            ScopedToken::Atom(_) => unreachable!(),
                        }
                    }
                    match domain {
                        0 => name_tokens.push(convert(operator)),
                        1 => content_tokens.push(convert(operator)),
                        _ => selector_tokens.push(convert(operator)),
                    }
                }
            }
        }
        Ok((
            Self::parse_boolean(&name_tokens)?,
            Self::parse_boolean(&content_tokens)?,
            Self::parse_boolean(&selector_tokens)?,
        ))
    }

    fn parse_boolean<T: Clone>(tokens: &[ExprToken<T>]) -> Result<Option<BoolExpr<T>>, String> {
        fn primary<T: Clone>(
            tokens: &[ExprToken<T>],
            index: &mut usize,
        ) -> Result<BoolExpr<T>, String> {
            match tokens.get(*index) {
                Some(ExprToken::Not) => {
                    *index += 1;
                    Ok(BoolExpr::Not(Box::new(primary(tokens, index)?)))
                }
                Some(ExprToken::Atom(expr)) => {
                    *index += 1;
                    Ok(expr.clone())
                }
                Some(ExprToken::Open) => {
                    *index += 1;
                    let expr = parse_or(tokens, index)?;
                    if !matches!(tokens.get(*index), Some(ExprToken::Close)) {
                        return Err("Expected closing parenthesis".into());
                    }
                    *index += 1;
                    Ok(expr)
                }
                _ => Err("Expected expression operand".into()),
            }
        }
        fn parse_and<T: Clone>(
            tokens: &[ExprToken<T>],
            index: &mut usize,
        ) -> Result<BoolExpr<T>, String> {
            let mut expr = primary(tokens, index)?;
            loop {
                match tokens.get(*index) {
                    Some(ExprToken::And) => *index += 1,
                    Some(ExprToken::Atom(_) | ExprToken::Not | ExprToken::Open) => {}
                    _ => break,
                }
                expr = BoolExpr::And(Box::new(expr), Box::new(primary(tokens, index)?));
            }
            Ok(expr)
        }
        fn parse_or<T: Clone>(
            tokens: &[ExprToken<T>],
            index: &mut usize,
        ) -> Result<BoolExpr<T>, String> {
            let mut expr = parse_and(tokens, index)?;
            while matches!(tokens.get(*index), Some(ExprToken::Or)) {
                *index += 1;
                expr = BoolExpr::Or(Box::new(expr), Box::new(parse_and(tokens, index)?));
            }
            Ok(expr)
        }
        if tokens.len() > 256 {
            return Err("Expression complexity exceeds the supported limit".into());
        }
        if tokens.is_empty() {
            return Ok(None);
        }
        let mut index = 0;
        let expr = parse_or(tokens, &mut index)?;
        if index != tokens.len() {
            return Err("Unexpected expression token".into());
        }
        Ok(Some(expr))
    }

    fn contains_hex(expr: &QueryExpr) -> bool {
        match expr {
            QueryExpr::Pattern(SearchPattern::Hex(_)) => true,
            QueryExpr::Pattern(_) => false,
            QueryExpr::And(a, b) | QueryExpr::Or(a, b) => {
                Self::contains_hex(a) || Self::contains_hex(b)
            }
            QueryExpr::Not(a) => Self::contains_hex(a),
        }
    }
}
