use crate::core::{MatchRecord, MatchSink, Matcher};
use crate::search::{expand_context_lines, extract_printable_strings, is_binary_with_probe_size};
use std::io;

#[derive(Clone, Copy, Default)]
pub struct SearchOptions {
    pub max_count: Option<usize>,
    pub before_context: usize,
    pub after_context: usize,
    pub skip_binary: bool,
    pub only_binary: bool,
    pub binary_strings_min_len: Option<usize>,
    pub binary_null_probe_bytes: usize,

    pub has_proximity: bool,
}

pub struct SearchResult<'a> {
    pub records: Vec<MatchRecord<'a>>,
    pub matched_lines: usize,
    pub matches: usize,
}

pub fn process_ordered<T, R, I, F, C>(
    items: I,
    threads: usize,
    window: usize,
    process: &F,
    mut consume: C,
) -> io::Result<()>
where
    T: Send,
    R: Send,
    I: IntoIterator<Item = T>,
    F: Fn(T) -> io::Result<R> + Sync,
    C: FnMut(R) -> io::Result<std::ops::ControlFlow<()>>,
{
    use std::collections::BTreeMap;
    use std::sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::sync_channel,
    };
    let mut items = items.into_iter().enumerate();
    let Some(first) = items.next() else {
        return Ok(());
    };
    let evaluate = |item| {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| process(item)))
            .unwrap_or_else(|_| Err(io::Error::other("content worker panicked")))
    };
    let threads = threads.max(1);
    if threads == 1 {
        for (_, item) in std::iter::once(first).chain(items) {
            if consume(evaluate(item)?)?.is_break() {
                break;
            }
        }
        return Ok(());
    }
    let window = window.max(1);
    let canceled = AtomicBool::new(false);
    let (jobs_tx, jobs_rx) = sync_channel::<(usize, T)>(window);
    let jobs_rx = Mutex::new(jobs_rx);
    let (results_tx, results_rx) = sync_channel(window);
    std::thread::scope(|scope| {
        for _ in 0..threads {
            let jobs = &jobs_rx;
            let sender = results_tx.clone();
            let canceled = &canceled;
            let evaluate = &evaluate;
            scope.spawn(move || {
                loop {
                    let task = jobs
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .recv();
                    let Ok((index, item)) = task else {
                        break;
                    };
                    if canceled.load(Ordering::Acquire) {
                        break;
                    }
                    let result = evaluate(item);
                    if sender.send((index, result)).is_err() {
                        break;
                    }
                }
            });
        }
        drop(results_tx);
        let run = (|| {
            jobs_tx
                .send(first)
                .map_err(|_| io::Error::other("content workers disconnected"))?;
            let mut outstanding = 1;
            for _ in 1..window {
                let Some(task) = items.next() else {
                    break;
                };
                jobs_tx
                    .send(task)
                    .map_err(|_| io::Error::other("content workers disconnected"))?;
                outstanding += 1;
            }
            let mut next = 0;
            let mut pending = BTreeMap::new();
            while outstanding > 0 {
                let (index, result) = results_rx
                    .recv()
                    .map_err(|_| io::Error::other("content workers disconnected"))?;
                pending.insert(index, result);
                while let Some(result) = pending.remove(&next) {
                    outstanding -= 1;
                    next += 1;
                    if consume(result?)?.is_break() {
                        return Ok(());
                    }
                    if let Some(task) = items.next() {
                        jobs_tx
                            .send(task)
                            .map_err(|_| io::Error::other("content workers disconnected"))?;
                        outstanding += 1;
                    }
                }
            }
            Ok(())
        })();
        canceled.store(true, Ordering::Release);
        drop(jobs_tx);
        drop(results_rx);
        run
    })
}

pub struct BufferSearch<'m> {
    pub matcher: &'m dyn Matcher,
    pub options: SearchOptions,
}

struct LimitedCollector<'a> {
    records: Vec<MatchRecord<'a>>,
    limit: Option<usize>,
    stopped: bool,
    primary_count: usize,
}

impl<'a> MatchSink<'a> for LimitedCollector<'a> {
    fn on_match(&mut self, record: MatchRecord<'a>) -> io::Result<()> {
        let is_primary_match = !record.is_context;
        if is_primary_match {
            self.primary_count += 1;
        }
        self.records.push(record);
        if is_primary_match && self.limit.is_some_and(|limit| self.primary_count > limit) {
            self.records.pop();
            self.stopped = true;
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "match limit reached",
            ));
        }
        Ok(())
    }
}

