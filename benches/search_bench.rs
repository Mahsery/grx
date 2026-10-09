use criterion::{Criterion, Throughput, black_box, criterion_group, criterion_main};
use grx::core::{MatchRecord, MatchSink, Matcher};
use grx::dsl::{QueryExpr, SearchPattern};
use grx::search::{BooleanEngine, SimdLiteralMatcher, find_line_bounds, is_binary};

struct NullSink(usize);

impl<'a> MatchSink<'a> for NullSink {
    #[inline]
    fn on_match(&mut self, _record: MatchRecord<'a>) -> std::io::Result<()> {
        self.0 += 1;
        Ok(())
    }
}

fn verify_matches(matcher: &dyn Matcher, corpus: &[u8], name: &str) {
    let mut sink = NullSink(0);
    let count = matcher.find_matches(corpus, &mut sink).unwrap();
    assert_eq!((count, sink.0), (150, 150), "{name} corpus contract");
}

fn representative_corpus() -> Vec<u8> {
    let non_match_line =
        b"pub fn disconnect_from_service(host: &str, port: u16) -> Result<Status, Error> {\n";
    let match_line = b"let conn = connect_to_database(\"localhost\", 5432)?;\n";

    let mut corpus = Vec::with_capacity(1024 * 1024);
    for i in 0..15000 {
        if i % 100 == 0 {
            corpus.extend_from_slice(match_line);
        } else {
            corpus.extend_from_slice(non_match_line);
        }
    }

    corpus
}

fn bench_simd_literal_search(c: &mut Criterion) {
    let mut group = c.benchmark_group("simd_literal");
    let corpus = representative_corpus();
    group.throughput(Throughput::Bytes(corpus.len() as u64));

    let matcher = SimdLiteralMatcher::new("connect_to_database", true);
    verify_matches(&matcher, &corpus, "simd literal");

    group.bench_function("literal_1mb_scan", |b| {
        b.iter(|| {
            let mut sink = NullSink(0);
            let count = matcher.find_matches(black_box(&corpus), &mut sink).unwrap();
            black_box(count);
        });
    });

    group.finish();
}

fn bench_compiled_matcher_stages(c: &mut Criterion) {
    let corpus = representative_corpus();
    let literal = |text: &str| {
        QueryExpr::Pattern(SearchPattern::Literal {
            text: text.into(),
            case_sensitive: Some(true),
        })
    };
    let cases = [
        (
            "exact_line",
            QueryExpr::Pattern(SearchPattern::ExactMatch {
                text: "let conn = connect_to_database(\"localhost\", 5432)?;".into(),
                case_sensitive: Some(true),
            }),
        ),
        (
            "boolean_and",
            QueryExpr::And(Box::new(literal("connect")), Box::new(literal("database"))),
        ),
        (
            "fuzzy_subsequence",
            QueryExpr::Pattern(SearchPattern::Fuzzy {
                text: "ctdb".into(),
                case_sensitive: Some(true),
            }),
        ),
    ];
    let mut group = c.benchmark_group("compiled_matchers");
    group.throughput(Throughput::Bytes(corpus.len() as u64));
    for (name, expression) in cases {
        let matcher = BooleanEngine::try_new(expression).unwrap();
        verify_matches(&matcher, &corpus, name);
        group.bench_function(name, |b| {
            b.iter(|| {
                let mut sink = NullSink(0);
                let count = matcher.find_matches(black_box(&corpus), &mut sink).unwrap();
                black_box((count, sink.0));
            })
        });
    }
    group.finish();
}

fn bench_binary_probe(c: &mut Criterion) {
    let mut group = c.benchmark_group("binary_probe");

    let clean_text = vec![b'a'; 1024];
    let mut binary_data = vec![b'a'; 1024];
    binary_data[512] = 0x00;
    assert!(!is_binary(&clean_text));
    assert!(is_binary(&binary_data));

    group.bench_function("clean_text_1kb", |b| {
        b.iter(|| {
            black_box(is_binary(black_box(&clean_text)));
        });
    });

    group.bench_function("binary_probe_1kb", |b| {
        b.iter(|| {
            black_box(is_binary(black_box(&binary_data)));
        });
    });

    group.finish();
}

fn bench_line_bounds(c: &mut Criterion) {
    let mut group = c.benchmark_group("line_bounds");
    let buffer = b"line 1: some prefix\nline 2: match is here\nline 3: suffix line\n";
    let match_offset = 28;
    let (start, end, line) = find_line_bounds(buffer, match_offset, 0, 1);
    assert_eq!(
        (&buffer[start..end], line),
        (b"line 2: match is here".as_slice(), 2)
    );

    group.bench_function("find_line_bounds", |b| {
        b.iter(|| {
            black_box(find_line_bounds(
                black_box(buffer),
                black_box(match_offset),
                0,
                1,
            ));
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_simd_literal_search,
    bench_compiled_matcher_stages,
    bench_binary_probe,
    bench_line_bounds
);
criterion_main!(benches);
