use criterion::{Criterion, Throughput, black_box, criterion_group, criterion_main};
use grx::core::{MatchRecord, MatchSink, Matcher};
use grx::search::{SimdLiteralMatcher, find_line_bounds, is_binary};

struct NullSink(usize);

impl<'a> MatchSink<'a> for NullSink {
    #[inline]
    fn on_match(&mut self, _record: MatchRecord<'a>) -> std::io::Result<()> {
        self.0 += 1;
        Ok(())
    }
}

fn bench_simd_literal_search(c: &mut Criterion) {
    let mut group = c.benchmark_group("simd_literal");

    // Generate 1 MB text buffer with repeated non-matching lines and periodic matches (1 in 100)
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

    group.throughput(Throughput::Bytes(corpus.len() as u64));

    let matcher = SimdLiteralMatcher::new("connect_to_database", true);

    group.bench_function("literal_1mb_scan", |b| {
        b.iter(|| {
            let mut sink = NullSink(0);
            let count = matcher.find_matches(black_box(&corpus), &mut sink).unwrap();
            black_box(count);
        });
    });

    group.finish();
}

fn bench_binary_probe(c: &mut Criterion) {
    let mut group = c.benchmark_group("binary_probe");

    let clean_text = vec![b'a'; 1024];
    let mut binary_data = vec![b'a'; 1024];
    binary_data[512] = 0x00;

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
    let match_offset = 28; // inside line 2

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
    bench_binary_probe,
    bench_line_bounds
);
criterion_main!(benches);