impl<'m> BufferSearch<'m> {
    fn collect<'a>(
        &self,
        bytes: &'a [u8],
        limit: Option<usize>,
    ) -> io::Result<Vec<MatchRecord<'a>>> {
        let mut sink = LimitedCollector {
            records: Vec::new(),
            limit: if self.options.has_proximity {
                None
            } else {
                limit
            },
            stopped: false,
            primary_count: 0,
        };
        if limit != Some(0) {
            match self.matcher.find_matches(bytes, &mut sink) {
                Err(err) if !(sink.stopped && err.kind() == io::ErrorKind::Interrupted) => {
                    return Err(err);
                }
                _ => {}
            }
        }
        if let Some(limit) = limit.filter(|_| self.options.has_proximity) {
            retain_primary_neighborhood(&mut sink.records, limit);
        }
        Ok(sink.records)
    }

    pub fn search<'a>(&self, bytes: &'a [u8]) -> io::Result<Option<SearchResult<'a>>> {
        let probe_size = if self.options.binary_null_probe_bytes > 0 {
            self.options.binary_null_probe_bytes
        } else {
            1024
        };
        let binary = is_binary_with_probe_size(bytes, probe_size);
        if (self.options.skip_binary && binary) || (self.options.only_binary && !binary) {
            return Ok(None);
        }
        let mut records =
            if let Some(min_len) = self.options.binary_strings_min_len.filter(|_| binary) {
                let mut records = Vec::new();
                for (offset, string) in extract_printable_strings(bytes, min_len) {
                    let remaining = self
                        .options
                        .max_count
                        .map(|n| n.saturating_sub(records.len()));
                    if remaining == Some(0) {
                        break;
                    }
                    for mut record in self.collect(string, remaining)? {
                        record.line_number = records.len() + 1;
                        record.line_byte_offset += offset;
                        records.push(record);
                    }
                }
                records
            } else {
                self.collect(bytes, self.options.max_count)?
            };
        let matched_lines = records.iter().filter(|r| !r.is_context).count();
        let matches = records
            .iter()
            .filter(|r| !r.is_context)
            .map(|r| r.occurrence_count)
            .sum();
        if matched_lines > 0
            && !binary
            && (self.options.before_context > 0 || self.options.after_context > 0)
        {
            records = expand_context_lines(
                bytes,
                &records,
                self.options.before_context,
                self.options.after_context,
            );
        }
        for record in &mut records {
            let start = record.line_byte_offset;
            let mut end = start + record.line_bytes.len();
            if bytes.get(end) == Some(&b'\r') {
                end += 1;
            }
            if bytes.get(end) == Some(&b'\n') {
                end += 1;
            }
            record.source_line_bytes = &bytes[start..end];
            record.bytes_searched = Some(bytes.len());
        }
        Ok(Some(SearchResult {
            records,
            matched_lines,
            matches,
        }))
    }
}

