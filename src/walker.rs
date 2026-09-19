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

/// Extract a durable 128-bit file/directory identity for cycle and duplicate detection.
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

/// Synchronization primitive for parking and waking idle worker threads without sched_yield spinning.
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

/// Work item in the parallel traversal queue.
pub struct WalkTask {
    /// Directory path to enumerate.
    pub dir: PathBuf,
    /// Depth relative to the search root.
    pub depth: usize,
    /// Active scoped ignore filter for this directory.
    pub filter: Arc<dyn IgnoreFilter>,
}

/// High-performance parallel directory walker using work-stealing and low-level Linux getdents64.
pub struct ParallelWalker {
    num_threads: usize,
    max_depth: Option<usize>,
    follow_symlinks: bool,
    regular_files_only: bool,
    emit_dirs: bool,
    emit_links: bool,
    emit_files: Option<bool>,
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

        // Push initial root paths
        for root in roots {
            // Check the link itself before helpers such as is_dir(), which
            // follow links and would otherwise admit a directory-link root
            // even when follow_symlinks is disabled.
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
                // Direct file argument
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

        let max_depth = self.max_depth;
        let follow_symlinks = self.follow_symlinks;
        let regular_files_only = self.regular_files_only;
        let emit_dirs = self.emit_dirs;
        let emit_links = self.emit_links;
        let emit_files = self.emit_files.unwrap_or(!emit_dirs && !emit_links);
        let buffer_size = self.buffer_size;

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
                                    max_depth,
                                    follow_symlinks,
                                    regular_files_only,
                                    emit_dirs,
                                    emit_links,
                                    emit_files,
                                    &visited_dirs,
                                    &task.filter,
                                    &worker,
                                    &injector,
                                    &active_tasks,
                                    &notifier,
                                    buffer_size,
                                    on_entry,
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
                                    // All work completed
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

#[allow(clippy::too_many_arguments)]
fn process_directory(
    dir: &Path,
    depth: usize,
    max_depth: Option<usize>,
    follow_symlinks: bool,
    regular_files_only: bool,
    emit_dirs: bool,
    emit_links: bool,
    emit_files: bool,
    visited_dirs: &Arc<Mutex<HashSet<(u64, u64)>>>,
    parent_filter: &Arc<dyn IgnoreFilter>,
    worker: &Worker<WalkTask>,
    _injector: &Injector<WalkTask>,
    active_tasks: &AtomicUsize,
    notifier: &WorkNotifier,
    buffer_size: usize,
    on_entry: &(dyn Fn(DirEntry) -> Result<(), WalkerError> + Sync),
) -> Result<(), WalkerError> {
    if let Some(max) = max_depth
        && depth > max
    {
        return Ok(());
    }

    // Fast streaming Linux getdents64 traversal with reusable thread-local buffers
    #[cfg(target_os = "linux")]
    {
        process_directory_getdents(
            dir,
            depth,
            max_depth,
            follow_symlinks,
            regular_files_only,
            emit_dirs,
            emit_links,
            emit_files,
            visited_dirs,
            parent_filter,
            worker,
            _injector,
            active_tasks,
            notifier,
            buffer_size,
            on_entry,
        )
    }

    // Portable fallback for non-Linux targets
    #[cfg(not(target_os = "linux"))]
    {
        let _ = regular_files_only;
        let _ = buffer_size;
        if let Ok(read_dir) = fs::read_dir(dir) {
            let dir_filter = parent_filter
                .for_dir(dir)
                .unwrap_or_else(|| Arc::clone(parent_filter));
            for entry_res in read_dir {
                if let Ok(fs_entry) = entry_res {
                    let file_type = match fs_entry.file_type() {
                        Ok(ft) => ft,
                        Err(_) => continue,
                    };
                    let is_dir = file_type.is_dir();
                    let is_symlink = file_type.is_symlink();
                    let is_file = file_type.is_file();
                    if !is_dir && !is_symlink && !is_file {
                        continue;
                    }
                    let name_bytes = fs_entry.file_name().as_encoded_bytes().to_vec();

                    let decision = dir_filter.filter(dir, &name_bytes, is_dir);
                    match decision {
                        FilterDecision::SkipDir => continue,
                        FilterDecision::Exclude => continue,
                        FilterDecision::Include => {
                            if is_dir {
                                if emit_dirs {
                                    if max_depth.map_or(true, |m| depth + 1 <= m) {
                                        let entry = DirEntry::new(
                                            dir.to_path_buf(),
                                            name_bytes.clone(),
                                            true,
                                            false,
                                            None,
                                        );
                                        on_entry(entry)?;
                                    }
                                }
                                if max_depth.map_or(true, |m| depth + 1 <= m) {
                                    let p = fs_entry.path();
                                    if follow_symlinks {
                                        if let Ok(meta) = fs::metadata(&p) {
                                            if let Some(id) = get_file_identity(&p, &meta) {
                                                if !visited_dirs
                                                    .lock()
                                                    .unwrap_or_else(|p| p.into_inner())
                                                    .insert(id)
                                                {
                                                    continue;
                                                }
                                            }
                                        }
                                    }
                                    active_tasks.fetch_add(1, Ordering::SeqCst);
                                    worker.push(WalkTask {
                                        dir: p.clone(),
                                        depth: depth + 1,
                                        filter: Arc::clone(&dir_filter),
                                    });
                                    notifier.notify_one();
                                }
                            } else if is_symlink {
                                let p = fs_entry.path();
                                let mut emitted = false;
                                if emit_links {
                                    if max_depth.map_or(true, |m| depth + 1 <= m) {
                                        let entry = DirEntry::new(
                                            dir.to_path_buf(),
                                            name_bytes.clone(),
                                            false,
                                            true,
                                            None,
                                        );
                                        on_entry(entry)?;
                                        emitted = true;
                                    }
                                }
                                if let Ok(target_meta) = fs::metadata(&p) {
                                    if target_meta.is_dir() {
                                        if follow_symlinks {
                                            if let Some(id) = get_file_identity(&p, &target_meta) {
                                                if !visited_dirs
                                                    .lock()
                                                    .unwrap_or_else(|p| p.into_inner())
                                                    .insert(id)
                                                {
                                                    continue;
                                                }
                                            }
                                            if max_depth.map_or(true, |m| depth + 1 <= m) {
                                                active_tasks.fetch_add(1, Ordering::SeqCst);
                                                worker.push(WalkTask {
                                                    dir: p,
                                                    depth: depth + 1,
                                                    filter: Arc::clone(&dir_filter),
                                                });
                                                notifier.notify_one();
                                            }
                                        }
                                    } else if target_meta.is_file() && emit_files && !emitted {
                                        let entry = DirEntry::new(
                                            dir.to_path_buf(),
                                            name_bytes,
                                            false,
                                            true,
                                            None,
                                        );
                                        on_entry(entry)?;
                                    }
                                }
                            } else {
                                if emit_files {
                                    let entry = DirEntry::new(
                                        dir.to_path_buf(),
                                        name_bytes,
                                        false,
                                        false,
                                        None,
                                    );
                                    on_entry(entry)?;
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

#[cfg(target_os = "linux")]
thread_local! {
    static GETDENTS_BUF: std::cell::RefCell<Vec<u8>> = std::cell::RefCell::new(vec![0u8; 64 * 1024]);
    static DIR_PATH_BUF: std::cell::RefCell<Vec<u8>> = std::cell::RefCell::new(Vec::with_capacity(1024));
}

/// Linux-specific high-performance streaming directory processor using direct SYS_getdents64 syscall.
/// Traverses directory entries directly from thread-local buffer without vector allocations.
#[cfg(target_os = "linux")]
#[allow(clippy::too_many_arguments)]
fn process_directory_getdents(
    dir: &Path,
    depth: usize,
    max_depth: Option<usize>,
    follow_symlinks: bool,
    regular_files_only: bool,
    emit_dirs: bool,
    emit_links: bool,
    emit_files: bool,
    visited_dirs: &Arc<Mutex<HashSet<(u64, u64)>>>,
    parent_filter: &Arc<dyn IgnoreFilter>,
    worker: &Worker<WalkTask>,
    _injector: &Injector<WalkTask>,
    active_tasks: &AtomicUsize,
    notifier: &WorkNotifier,
    buffer_size: usize,
    on_entry: &(dyn Fn(DirEntry) -> Result<(), WalkerError> + Sync),
) -> Result<(), WalkerError> {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    let fd = DIR_PATH_BUF.with(|cell| {
        let mut path_bytes = cell.borrow_mut();
        path_bytes.clear();
        path_bytes.extend_from_slice(dir.as_os_str().as_bytes());
        path_bytes.push(0);
        unsafe {
            libc::open(
                path_bytes.as_ptr() as *const libc::c_char,
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
            )
        }
    });

    if fd < 0 {
        // Skip unreadable directories gracefully
        return Ok(());
    }

    struct FdGuard(libc::c_int);
    impl Drop for FdGuard {
        fn drop(&mut self) {
            unsafe {
                libc::close(self.0);
            }
        }
    }
    let _guard = FdGuard(fd);

    // Efficiently detect .gitignore or .ignore up-front via direct directory fd probe
    let has_ignore_file = unsafe {
        libc::faccessat(fd, c".gitignore".as_ptr(), libc::F_OK, 0) == 0
            || libc::faccessat(fd, c".ignore".as_ptr(), libc::F_OK, 0) == 0
    };
    let active_filter = if has_ignore_file {
        parent_filter
            .for_dir(dir)
            .unwrap_or_else(|| Arc::clone(parent_filter))
    } else {
        Arc::clone(parent_filter)
    };

    GETDENTS_BUF.with(|cell| -> Result<(), WalkerError> {
        let mut buffer = cell.borrow_mut();
        if buffer_size > 0 && buffer.len() != buffer_size {
            buffer.resize(buffer_size, 0);
        }
        let buf_size = buffer.len();

        loop {
            let nread = unsafe {
                libc::syscall(
                    libc::SYS_getdents64,
                    fd,
                    buffer.as_mut_ptr() as *mut libc::c_void,
                    buf_size,
                )
            };

            if nread < 0 {
                let err = std::io::Error::last_os_error();
                if err.raw_os_error() == Some(libc::EINTR) {
                    continue;
                }
                // Gracefully skip unreadable/restricted directories (e.g. /proc/[pid]/map_files, vanished PIDs)
                // matching the fd < 0 check at open time.
                return Ok(());
            }
            if nread == 0 {
                break;
            }

            let nread = nread as usize;

            let mut offset = 0;
            while offset < nread {
                if offset + 19 > nread {
                    break;
                }
                let reclen =
                    u16::from_ne_bytes([buffer[offset + 16], buffer[offset + 17]]) as usize;
                if reclen < 19 || offset + reclen > nread {
                    break;
                }
                let d_type = buffer[offset + 18];
                let name_start = offset + 19;
                let mut name_end = name_start;
                while name_end < offset + reclen && buffer[name_end] != 0 {
                    name_end += 1;
                }
                let name = &buffer[name_start..name_end];

                if name != b"." && name != b".." {
                    // Fast filter for special files (character devices, block devices, FIFOs, sockets)
                    if (regular_files_only || emit_dirs || emit_links)
                        && (d_type == libc::DT_CHR
                            || d_type == libc::DT_BLK
                            || d_type == libc::DT_FIFO
                            || d_type == libc::DT_SOCK)
                    {
                        offset += reclen;
                        continue;
                    }

                    let (is_dir, is_symlink) = if d_type == libc::DT_UNKNOWN {
                        let child_path = dir.join(OsStr::from_bytes(name));
                        if let Ok(meta) = fs::symlink_metadata(&child_path) {
                            let ft = meta.file_type();
                            if (regular_files_only || emit_dirs || emit_links)
                                && !ft.is_dir()
                                && !ft.is_file()
                                && !ft.is_symlink()
                            {
                                offset += reclen;
                                continue;
                            }
                            (ft.is_dir(), ft.is_symlink())
                        } else {
                            offset += reclen;
                            continue;
                        }
                    } else {
                        (d_type == libc::DT_DIR, d_type == libc::DT_LNK)
                    };

                    let decision = active_filter.filter(dir, name, is_dir);
                    match decision {
                        FilterDecision::SkipDir | FilterDecision::Exclude => {}
                        FilterDecision::Include => {
                            if is_dir {
                                if emit_dirs && max_depth.is_none_or(|m| depth < m) {
                                    let entry = DirEntry::new(
                                        dir.to_path_buf(),
                                        name.to_vec(),
                                        true,
                                        false,
                                        None,
                                    );
                                    on_entry(entry)?;
                                }
                                if max_depth.is_none_or(|m| depth < m) {
                                    let child_dir = dir.join(OsStr::from_bytes(name));
                                    #[cfg(unix)]
                                    if follow_symlinks
                                        && let Ok(meta) = fs::metadata(&child_dir)
                                        && !visited_dirs
                                            .lock()
                                            .unwrap_or_else(|p| p.into_inner())
                                            .insert((meta.dev(), meta.ino()))
                                    {
                                        offset += reclen;
                                        continue;
                                    }
                                    active_tasks.fetch_add(1, Ordering::SeqCst);
                                    worker.push(WalkTask {
                                        dir: child_dir,
                                        depth: depth + 1,
                                        filter: Arc::clone(&active_filter),
                                    });
                                    notifier.notify_one();
                                }
                            } else if is_symlink {
                                let child_path = dir.join(OsStr::from_bytes(name));
                                let mut emitted = false;
                                if emit_links && max_depth.is_none_or(|m| depth < m) {
                                    let entry = DirEntry::new(
                                        dir.to_path_buf(),
                                        name.to_vec(),
                                        false,
                                        true,
                                        None,
                                    );
                                    on_entry(entry)?;
                                    emitted = true;
                                }
                                if let Ok(target_meta) = fs::metadata(&child_path) {
                                    if target_meta.is_dir() {
                                        if follow_symlinks {
                                            #[cfg(unix)]
                                            {
                                                if !visited_dirs
                                                    .lock()
                                                    .unwrap_or_else(|p| p.into_inner())
                                                    .insert((target_meta.dev(), target_meta.ino()))
                                                {
                                                    offset += reclen;
                                                    continue;
                                                }
                                            }
                                            if max_depth.is_none_or(|m| depth < m) {
                                                active_tasks.fetch_add(1, Ordering::SeqCst);
                                                worker.push(WalkTask {
                                                    dir: child_path,
                                                    depth: depth + 1,
                                                    filter: Arc::clone(&active_filter),
                                                });
                                                notifier.notify_one();
                                            }
                                        }
                                    } else if target_meta.is_file() && emit_files && !emitted {
                                        let entry = DirEntry::new(
                                            dir.to_path_buf(),
                                            name.to_vec(),
                                            false,
                                            true,
                                            None,
                                        );
                                        on_entry(entry)?;
                                    }
                                }
                            } else {
                                if emit_files {
                                    let entry = DirEntry::new(
                                        dir.to_path_buf(),
                                        name.to_vec(),
                                        false,
                                        false,
                                        None,
                                    );
                                    on_entry(entry)?;
                                }
                            }
                        }
                    }
                }

                offset += reclen;
            }
        }

        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
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

        // Structure:
        // root/allowed.txt
        // root/sub/nested_allowed.txt
        // root/sub/.gitignore (ignores "ignored_dir/")
        // root/sub/ignored_dir/secret.txt
        fs::create_dir_all(root.join("sub/ignored_dir")).unwrap();
        fs::write(root.join("allowed.txt"), b"allowed").unwrap();
        fs::write(root.join("sub/nested_allowed.txt"), b"nested").unwrap();
        fs::write(root.join("sub/.gitignore"), b"ignored_dir/\n").unwrap();
        fs::write(root.join("sub/ignored_dir/secret.txt"), b"secret").unwrap();

        let engine =
            crate::ignore::GitignoreEngine::new(Vec::new(), Vec::new(), Vec::new(), true, false);

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

        // Create symlink cycle: sub/loop -> sub
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

        // Symlink to a regular file (should be included)
        std::os::unix::fs::symlink(
            root.join("regular.txt"),
            root.join("symlink_to_regular.txt"),
        )
        .unwrap();

        // Broken symlink (should be skipped)
        std::os::unix::fs::symlink(root.join("nonexistent.txt"), root.join("broken_link.txt"))
            .unwrap();

        // Symlink to /dev/null (character device - should be skipped)
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

        // 1. Unlimited depth directory emission
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

        // 2. Depth bounded directory emission (d:1 should emit sub1 at depth 1, but not sub2 at depth 2)
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

        // Create 16 branch directories, each with files, ensuring work queues become imbalanced
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
    fn test_parallel_walker_skips_unreadable_directory() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempdir().unwrap();
        let root = tmp.path();

        let readable_dir = root.join("readable");
        let unreadable_dir = root.join("unreadable");
        fs::create_dir_all(&readable_dir).unwrap();
        fs::create_dir_all(&unreadable_dir).unwrap();

        fs::write(readable_dir.join("found.txt"), b"accessible").unwrap();
        fs::write(unreadable_dir.join("secret.txt"), b"hidden").unwrap();

        // Make unreadable_dir unreadable
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

        // Restore permissions for cleanup
        let _ = fs::set_permissions(&unreadable_dir, fs::Permissions::from_mode(0o755));

        assert!(
            walk_res.is_ok(),
            "Walker must not abort on unreadable directory"
        );
        let list = found.lock().unwrap().clone();
        assert!(list.contains(&"found.txt".to_string()));
        assert!(!list.contains(&"secret.txt".to_string()));
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
}
