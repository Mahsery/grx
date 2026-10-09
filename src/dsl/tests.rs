use super::*;

#[test]
fn reject_malformed_expressions_and_modifiers() {
    for args in [
        vec!["foo", "AND"],
        vec!["foo", "OR"],
        vec!["AND", "foo"],
        vec!["foo", "AND", "AND", "bar"],
        vec!["foo", "NOT"],
        vec!["foo", "ctx:bad"],
        vec!["foo", "top:-1"],
        vec!["hex:zz"],
        vec!["NEAR", "foo"],
        vec!["foo", "NEAR", "AND"],
    ] {
        assert!(DslParser::parse(args.clone()).is_err(), "{args:?}");
    }
}

#[test]
fn test_parse_hex_pattern() {
    let args = vec!["hex:48??e5"];
    let query = DslParser::parse(args).unwrap();

    assert!(query.include_binaries);
    assert_eq!(
        query.expr,
        Some(QueryExpr::Pattern(SearchPattern::Hex(vec![
            Some(0x48),
            None,
            Some(0xE5),
        ])))
    );
    for malformed in ["hex:abc", "hex:48?", "hex:abé", "in:hex:abc"] {
        assert!(
            DslParser::parse([malformed]).is_err(),
            "must reject incomplete/non-ASCII byte pattern: {malformed}"
        );
    }
}

#[test]
fn test_parse_fuzzy_tokens_and_quote_diagnostics() {
    for text in ["from,ptr,err", "foo bar", "snake_case", "CamelCase"] {
        let arg = format!("fz:{text}");
        let q = DslParser::parse([arg]).unwrap();
        assert!(
            matches!(q.expr, Some(QueryExpr::Pattern(SearchPattern::Fuzzy {text: ref actual,..})) if actual == text)
        );
    }
    assert!(DslParser::parse(["fz:"]).is_err());
}

#[test]
fn test_parse_proximity_filters_and_infix() {
    let q = DslParser::parse(["needle", "near:2,other", "no-near:1,bad"]).unwrap();
    assert_eq!(q.proximity_filters.len(), 2);
    assert!(DslParser::parse(["kind:file", "needle", "near:2,other"]).is_err());
}

#[test]
fn name_patterns_use_the_shared_matcher() {
    let cases: &[(&str, &[u8], bool)] = &[
        ("report", b"my_report.txt", true),
        ("report", b"other.txt", false),
        ("^report", b"report_final.txt", true),
        ("^report", b"annual_report.txt", false),
        ("report$", b"annual_report", true),
        ("report$", b"report.txt", false),
        ("^report$", b"report", true),
        ("^report$", b" report", false),
        ("report..2026", b"report_q1_2026.pdf", true),
        ("report..2026", b"2026_report", false),
        ("=report.md", b"report.md", true),
        ("=report.md", b"Report.md", true),
        ("=report.md", b"my_report.md", false),
        ("Report", b"Report.txt", true),
        ("Report", b"report.txt", false),
        ("foo..$", b"foo_bar.txt", true),
        ("foo..$", b"bar", false),
        ("foo..bar$", b"foo_test_bar", true),
        ("foo..bar$", b"foo_test_bar_baz", false),
    ];
    for (pattern, name, expected) in cases {
        let query = DslParser::parse(["kind:file", pattern]).unwrap();
        let matcher =
            crate::search::CompiledExpr::from_query_expr(query.name_expr.as_ref().unwrap())
                .unwrap();
        assert_eq!(matcher.eval_on_line(name), *expected, "{pattern}: {name:?}");
    }
}

#[test]
fn test_basename_excludes_ni() {
    assert!(
        DslParser::parse(["ni:debug"])
            .unwrap_err()
            .contains("retired")
    );
    let q = DslParser::parse(["kind:file", "NOT", "debug"]).unwrap();
    assert!(matches!(q.name_expr, Some(QueryExpr::Not(_))));
}

#[test]
fn test_kind_selectors() {
    for value in ["file", "dir", "link", "bin", "text"] {
        let arg = format!("kind:{value}");
        assert!(DslParser::parse([arg]).unwrap().is_discovery());
    }
    assert!(DslParser::parse(["kind:file", "kind:dir"]).is_err());
    assert!(DslParser::parse(["kind:dir", "t:rs"]).is_err());
}

#[test]
fn test_unrecognized_long_options_rejected() {
    assert!(DslParser::parse(["needle", "--nonesuch"]).is_err());
    let q = DslParser::parse(["needle", "root:src/"]).unwrap();
    assert_eq!(q.targets, vec![PathBuf::from("src/")]);
}