fn retain_primary_neighborhood(records: &mut Vec<MatchRecord<'_>>, limit: usize) {
    let primary: std::collections::BTreeSet<_> = records
        .iter()
        .filter(|record| !record.is_context)
        .take(limit)
        .map(|record| record.line_number)
        .collect();
    records.retain_mut(|record| record.retain_for_primary(&primary));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::MatchSink;
    use crate::search::SimdLiteralMatcher;

    #[test]
    fn ordered_workers_reuse_threads_and_bound_outstanding_results() {
        use std::sync::{
            Barrier, Mutex,
            atomic::{AtomicUsize, Ordering},
        };
        let started = AtomicUsize::new(0);
        let consumed = AtomicUsize::new(0);
        let largest_window = AtomicUsize::new(0);
        let identities = Mutex::new(std::collections::HashSet::new());
        let start_pair = Barrier::new(2);
        let release_first = (Mutex::new(false), std::sync::Condvar::new());
        let process = |index: usize| {
            identities
                .lock()
                .unwrap()
                .insert(std::thread::current().id());
            let started = started.fetch_add(1, Ordering::SeqCst) + 1;
            largest_window.fetch_max(
                started.saturating_sub(consumed.load(Ordering::SeqCst)),
                Ordering::SeqCst,
            );
            if index < 2 {
                start_pair.wait();
            }
            if index == 0 {
                let mut released = release_first.0.lock().unwrap();
                while !*released {
                    released = release_first.1.wait(released).unwrap();
                }
            } else if index == 3 {
                *release_first.0.lock().unwrap() = true;
                release_first.1.notify_one();
            }
            Ok(index)
        };
        let mut output = Vec::new();
        process_ordered(0..20, 2, 4, &process, |index| {
            output.push(index);
            consumed.fetch_add(1, Ordering::SeqCst);
            Ok(std::ops::ControlFlow::Continue(()))
        })
        .unwrap();
        assert_eq!(output, (0..20).collect::<Vec<_>>());
        assert_eq!(identities.into_inner().unwrap().len(), 2);
        assert!(largest_window.load(Ordering::SeqCst) <= 4);
    }

    #[test]
    fn inline_processing_keeps_order_and_executes_on_the_caller() {
        let caller = std::thread::current().id();
        let mut output = Vec::new();
        process_ordered(
            0..5,
            1,
            8,
            &|index| {
                assert_eq!(std::thread::current().id(), caller);
                Ok(index)
            },
            |index| {
                output.push(index);
                Ok(std::ops::ControlFlow::Continue(()))
            },
        )
        .unwrap();
        assert_eq!(output, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn ordered_workers_cancel_without_reading_ahead_in_single_job_mode() {
        let processed = std::sync::atomic::AtomicUsize::new(0);
        let mut consumed = Vec::new();
        process_ordered(
            0..100,
            1,
            1,
            &|index| {
                processed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                Ok(index)
            },
            |index| {
                consumed.push(index);
                Ok(std::ops::ControlFlow::Break(()))
            },
        )
        .unwrap();
        assert_eq!(consumed, vec![0]);
        assert_eq!(processed.load(std::sync::atomic::Ordering::Relaxed), 1);
    }

    #[test]
    fn ordered_workers_propagate_errors_in_input_order_and_join() {
        for threads in [1, 3] {
            let mut consumed = Vec::new();
            let error = process_ordered(
                0..20,
                threads,
                6,
                &|index| {
                    if index == 2 {
                        Err(io::Error::new(io::ErrorKind::InvalidData, "bad input"))
                    } else {
                        Ok(index)
                    }
                },
                |index| {
                    consumed.push(index);
                    Ok(std::ops::ControlFlow::Continue(()))
                },
            )
            .unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidData);
            assert_eq!(consumed, vec![0, 1]);
            let consumer_error = process_ordered(0..20, threads, 4, &Ok, |_| {
                Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed output"))
            })
            .unwrap_err();
            assert_eq!(consumer_error.kind(), io::ErrorKind::BrokenPipe);
        }
    }

    #[test]
    fn ordered_worker_panics_report_an_error_instead_of_losing_an_index() {
        for threads in [1, 2] {
            let error = process_ordered(
                0..10,
                threads,
                4,
                &|index| {
                    assert_ne!(index, 0, "task failure");
                    Ok(index)
                },
                |_| Ok(std::ops::ControlFlow::Continue(())),
            )
            .unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::Other);
            assert_eq!(error.to_string(), "content worker panicked");
        }
    }

    struct InvertedStubMatcher;
    impl Matcher for InvertedStubMatcher {
        fn find_matches<'a>(
            &self,
            _buffer: &'a [u8],
            sink: &mut dyn MatchSink<'a>,
        ) -> io::Result<usize> {
            sink.on_match(MatchRecord::new(1, 0, b"inverted line", Vec::new()))?;
            Ok(1)
        }
    }

    #[test]
    fn test_buffer_search_inverted_empty_spans_counts_as_match() {
        let matcher = InvertedStubMatcher;
        let searcher = BufferSearch {
            matcher: &matcher,
            options: SearchOptions::default(),
        };
        let result = searcher.search(b"inverted line\n").unwrap().unwrap();
        assert_eq!(result.matched_lines, 1);
        assert_eq!(result.matches, 1);
    }

    #[test]
    fn binary_string_matches_receive_unique_record_ordinals() {
        let matcher = SimdLiteralMatcher::new("hit", true);
        let searcher = BufferSearch {
            matcher: &matcher,
            options: SearchOptions {
                binary_strings_min_len: Some(4),
                ..SearchOptions::default()
            },
        };

        let result = searcher
            .search(b"hit-one\0hit-two\0hit-three\0")
            .unwrap()
            .unwrap();
        let ordinals: Vec<_> = result.records.iter().map(|r| r.line_number).collect();
        assert_eq!(ordinals, vec![1, 2, 3]);
    }

    #[test]
    fn proximity_limits_use_actual_owners_across_different_windows() {
        let bytes = b"LONG\nSAFETY first\nunsafe first\nbridge\nSAFETY second\nunsafe second\n";
        for (filters, expected) in [
            (vec!["near:1,SAFETY", "near:10,LONG"], vec![1, 2, 3]),
            (vec!["near:1,SAFETY", "no-near:20,ABSENT"], vec![2, 3]),
        ] {
            let query =
                crate::dsl::DslParser::parse(std::iter::once("unsafe").chain(filters)).unwrap();
            let matcher = crate::search::BooleanEngine::try_new(query.expr.unwrap())
                .unwrap()
                .with_proximity_filters(&query.proximity_filters);
            let search = BufferSearch {
                matcher: &matcher,
                options: SearchOptions {
                    max_count: Some(1),
                    has_proximity: true,
                    ..Default::default()
                },
            };
            let result = search.search(bytes).unwrap().unwrap();
            assert_eq!(result.matched_lines, 1);
            assert_eq!(
                result
                    .records
                    .iter()
                    .map(|record| record.line_number)
                    .collect::<Vec<_>>(),
                expected
            );
            assert!(
                result
                    .records
                    .iter()
                    .flat_map(|record| &record.context_origins)
                    .all(|origin| origin.primary_line == 3)
            );
            assert_eq!(
                result
                    .records
                    .iter()
                    .map(|record| record.occurrence_count)
                    .sum::<usize>(),
                1
            );
        }
    }

    #[test]
    fn selected_primary_can_own_another_primary_as_neighbor_context() {
        let query = crate::dsl::DslParser::parse(["unsafe", "near:1,unsafe"]).unwrap();
        let matcher = crate::search::BooleanEngine::try_new(query.expr.unwrap())
            .unwrap()
            .with_proximity_filters(&query.proximity_filters);
        let search = BufferSearch {
            matcher: &matcher,
            options: SearchOptions {
                max_count: Some(1),
                has_proximity: true,
                ..Default::default()
            },
        };
        let result = search
            .search(b"unsafe one\nunsafe two\nunsafe three\n")
            .unwrap()
            .unwrap();
        assert_eq!(result.records.len(), 2);
        assert!(!result.records[0].is_context);
        assert!(result.records[1].is_context);
        assert_eq!(result.records[1].occurrence_count, 0);
        assert!(result.records[1].match_spans.is_empty());
        assert_eq!(result.records[1].near_spans, vec![(0, 6)]);
        assert_eq!(result.records[1].context_origins.len(), 1);
        assert_eq!(result.records[1].context_origins[0].primary_line, 1);
    }

    #[test]
    fn proximity_limits_keep_only_neighbors_of_selected_primary_lines() {
        let query = crate::dsl::DslParser::parse(["from_raw", "near:2,SAFETY"]).unwrap();
        let matcher = crate::search::BooleanEngine::try_new(query.expr.unwrap())
            .unwrap()
            .with_proximity_filters(&query.proximity_filters);
        let search = BufferSearch {
            matcher: &matcher,
            options: SearchOptions {
                max_count: Some(1),
                has_proximity: true,
                ..SearchOptions::default()
            },
        };
        let bytes = b"header\nSAFETY first\nfrom_raw(1)\nbridge\nblank\nSAFETY second\nbridge\nfrom_raw(2)\n";
        let result = search.search(bytes).unwrap().unwrap();
        assert_eq!(result.matched_lines, 1);
        assert_eq!(
            result
                .records
                .iter()
                .map(|record| record.line_number)
                .collect::<Vec<_>>(),
            vec![2, 3]
        );
        assert_eq!(result.records[0].occurrence_count, 0);
        assert_eq!(result.records[1].occurrence_count, 1);
    }
}
