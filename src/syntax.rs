#[derive(Debug, Clone, Copy)]
pub struct SyntaxEntry {
    pub prefix: &'static str,
    pub description: &'static str,
}

pub const DSL_SYNTAX: &[SyntaxEntry] = &[
    SyntaxEntry {
        prefix: "root",
        description: "Exact traversal root (repeat for multiple roots)",
    },
    SyntaxEntry {
        prefix: "p",
        description: "Include paths; ? separates alternatives, trailing / selects directories",
    },
    SyntaxEntry {
        prefix: "np",
        description: "Exclude paths; ? separates alternatives",
    },
    SyntaxEntry {
        prefix: "kind",
        description: "Discover file, dir, link, bin, or text entries",
    },
    SyntaxEntry {
        prefix: "k",
        description: "Alias for kind: (file, dir, link, bin, text)",
    },
    SyntaxEntry {
        prefix: "t",
        description: "Filter filetypes without changing mode; ?-separated alternatives",
    },
    SyntaxEntry {
        prefix: "nt",
        description: "Exclude filetypes; ? separates alternatives",
    },
    SyntaxEntry {
        prefix: "in",
        description: "Search contents of selected files",
    },
    SyntaxEntry {
        prefix: "ns",
        description: "Exclude content lines; ? separates alternatives",
    },
    SyntaxEntry {
        prefix: "lit",
        description: "Literal substring without DSL interpretation",
    },
    SyntaxEntry {
        prefix: "fz",
        description: "Scored ordered-subsequence fuzzy matching",
    },
    SyntaxEntry {
        prefix: "re",
        description: "Explicit regular expression",
    },
    SyntaxEntry {
        prefix: "larger",
        description: "File size lower bound",
    },
    SyntaxEntry {
        prefix: "smaller",
        description: "File size upper bound",
    },
    SyntaxEntry {
        prefix: "newer",
        description: "Modification age upper bound",
    },
    SyntaxEntry {
        prefix: "older",
        description: "Modification age lower bound",
    },
    SyntaxEntry {
        prefix: "d",
        description: "Traversal depth from each root",
    },
    SyntaxEntry {
        prefix: "max",
        description: "Maximum matching lines per file",
    },
    SyntaxEntry {
        prefix: "head",
        description: "First result records after sorting",
    },
    SyntaxEntry {
        prefix: "tail",
        description: "Last result records after sorting",
    },
    SyntaxEntry {
        prefix: "nth",
        description: "Select a one-based result index",
    },
    SyntaxEntry {
        prefix: "pick",
        description: "Interactive result selection",
    },
    SyntaxEntry {
        prefix: "sort",
        description: "Result sorting key",
    },
    SyntaxEntry {
        prefix: "ctx",
        description: "Content context lines",
    },
    SyntaxEntry {
        prefix: "near",
        description: "Content proximity constraint",
    },
    SyntaxEntry {
        prefix: "no-near",
        description: "Negated content proximity constraint",
    },
    SyntaxEntry {
        prefix: "str",
        description: "Binary printable-string extraction",
    },
    SyntaxEntry {
        prefix: "hex",
        description: "Hex byte content pattern",
    },
    SyntaxEntry {
        prefix: "mv",
        description: "Move selected entries",
    },
    SyntaxEntry {
        prefix: "cp",
        description: "Copy selected entries",
    },
    SyntaxEntry {
        prefix: "trash",
        description: "Trash selected entries",
    },
    SyntaxEntry {
        prefix: "rename",
        description: "Rename selected entries",
    },
    SyntaxEntry {
        prefix: "chmod",
        description: "Change selected entry permissions",
    },
    SyntaxEntry {
        prefix: "dry",
        description: "Preview actions",
    },
];
