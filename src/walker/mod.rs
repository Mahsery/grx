use crate::core::{DirEntry, DirectoryWalker, FilterDecision, IgnoreFilter, WalkerError};
use crossbeam_deque::{Injector, Steal, Worker};
use std::collections::HashSet;
use std::fs;
use std::io;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

#[inline]
fn get_file_identity(path: &Path, meta: &fs::Metadata) -> Option<(u64, u64)> {
    #[cfg(unix)]
    {
        let _ = path;
        Some((meta.dev(), meta.ino()))
    }
    #[cfg(windows)]
    {
        let _ = meta;
        if let Ok(canon) = fs::canonicalize(path) {
            use std::hash::{DefaultHasher, Hash, Hasher};
            let mut hasher1 = DefaultHasher::new();
            canon.hash(&mut hasher1);
            let h1 = hasher1.finish();
            let mut hasher2 = DefaultHasher::new();
            hasher2.write_u64(0x9e3779b97f4a7c15);
            canon.hash(&mut hasher2);
            let h2 = hasher2.finish();
            return Some((h1, h2));
        }
        None
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (path, meta);
        None
    }
}

#[derive(Default)]
struct WorkNotifier {
    mutex: Mutex<()>,
    condvar: Condvar,
}

impl WorkNotifier {
    fn notify_one(&self) {
        self.condvar.notify_one();
    }

    fn notify_all(&self) {
        self.condvar.notify_all();
    }

    fn park(&self, timeout: Duration) {
        if let Ok(guard) = self.mutex.lock() {
            let _ = self.condvar.wait_timeout(guard, timeout);
        }
    }
}

pub struct WalkTask {
    pub dir: PathBuf,

    pub depth: usize,

    pub filter: Arc<dyn IgnoreFilter>,
}

pub struct ParallelWalker {
    num_threads: usize,
    max_depth: Option<usize>,
    follow_symlinks: bool,
    regular_files_only: bool,
    emit_dirs: bool,
    emit_links: bool,
    emit_files: Option<bool>,
    suppress_errors: bool,
    pub buffer_size: usize,
}

impl ParallelWalker {
    pub fn new(num_threads: usize, max_depth: Option<usize>, follow_symlinks: bool) -> Self {
        let threads = if num_threads == 0 {
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4)
        } else {
            num_threads
        };

        Self {
            num_threads: threads,
            max_depth,
            follow_symlinks,
            regular_files_only: true,
            emit_dirs: false,
            emit_links: false,
            emit_files: None,
            suppress_errors: false,
            buffer_size: 64 * 1024,
        }
    }

    pub fn with_buffer_size(mut self, buffer_size: usize) -> Self {
        self.buffer_size = buffer_size;
        self
    }

    pub fn with_regular_files_only(mut self, regular_files_only: bool) -> Self {
        self.regular_files_only = regular_files_only;
        self
    }

    pub fn with_emit_dirs(mut self, emit_dirs: bool) -> Self {
        self.emit_dirs = emit_dirs;
        self
    }

    pub fn with_emit_links(mut self, emit_links: bool) -> Self {
        self.emit_links = emit_links;
        self
    }

    pub fn with_emit_files(mut self, emit_files: bool) -> Self {
        self.emit_files = Some(emit_files);
        self
    }

    pub fn with_suppress_errors(mut self, suppress_errors: bool) -> Self {
        self.suppress_errors = suppress_errors;
        self
    }
}

fn lexical_absolute(path: &Path, cwd: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    let mut key = PathBuf::new();
    for component in absolute.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                key.pop();
            }
            component => key.push(component.as_os_str()),
        }
    }
    key
}

fn prepare_roots<'a>(roots: &'a [PathBuf], cwd: &Path) -> (Vec<&'a PathBuf>, bool) {
    if roots.len() < 2 {
        return (roots.iter().collect(), false);
    }
    let mut seen = HashSet::new();
    let mut identities = Vec::<PathBuf>::new();
    let mut unique = Vec::new();
    let mut overlap = false;
    for root in roots {
        let identity = lexical_absolute(root, cwd);
        if !seen.insert(identity.clone()) {
            continue;
        }
        overlap |= identities
            .iter()
            .any(|previous| identity.starts_with(previous) || previous.starts_with(&identity));
        identities.push(identity);
        unique.push(root);
    }
    (unique, overlap)
}

