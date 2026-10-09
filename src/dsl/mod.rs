pub mod ast;
pub mod parser;
pub mod query;
#[cfg(test)]
mod tests;
pub mod token;

pub use crate::syntax::{DSL_SYNTAX, SyntaxEntry};
pub use ast::{
    BoolExpr, EntryKind, EntryPredicate, PatternSyntax, ProximityFilter, QueryExpr, QueryMode,
    SearchPattern, SelectorExpr, SizePredicate, SortKey, TimePredicate, is_valid_sort_key,
    parse_kind, parse_size_bytes, parse_size_predicate, parse_sort_key, parse_time_predicate,
};
pub use parser::DslParser;
pub use query::Query;
pub(crate) use query::assign_setting;
pub use token::Token;

#[cfg(test)]
pub(crate) use std::path::PathBuf;