#[test]
fn test_empty_selectors_rejected() {
    assert!(DslParser::parse(vec!["p:"]).is_err());
    assert!(DslParser::parse(vec!["path:"]).is_err());
    assert!(DslParser::parse(vec!["np:"]).is_err());
    assert!(DslParser::parse(vec!["no-path:"]).is_err());
    assert!(DslParser::parse(vec!["nt:"]).is_err());
    assert!(DslParser::parse(vec!["no-type:"]).is_err());
    assert!(DslParser::parse(vec!["ns:"]).is_err());
    assert!(DslParser::parse(vec!["not:"]).is_err());
    assert!(DslParser::parse(vec!["w:"]).is_err());
    assert!(DslParser::parse(vec!["@"]).is_err());
}

#[test]
fn test_dir_file_link_selectors() {
    for arg in ["dir:src", "file:report", "link:alias"] {
        assert!(DslParser::parse([arg]).is_err());
    }
    assert!(DslParser::parse(["kind:link", "in:needle"]).is_err());
}

#[test]
fn test_nth_and_pick_dsl_parsing() {
    assert_eq!(DslParser::parse(["nth:-2"]).unwrap().nth, Some(-2));
    assert!(DslParser::parse(["nth:0"]).is_err());
    assert!(DslParser::parse(["nth:2", "head:3"]).is_err());
    assert!(DslParser::parse(["nth:2", "pick:"]).is_err());
}

#[test]
fn test_cli_flags_are_owned_by_option_parser() {
    let err_f = DslParser::parse(vec!["hit", "-F"]).unwrap_err();
    assert!(err_f.contains("CLI flag '-F' must be handled by the CLI option parser"));

    let err_iv = DslParser::parse(vec!["hit", "-iv"]).unwrap_err();
    assert!(err_iv.contains("CLI flag '-iv' must be handled by the CLI option parser"));

    let err_i = DslParser::parse(vec!["hit", "-i", "p:flags.txt"]).unwrap_err();
    assert!(err_i.contains("CLI flag '-i' must be handled by the CLI option parser"));

    let err_m1 = DslParser::parse(vec!["hit", "-m1"]).unwrap_err();
    assert!(err_m1.contains("CLI flag '-m1' must be handled by the CLI option parser"));

    let err_c2 = DslParser::parse(vec!["hit", "-C2"]).unwrap_err();
    assert!(err_c2.contains("CLI flag '-C2' must be handled by the CLI option parser"));

    let err_im1 = DslParser::parse(vec!["hit", "-im1"]).unwrap_err();
    assert!(err_im1.contains("CLI flag '-im1' must be handled by the CLI option parser"));

    let err_meq = DslParser::parse(vec!["hit", "-m=1"]).unwrap_err();
    assert!(err_meq.contains("CLI flag '-m=1' must be handled by the CLI option parser"));

    let err_teq = DslParser::parse(vec!["hit", "-t=rs"]).unwrap_err();
    assert!(err_teq.contains("CLI flag '-t=rs' must be handled by the CLI option parser"));

    let err_neg = DslParser::parse(vec!["hit", "-koseoglu"]).unwrap_err();
    assert!(err_neg.contains("unknown command '-koseoglu'"));
    assert!(err_neg.contains("To search for this string, use 'lit:-koseoglu' instead."));

    let err_debug = DslParser::parse(vec!["hit", "-debug"]).unwrap_err();
    assert!(err_debug.contains("unknown command '-debug'"));
    assert!(err_debug.contains("To search for this string, use 'lit:-debug' instead."));

    let err_wall = DslParser::parse(vec!["-Wall"]).unwrap_err();
    assert!(err_wall.contains("unknown command '-Wall'"));
    assert!(err_wall.contains("To search for this string, use 'lit:-Wall' instead."));

    let q_ns = DslParser::parse(vec!["hit", "ns:F"]).unwrap();
    assert!(q_ns.expr.is_some());
    let q_ns_m1 = DslParser::parse(vec!["hit", "ns:m1"]).unwrap();
    assert!(q_ns_m1.expr.is_some());
    let q_ns_kose = DslParser::parse(vec!["hit", "ns:koseoglu"]).unwrap();
    assert!(q_ns_kose.expr.is_some());
}