impl DirectoryWalker for ParallelWalker {
    fn walk(
        &self,
        roots: &[PathBuf],
        filter: Arc<dyn IgnoreFilter>,
        on_entry: &(dyn Fn(DirEntry) -> Result<(), WalkerError> + Sync),
    ) -> Result<(), WalkerError> {
        let injector = Arc::new(Injector::<WalkTask>::new());
        let active_tasks = Arc::new(AtomicUsize::new(0));
        let has_error = Arc::new(AtomicBool::new(false));
        let broken_pipe = Arc::new(AtomicBool::new(false));
        let visited_dirs = Arc::new(Mutex::new(HashSet::<(u64, u64)>::new()));
        let notifier = Arc::new(WorkNotifier::default());

        let cwd = if roots.len() > 1 {
            std::env::current_dir()?
        } else {
            PathBuf::new()
        };
        let (roots, overlap) = prepare_roots(roots, &cwd);
        let emitted = Mutex::new(HashSet::<PathBuf>::new());
        let unique_entry = |entry: DirEntry| {
            let key = lexical_absolute(&entry.full_path(), &cwd);
            if emitted
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .insert(key)
            {
                on_entry(entry)
            } else {
                Ok(())
            }
        };
        let on_entry: &(dyn Fn(DirEntry) -> Result<(), WalkerError> + Sync) =
            if overlap { &unique_entry } else { on_entry };

        for root in roots {
            if let Some(name) = root.file_name() {
                let parent = root.parent().unwrap_or_else(|| Path::new("."));
                let scoped = filter
                    .for_dir(parent)
                    .unwrap_or_else(|| Arc::clone(&filter));
                if scoped.filter(
                    parent,
                    name.as_encoded_bytes(),
                    root.is_dir() && !root.is_symlink(),
                ) != FilterDecision::Include
                {
                    continue;
                }
            }

            if root.is_symlink() {
                if self.emit_links {
                    let parent = root.parent().unwrap_or_else(|| Path::new("")).to_path_buf();
                    let name = root
                        .file_name()
                        .unwrap_or_default()
                        .as_encoded_bytes()
                        .to_vec();
                    let entry = DirEntry::new(parent, name, false, true, None);
                    on_entry(entry)?;
                } else if let Ok(meta) = fs::metadata(root) {
                    if meta.is_dir() {
                        if self.follow_symlinks {
                            if let Some(id) = get_file_identity(root, &meta) {
                                visited_dirs
                                    .lock()
                                    .unwrap_or_else(|p| p.into_inner())
                                    .insert(id);
                            }
                            active_tasks.fetch_add(1, Ordering::SeqCst);
                            injector.push(WalkTask {
                                dir: root.clone(),
                                depth: 0,
                                filter: Arc::clone(&filter),
                            });
                            notifier.notify_one();
                        }
                    } else if meta.is_file() {
                        let parent = root.parent().unwrap_or_else(|| Path::new("")).to_path_buf();
                        let name = root
                            .file_name()
                            .unwrap_or_default()
                            .as_encoded_bytes()
                            .to_vec();
                        let entry = DirEntry::new(parent, name, false, true, None);
                        on_entry(entry)?;
                    }
                }
            } else if root.is_dir() {
                if let Ok(meta) = fs::metadata(root)
                    && let Some(id) = get_file_identity(root, &meta)
                {
                    visited_dirs
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .insert(id);
                }
                active_tasks.fetch_add(1, Ordering::SeqCst);
                injector.push(WalkTask {
                    dir: root.clone(),
                    depth: 0,
                    filter: Arc::clone(&filter),
                });
                notifier.notify_one();
            } else if root.is_file() {
                let emit_files = self
                    .emit_files
                    .unwrap_or(!self.emit_dirs && !self.emit_links);
                if emit_files {
                    let parent = root.parent().unwrap_or_else(|| Path::new("")).to_path_buf();
                    let name = root
                        .file_name()
                        .unwrap_or_default()
                        .as_encoded_bytes()
                        .to_vec();
                    let entry = DirEntry::new(parent, name, false, false, None);
                    on_entry(entry)?;
                }
            }
        }

        let config = WalkConfig {
            max_depth: self.max_depth,
            follow_symlinks: self.follow_symlinks,
            regular_files_only: self.regular_files_only,
            emit_dirs: self.emit_dirs,
            emit_links: self.emit_links,
            emit_files: self
                .emit_files
                .unwrap_or(!self.emit_dirs && !self.emit_links),
            suppress_errors: self.suppress_errors,
            buffer_size: self.buffer_size,
        };

        let workers: Vec<Worker<WalkTask>> =
            (0..self.num_threads).map(|_| Worker::new_fifo()).collect();
        let stealers: Arc<[crossbeam_deque::Stealer<WalkTask>]> = workers
            .iter()
            .map(|w| w.stealer())
            .collect::<Vec<_>>()
            .into();

        std::thread::scope(|scope| {
            for (worker_idx, worker) in workers.into_iter().enumerate() {
                let injector = Arc::clone(&injector);
                let active_tasks = Arc::clone(&active_tasks);
                let has_error = Arc::clone(&has_error);
                let broken_pipe = Arc::clone(&broken_pipe);
                let visited_dirs = Arc::clone(&visited_dirs);
                let notifier = Arc::clone(&notifier);
                let stealers = Arc::clone(&stealers);

                scope.spawn(move || {
                    let ctx = WalkContext {
                        config,
                        visited_dirs: &visited_dirs,
                        active_tasks: &active_tasks,
                        notifier: &notifier,
                        on_entry,
                    };

                    loop {
                        if has_error.load(Ordering::Relaxed) || broken_pipe.load(Ordering::Relaxed)
                        {
                            break;
                        }

                        let next_item = worker
                            .pop()
                            .or_else(|| match injector.steal_batch_and_pop(&worker) {
                                Steal::Success(val) => Some(val),
                                _ => None,
                            })
                            .or_else(|| {
                                stealers
                                    .iter()
                                    .enumerate()
                                    .cycle()
                                    .skip(worker_idx + 1)
                                    .take(stealers.len().saturating_sub(1))
                                    .find_map(|(_, s)| match s.steal_batch_and_pop(&worker) {
                                        Steal::Success(val) => Some(val),
                                        _ => None,
                                    })
                            });

                        match next_item {
                            Some(task) => {
                                let process_res = process_directory(
                                    &task.dir,
                                    task.depth,
                                    &task.filter,
                                    &worker,
                                    &injector,
                                    &ctx,
                                );

                                if active_tasks.fetch_sub(1, Ordering::SeqCst) == 1 {
                                    notifier.notify_all();
                                }

                                if let Err(e) = process_res {
                                    if e.is_broken_pipe() {
                                        broken_pipe.store(true, Ordering::Relaxed);
                                    } else {
                                        has_error.store(true, Ordering::Relaxed);
                                    }
                                    notifier.notify_all();
                                    break;
                                }
                            }
                            None => {
                                if active_tasks.load(Ordering::SeqCst) == 0 {
                                    notifier.notify_all();
                                    break;
                                } else {
                                    notifier.park(Duration::from_millis(2));
                                }
                            }
                        }
                    }
                });
            }
        });

        if broken_pipe.load(Ordering::Relaxed) {
            return Err(WalkerError::Io(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "Broken pipe",
            )));
        }

