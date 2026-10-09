use crate::core::{DirEntry, FilterDecision, IgnoreFilter, WalkerError};
use crossbeam_deque::{Injector, Worker};
use std::ffi::OsStr;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use super::{WalkConfig, WalkContext, WalkTask};

#[cfg(target_os = "linux")]
thread_local! {
    static GETDENTS_BUF: std::cell::RefCell<Vec<u8>> = std::cell::RefCell::new(vec![0u8; 64 * 1024]);
    static DIR_PATH_BUF: std::cell::RefCell<Vec<u8>> = std::cell::RefCell::new(Vec::with_capacity(1024));
}

#[cfg(target_os = "linux")]
pub fn process_directory_getdents(
    dir: &Path,
    depth: usize,
    parent_filter: &Arc<dyn IgnoreFilter>,
    worker: &Worker<WalkTask>,
    _injector: &Injector<WalkTask>,
    ctx: &WalkContext,
) -> Result<(), WalkerError> {
    let WalkConfig {
        max_depth,
        follow_symlinks,
        regular_files_only,
        emit_dirs,
        emit_links,
        emit_files,
        buffer_size,
    } = ctx.config;
    let visited_dirs = ctx.visited_dirs;
    let active_tasks = ctx.active_tasks;
    let notifier = ctx.notifier;
    let on_entry = ctx.on_entry;

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