#[test]
fn test_typo_prefix_diagnostics_and_quoted_escapes() {
    for arg in ["ext:rs", "ptah:src", "nmae:report"] {
        assert!(
            DslParser::parse([arg])
                .unwrap_err()
                .contains("unknown prefix")
        );
    }
    assert!(DslParser::parse(["lit:ext:rs"]).is_ok());
}

#[test]
fn test_suggest_size_prefix_typos() {
    assert!(DslParser::parse(["size:10M"]).is_err());
    assert!(DslParser::parse(["larger:10MiB"]).is_ok());
}

#[test]
fn test_parenthesized_disambiguation_and_double_dash() {
    let q = DslParser::parse(["kind:file", "(alpha", "OR", "beta)", "NOT", "gamma"]).unwrap();
    assert!(matches!(q.name_expr, Some(QueryExpr::And(_, _))));
    assert!(DslParser::parse(["(", "foo"]).is_err());
    assert!(DslParser::parse(["() "]).is_err());
}

#[test]
fn grouping_and_domain_boundaries_have_explicit_meaning() {
    let a = DslParser::parse(["kind:file", "(foo OR bar)", "NOT", "baz"]).unwrap();
    let b = DslParser::parse(["kind:file", "(", "foo", "OR", "bar", ")", "NOT", "baz"]).unwrap();
    assert_eq!(a, b);
    assert!(
        matches!(a.name_expr, Some(QueryExpr::And(ref alternatives, ref exclusion)) if matches!(**alternatives, QueryExpr::Or(_, _)) && matches!(**exclusion, QueryExpr::Not(_)))
    );
    for args in [
        vec!["kind:file", "foo", "OR", "in:bar"],
        vec!["t:rs", "OR", "foo"],
        vec!["(", "t:rs", "foo", ")"],
    ] {
        assert!(DslParser::parse(args).is_err());
    }
}

#[test]
fn malformed_operators_cannot_disappear_between_domains() {
    for args in [
        vec!["kind:file", "engine", "AND", "AND", "p:src"],
        vec!["t:rs", "AND", "AND", "in:needle"],
        vec!["t:rs", "AND", "OR", "in:needle"],
        vec!["kind:file", "(", "engine", "AND", ")", "p:src"],
    ] {
        assert!(DslParser::parse(args).is_err());
    }
    assert!(DslParser::parse(["kind:file", "engine", "AND", "p:src"]).is_ok());
}

#[test]
fn invalid_modal_combinations_are_rejected_regardless_of_order() {
    for args in [
        vec!["kind:dir", "t:rs"],
        vec!["t:rs", "kind:dir"],
        vec!["kind:link", "nt:rs"],
        vec!["kind:file", "d:2", "memory", "kind:dir"],
        vec!["kind:dir", "in:memory"],
        vec!["kind:dir", "larger:2MiB"],
        vec!["ns:debug"],
        vec!["kind:file", "memory", "ns:debug"],
    ] {
        assert!(DslParser::parse(args).is_err());
    }
    assert!(DslParser::parse(["kind:file", "memory", "in:needle", "ns:debug"]).is_ok());
}

#[test]
fn filetype_alternatives_reject_missing_values() {
    for prefix in ["t:", "nt:"] {
        for values in ["", "?rs", "rs?", "rs??py", "rs? ?py", "."] {
            assert!(DslParser::parse([format!("{prefix}{values}")]).is_err());
        }
        assert!(DslParser::parse([format!("{prefix}rs?py")]).is_ok());
    }
}

#[test]
fn scalar_declarations_are_idempotent_and_reject_conflicts() {
    for (a, b) in [
        ("d:1", "d:2"),
        ("max:1", "max:2"),
        ("sort:path", "sort:size"),
        ("head:1", "head:2"),
        ("tail:1", "tail:2"),
        ("nth:1", "nth:2"),
        ("ctx:1", "ctx:2"),
        ("-n", "-N"),
        ("str:3", "str:4"),
        ("cp:a", "cp:b"),
    ] {
        assert!(DslParser::parse(["needle", a, b]).is_err(), "{a} {b}");
        assert!(DslParser::parse(["needle", b, a]).is_err(), "{b} {a}");
        assert_eq!(
            DslParser::parse(["needle", a, a]).unwrap(),
            DslParser::parse(["needle", a]).unwrap(),
            "{a}"
        );
    }
    assert_eq!(
        DslParser::parse(["sort:path", "--reverse", "--reverse"]).unwrap(),
        DslParser::parse(["sort:path", "--reverse"]).unwrap()
    );
}