        if has_error.load(Ordering::Relaxed) {
            return Err(WalkerError::General(
                "Directory traversal failed in worker".to_string(),
            ));
        }

        Ok(())
    }
}

#[derive(Clone, Copy)]
struct WalkConfig {
    max_depth: Option<usize>,
    follow_symlinks: bool,
    regular_files_only: bool,
    emit_dirs: bool,
    emit_links: bool,
    emit_files: bool,
    suppress_errors: bool,
    buffer_size: usize,
}

struct WalkContext<'a> {
    config: WalkConfig,
    visited_dirs: &'a Arc<Mutex<HashSet<(u64, u64)>>>,
    active_tasks: &'a AtomicUsize,
    notifier: &'a WorkNotifier,
    on_entry: &'a (dyn Fn(DirEntry) -> Result<(), WalkerError> + Sync),
}

#[cfg(target_os = "linux")]
mod linux;
#[allow(dead_code)]
mod portable;

fn process_directory(
    dir: &Path,
    depth: usize,
    parent_filter: &Arc<dyn IgnoreFilter>,
    worker: &Worker<WalkTask>,
    injector: &Injector<WalkTask>,
    ctx: &WalkContext,
) -> Result<(), WalkerError> {
    if let Some(max) = ctx.config.max_depth
        && depth > max
    {
        return Ok(());
    }

    #[cfg(target_os = "linux")]
    {
        linux::process_directory_getdents(dir, depth, parent_filter, worker, injector, ctx)
    }

    #[cfg(not(target_os = "linux"))]
    {
        portable::process_directory_portable(dir, depth, parent_filter, worker, injector, ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::FilterDecision;
    use std::sync::Mutex;
    use tempfile::tempdir;

    struct AllowAllFilter;
    impl IgnoreFilter for AllowAllFilter {
        fn filter(&self, _parent: &Path, _name: &[u8], _is_dir: bool) -> FilterDecision {
            FilterDecision::Include
        }
    }

    #[test]
    fn test_parallel_walker_traversal() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();

        fs::create_dir_all(root.join("src/models")).unwrap();
        fs::write(root.join("src/main.rs"), b"fn main() {}").unwrap();
        fs::write(root.join("src/models/user.rs"), b"struct User;").unwrap();
        fs::write(root.join("Cargo.toml"), b"[package]").unwrap();

        let walker = ParallelWalker::new(2, None, false);
        let found_files = Arc::new(Mutex::new(Vec::new()));
        let found_clone = Arc::clone(&found_files);

        walker
            .walk(
                &[root.to_path_buf()],
                Arc::new(AllowAllFilter),
                &move |entry| {
                    found_clone
                        .lock()
                        .unwrap()
                        .push(entry.name_display().into_owned());
                    Ok(())
                },
            )
            .unwrap();

        let mut list = found_files.lock().unwrap().clone();
        list.sort();

        assert!(list.contains(&"main.rs".to_string()));
        assert!(list.contains(&"user.rs".to_string()));
        assert!(list.contains(&"Cargo.toml".to_string()));
    }

    #[test]
    fn test_parallel_walker_nested_gitignore() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();

        fs::create_dir_all(root.join("sub/ignored_dir")).unwrap();
        fs::write(root.join("allowed.txt"), b"allowed").unwrap();
        fs::write(root.join("sub/nested_allowed.txt"), b"nested").unwrap();
        fs::write(root.join("sub/.gitignore"), b"ignored_dir/\n").unwrap();
        fs::write(root.join("sub/ignored_dir/secret.txt"), b"secret").unwrap();

        let engine =
            crate::ignore::GitignoreEngine::new(Vec::new(), Vec::new(), Vec::new(), true, true);

        let walker = ParallelWalker::new(2, None, false);
        let found_files = Arc::new(Mutex::new(Vec::new()));
        let found_clone = Arc::clone(&found_files);

        walker
            .walk(&[root.to_path_buf()], Arc::new(engine), &move |entry| {
                found_clone
                    .lock()
                    .unwrap()
                    .push(entry.name_display().into_owned());
                Ok(())
            })
            .unwrap();

        let list = found_files.lock().unwrap().clone();
        assert!(list.contains(&"allowed.txt".to_string()));
        assert!(list.contains(&"nested_allowed.txt".to_string()));
        assert!(
            !list.contains(&"secret.txt".to_string()),
            "secret.txt should be pruned by nested .gitignore"
        );
    }

    #[test]
    #[cfg(unix)]
    fn test_parallel_walker_symlink_cycle() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();

        let sub = root.join("sub");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join("file.txt"), b"content").unwrap();

        std::os::unix::fs::symlink(&sub, sub.join("loop")).unwrap();

        let walker = ParallelWalker::new(2, None, true);
        let found = Arc::new(Mutex::new(Vec::new()));
        let found_clone = Arc::clone(&found);

        walker
            .walk(
                &[root.to_path_buf()],
                Arc::new(AllowAllFilter),
                &move |entry| {
                    found_clone
                        .lock()
                        .unwrap()
                        .push(entry.name_display().into_owned());
                    Ok(())
                },
            )
            .unwrap();

        let list = found.lock().unwrap().clone();
        assert!(list.contains(&"file.txt".to_string()));
    }

    #[test]
    #[cfg(unix)]
    fn test_parallel_walker_does_not_follow_directory_symlink_root_by_default() {
        let tmp = tempdir().unwrap();
        let target = tmp.path().join("target");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("hidden.txt"), b"content").unwrap();
        let root_link = tmp.path().join("root-link");
        std::os::unix::fs::symlink(&target, &root_link).unwrap();

        let found = Arc::new(Mutex::new(Vec::new()));
        let found_clone = Arc::clone(&found);
        ParallelWalker::new(2, None, false)
            .walk(&[root_link], Arc::new(AllowAllFilter), &move |entry| {
                found_clone
                    .lock()
                    .unwrap()
                    .push(entry.name_display().into_owned());
                Ok(())
            })
            .unwrap();

        assert!(found.lock().unwrap().is_empty());
    }

    #[test]
    #[cfg(unix)]
    fn test_parallel_walker_skips_device_and_broken_symlinks() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();

        fs::write(root.join("regular.txt"), b"regular content").unwrap();

        std::os::unix::fs::symlink(
            root.join("regular.txt"),
            root.join("symlink_to_regular.txt"),
        )
        .unwrap();

        std::os::unix::fs::symlink(root.join("nonexistent.txt"), root.join("broken_link.txt"))
            .unwrap();

        if Path::new("/dev/null").exists() {
            let _ = std::os::unix::fs::symlink("/dev/null", root.join("device_link"));
        }

        let walker = ParallelWalker::new(2, None, false);
        let found = Arc::new(Mutex::new(Vec::new()));
        let found_clone = Arc::clone(&found);

        walker
            .walk(
                &[root.to_path_buf()],
                Arc::new(AllowAllFilter),
                &move |entry| {
                    found_clone
                        .lock()
                        .unwrap()
                        .push(entry.name_display().into_owned());
                    Ok(())
                },
            )
            .unwrap();

        let list = found.lock().unwrap().clone();
        assert!(list.contains(&"regular.txt".to_string()));
        assert!(list.contains(&"symlink_to_regular.txt".to_string()));
        assert!(!list.contains(&"broken_link.txt".to_string()));
        assert!(!list.contains(&"device_link".to_string()));
    }

    #[test]
    fn test_parallel_walker_emits_directories() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();

        fs::create_dir_all(root.join("sub1/sub2")).unwrap();
        fs::write(root.join("root_file.txt"), b"root").unwrap();
        fs::write(root.join("sub1/sub1_file.txt"), b"sub1").unwrap();
        fs::write(root.join("sub1/sub2/sub2_file.txt"), b"sub2").unwrap();

        let walker = ParallelWalker::new(2, None, false).with_emit_dirs(true);
        let found = Arc::new(Mutex::new(Vec::new()));
        let found_clone = Arc::clone(&found);

        walker
            .walk(
                &[root.to_path_buf()],
                Arc::new(AllowAllFilter),
                &move |entry| {
                    assert!(entry.is_dir, "entry must be a directory");
                    assert!(!entry.is_file(), "directory is not a file");
                    found_clone
                        .lock()
                        .unwrap()
                        .push(entry.name_display().into_owned());
                    Ok(())
                },
            )
            .unwrap();

        let list = found.lock().unwrap().clone();
        assert!(list.contains(&"sub1".to_string()));
        assert!(list.contains(&"sub2".to_string()));
        assert!(!list.contains(&"root_file.txt".to_string()));
        assert!(!list.contains(&"sub1_file.txt".to_string()));
        assert!(!list.contains(&root.file_name().unwrap().to_string_lossy().to_string()));

        let walker_d1 = ParallelWalker::new(2, Some(1), false).with_emit_dirs(true);
        let found_d1 = Arc::new(Mutex::new(Vec::new()));
        let found_clone_d1 = Arc::clone(&found_d1);

        walker_d1
            .walk(
                &[root.to_path_buf()],
                Arc::new(AllowAllFilter),
                &move |entry| {
                    found_clone_d1
                        .lock()
                        .unwrap()
                        .push(entry.name_display().into_owned());
                    Ok(())
                },
            )
            .unwrap();

        let list_d1 = found_d1.lock().unwrap().clone();
        assert!(list_d1.contains(&"sub1".to_string()));
        assert!(!list_d1.contains(&"sub2".to_string()));
    }

    #[test]
    #[cfg(unix)]
    fn test_parallel_walker_emits_links_including_broken() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();

        fs::write(root.join("target.txt"), b"target").unwrap();
        std::os::unix::fs::symlink(root.join("target.txt"), root.join("valid_link")).unwrap();
        std::os::unix::fs::symlink(root.join("missing.txt"), root.join("broken_link")).unwrap();

        let walker = ParallelWalker::new(2, None, false).with_emit_links(true);
        let found = Arc::new(Mutex::new(Vec::new()));
        let found_clone = Arc::clone(&found);

        walker
            .walk(
                &[root.to_path_buf()],
                Arc::new(AllowAllFilter),
                &move |entry| {
                    assert!(entry.is_symlink, "entry must be a symlink");
                    found_clone
                        .lock()
                        .unwrap()
                        .push(entry.name_display().into_owned());
                    Ok(())
                },
            )
            .unwrap();

        let list = found.lock().unwrap().clone();
        assert!(list.contains(&"valid_link".to_string()));
        assert!(list.contains(&"broken_link".to_string()));
        assert!(!list.contains(&"target.txt".to_string()));
    }

    #[test]
    #[cfg(unix)]
    fn test_parallel_walker_emits_file_symlink_at_max_depth_zero() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();

        fs::write(root.join("yes.txt"), b"content").unwrap();
        std::os::unix::fs::symlink(root.join("yes.txt"), root.join("link.txt")).unwrap();

        let walker = ParallelWalker::new(2, Some(0), false).with_emit_files(true);
        let found = Arc::new(Mutex::new(Vec::new()));
        let found_clone = Arc::clone(&found);

        walker
            .walk(
                &[root.to_path_buf()],
                Arc::new(AllowAllFilter),
                &move |entry| {
                    found_clone
                        .lock()
                        .unwrap()
                        .push(entry.name_display().into_owned());
                    Ok(())
                },
            )
            .unwrap();

        let list = found.lock().unwrap().clone();
        assert!(list.contains(&"yes.txt".to_string()));
        assert!(list.contains(&"link.txt".to_string()));
    }

    #[test]
    fn test_parallel_walker_cross_worker_stealing() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();

        for i in 0..16 {
            let branch = root.join(format!("branch_{i}"));
            fs::create_dir_all(&branch).unwrap();
            for j in 0..5 {
                fs::write(branch.join(format!("leaf_{j}.txt")), b"test").unwrap();
            }
        }

        let walker = ParallelWalker::new(4, None, false).with_emit_files(true);
        let count = Arc::new(AtomicUsize::new(0));
        let count_clone = Arc::clone(&count);

        walker
            .walk(
                &[root.to_path_buf()],
                Arc::new(AllowAllFilter),
                &move |_entry| {
                    count_clone.fetch_add(1, Ordering::Relaxed);
                    Ok(())
                },
            )
            .unwrap();

        assert_eq!(count.load(Ordering::SeqCst), 16 * 5);
    }

    #[test]
    #[cfg(unix)]
    fn test_parallel_walker_reports_unreadable_directory() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempdir().unwrap();
        let root = tmp.path();

        let readable_dir = root.join("readable");
        let unreadable_dir = root.join("unreadable");
        fs::create_dir_all(&readable_dir).unwrap();
        fs::create_dir_all(&unreadable_dir).unwrap();

        fs::write(readable_dir.join("found.txt"), b"accessible").unwrap();
        fs::write(unreadable_dir.join("secret.txt"), b"hidden").unwrap();

        fs::set_permissions(&unreadable_dir, fs::Permissions::from_mode(0o000)).unwrap();

        let walker = ParallelWalker::new(2, None, false).with_emit_files(true);
        let found = Arc::new(Mutex::new(Vec::new()));
        let found_clone = Arc::clone(&found);

        let walk_res = walker.walk(
            &[root.to_path_buf()],
            Arc::new(AllowAllFilter),
            &move |entry| {
                found_clone
                    .lock()
                    .unwrap()
                    .push(entry.name_display().into_owned());
                Ok(())
            },
        );

        let access_denied = fs::read_dir(&unreadable_dir).is_err();
        fs::set_permissions(&unreadable_dir, fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(walk_res.is_err(), access_denied);
        let list = found.lock().unwrap().clone();
        assert_eq!(list.contains(&"secret.txt".to_string()), !access_denied);
    }

    #[test]
    #[cfg(unix)]
    fn test_parallel_walker_direct_symlink_file_argument() {
        let tmp = tempdir().unwrap();
        let target_file = tmp.path().join("real_target.txt");
        let symlink_path = tmp.path().join("link_to_target.txt");
        fs::write(&target_file, b"content").unwrap();
        std::os::unix::fs::symlink(&target_file, &symlink_path).unwrap();

        let walker = ParallelWalker::new(2, None, false).with_emit_files(true);
        let found = Arc::new(Mutex::new(Vec::new()));
        let found_clone = Arc::clone(&found);

        let walk_res = walker.walk(&[symlink_path], Arc::new(AllowAllFilter), &move |entry| {
            found_clone
                .lock()
                .unwrap()
                .push(entry.name_display().into_owned());
            Ok(())
        });

        assert!(walk_res.is_ok());
        let list = found.lock().unwrap().clone();
        assert_eq!(list, vec!["link_to_target.txt".to_string()]);
    }

    #[test]
    fn test_parallel_walker_custom_buffer_size() {
        let tmp = tempdir().unwrap();
        let file_path = tmp.path().join("sample.txt");
        fs::write(&file_path, b"hello world").unwrap();

        let walker = ParallelWalker::new(2, None, false)
            .with_buffer_size(32 * 1024)
            .with_emit_files(true);
        assert_eq!(walker.buffer_size, 32 * 1024);

        let found = Arc::new(Mutex::new(Vec::new()));
        let found_clone = Arc::clone(&found);

        let walk_res = walker.walk(
            &[tmp.path().to_path_buf()],
            Arc::new(AllowAllFilter),
            &move |entry| {
                found_clone
                    .lock()
                    .unwrap()
                    .push(entry.name_display().into_owned());
                Ok(())
            },
        );

        assert!(walk_res.is_ok());
        let list = found.lock().unwrap().clone();
        assert!(list.contains(&"sample.txt".to_string()));
    }

    #[test]
    fn explicit_roots_obey_visibility_and_overlaps_emit_once() {
        let tmp = tempdir().unwrap();
        let visible = tmp.path().join("visible");
        let hidden = tmp.path().join(".hidden");
        fs::create_dir_all(&visible).unwrap();
        fs::create_dir_all(&hidden).unwrap();
        fs::write(visible.join("one.txt"), b"one").unwrap();
        fs::write(hidden.join("two.txt"), b"two").unwrap();
        fs::write(visible.join("skip.txt"), b"skip").unwrap();
        let filter = |search_hidden| {
            Arc::new(crate::ignore::GitignoreEngine::new(
                vec!["skip.txt".into()],
                Vec::new(),
                Vec::new(),
                false,
                search_hidden,
            )) as Arc<dyn IgnoreFilter>
        };
        let found = Mutex::new(Vec::new());
        let collect = |entry: DirEntry| {
            found
                .lock()
                .unwrap()
                .push(entry.name_display().into_owned());
            Ok(())
        };
        let walker = ParallelWalker::new(2, None, false);
        walker
            .walk(
                &[
                    visible.clone(),
                    visible.join("one.txt"),
                    hidden.clone(),
                    visible.join("skip.txt"),
                ],
                filter(false),
                &collect,
            )
            .unwrap();
        assert_eq!(*found.lock().unwrap(), vec!["one.txt"]);
        found.lock().unwrap().clear();
        walker.walk(&[hidden], filter(true), &collect).unwrap();
        assert_eq!(*found.lock().unwrap(), vec!["two.txt"]);
    }

    #[test]
    #[cfg(unix)]
    fn disjoint_roots_preserve_hard_link_names_and_mixed_emission_is_unique() {
        let tmp = tempdir().unwrap();
        let left = tmp.path().join("left");
        let right = tmp.path().join("right");
        fs::create_dir_all(&left).unwrap();
        fs::create_dir_all(&right).unwrap();
        fs::write(left.join("original"), b"shared").unwrap();
        fs::hard_link(left.join("original"), right.join("another-name")).unwrap();
        std::os::unix::fs::symlink("original", left.join("file-link")).unwrap();
        std::os::unix::fs::symlink("absent", left.join("broken-link")).unwrap();
        let found = Mutex::new(Vec::new());
        let collect = |entry: DirEntry| {
            found
                .lock()
                .unwrap()
                .push((entry.name_display().into_owned(), entry.is_symlink));
            Ok(())
        };
        let walker = ParallelWalker::new(2, None, false)
            .with_emit_files(true)
            .with_emit_links(true);
        walker
            .walk(
                std::slice::from_ref(&left),
                Arc::new(AllowAllFilter),
                &collect,
            )
            .unwrap();
        let mut single = std::mem::take(&mut *found.lock().unwrap());
        single.sort();
        assert_eq!(
            single,
            vec![
                ("broken-link".into(), true),
                ("file-link".into(), true),
                ("original".into(), false),
            ]
        );
        walker
            .walk(&[left, right], Arc::new(AllowAllFilter), &collect)
            .unwrap();
        let mut entries = found.into_inner().unwrap();
        entries.sort();
        assert_eq!(
            entries,
            vec![
                ("another-name".into(), false),
                ("broken-link".into(), true),
                ("file-link".into(), true),
                ("original".into(), false),
            ]
        );
    }

    #[test]
    fn overlapping_root_aliases_keep_shallowest_depth_and_emit_files_once() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("root");
        fs::create_dir_all(root.join("sub/deep")).unwrap();
        fs::write(root.join("one.txt"), b"one").unwrap();
        fs::write(root.join("sub/two.txt"), b"two").unwrap();
        fs::write(root.join("sub/deep/three.txt"), b"three").unwrap();
        let found = Mutex::new(Vec::new());
        let collect = |entry: DirEntry| {
            found
                .lock()
                .unwrap()
                .push(entry.name_display().into_owned());
            Ok(())
        };
        ParallelWalker::new(2, Some(0), false)
            .with_emit_files(true)
            .walk(
                &[
                    root.clone(),
                    root.join("sub"),
                    root.join("sub/../one.txt"),
                    root.join("."),
                ],
                Arc::new(AllowAllFilter),
                &collect,
            )
            .unwrap();
        let mut entries = found.into_inner().unwrap();
        entries.sort();
        assert_eq!(entries, vec!["one.txt", "two.txt"]);
    }
}
