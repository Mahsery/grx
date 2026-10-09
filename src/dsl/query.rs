use super::ast::{
    BoolExpr, EntryKind, EntryPredicate, PatternSyntax, ProximityFilter, QueryExpr, QueryMode,
    SelectorExpr, SortKey,
};
use std::path::PathBuf;

pub(crate) fn assign_setting<T: Eq + std::fmt::Debug>(
    slot: &mut Option<T>,
    value: T,
    label: &str,
) -> Result<(), String> {
    if let Some(previous) = slot.as_ref()
        && previous != &value
    {
        return Err(format!(
            "Conflicting values for {label}: {previous:?} and {value:?}"
        ));
    }
    *slot = Some(value);
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Query {
    pub expr: Option<QueryExpr>,

    pub name_expr: Option<QueryExpr>,

    pub selector_expr: Option<SelectorExpr>,

    pub mode: QueryMode,

    pub pattern_syntax: PatternSyntax,

    pub targets: Vec<PathBuf>,

    pub stdin: bool,

    pub kind: Option<EntryKind>,

    pub search_hidden: Option<bool>,

    pub follow_symlinks: Option<bool>,

    pub respect_ignore: Option<bool>,

    pub include_binaries: bool,

    pub only_binaries: bool,

    pub case_sensitive: Option<bool>,

    pub max_depth: Option<usize>,

    pub max_count: Option<usize>,

    pub head: Option<usize>,

    pub tail: Option<usize>,

    pub nth: Option<isize>,

    pub pick: bool,

    pub context: Option<usize>,

    pub line_number: Option<bool>,

    pub binary_strings_min_len: Option<usize>,

    pub proximity_filters: Vec<ProximityFilter>,

    pub sort: Option<SortKey>,

    pub empty_pattern_set: bool,

    pub action: Option<crate::ops::ActionKind>,

    pub dry_run: bool,

    pub exec: Vec<String>,

    pub exec_batch: Vec<String>,
}

impl Query {
    pub fn is_discovery(&self) -> bool {
        self.mode == QueryMode::Discovery
    }

    pub fn constrain(&mut self, expr: SelectorExpr) {
        self.selector_expr = Some(match self.selector_expr.take() {
            Some(previous) => BoolExpr::And(Box::new(previous), Box::new(expr)),
            None => expr,
        });
    }

    pub fn has_type_predicates(&self) -> bool {
        self.selector_expr
            .as_ref()
            .is_some_and(|expr| expr.any(&|value| matches!(value, EntryPredicate::Type(_))))
    }

    pub fn has_size_predicates(&self) -> bool {
        self.selector_expr
            .as_ref()
            .is_some_and(|expr| expr.any(&|value| matches!(value, EntryPredicate::Size(_))))
    }

    pub fn has_entry_selectors(&self) -> bool {
        self.selector_expr.is_some()
            || self.name_expr.is_some()
            || self.kind.is_some()
            || !self.targets.is_empty()
    }
}
