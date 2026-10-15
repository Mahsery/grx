use crate::core::{DirEntry, FilterDecision, IgnoreFilter, WalkerError};
use crossbeam_deque::{Injector, Worker};
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use super::{WalkConfig, WalkContext, WalkTask, get_file_identity};

#[allow(dead_code)]
pub fn process_directory_portable(
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
        suppress_errors,
        buffer_size,
    } = ctx.config;
    let _ = buffer_size;
    let visited_dirs = ctx.visited_dirs;
    let active_tasks = ctx.active_tasks;
    let notifier = ctx.notifier;
    let on_entry = ctx.on_entry;

    {
        let read_dir = match fs::read_dir(dir) {
            Ok(rd) => rd,
            Err(_) if suppress_errors => return Ok(()),
            Err(err) => return Err(err.into()),
        };
        let dir_filter = parent_filter
            .for_dir(dir)
            .unwrap_or_else(|| Arc::clone(parent_filter));
        for fs_entry in read_dir {
            let fs_entry = match fs_entry {
                Ok(entry) => entry,
                Err(_) if suppress_errors => continue,
                Err(err) => return Err(err.into()),
            };
            let file_type = match fs_entry.file_type() {
                Ok(ft) => ft,
                Err(_) => continue,
            };
            let is_dir = file_type.is_dir();
            let is_symlink = file_type.is_symlink();
            let is_file = file_type.is_file();
            if (regular_files_only || emit_dirs || emit_links) && !is_dir && !is_symlink && !is_file
            {
                continue;
            }
            let name_bytes = fs_entry.file_name().as_encoded_bytes().to_vec();

            let decision = dir_filter.filter(dir, &name_bytes, is_dir);
            match decision {
                FilterDecision::SkipDir | FilterDecision::Exclude => continue,
                FilterDecision::Include => {
                    if is_dir {
                        if emit_dirs && max_depth.is_none_or(|m| depth < m) {
                            let entry = DirEntry::new(
                                dir.to_path_buf(),
                                name_bytes.clone(),
                                true,
                                false,
                                None,
                            );
                            on_entry(entry)?;
                        }
                        if max_depth.is_none_or(|m| depth < m) {
                            let p = fs_entry.path();
                            if follow_symlinks
                                && let Ok(meta) = fs::metadata(&p)
                                && let Some(id) = get_file_identity(&p, &meta)
                                && !visited_dirs
                                    .lock()
                                    .unwrap_or_else(|p| p.into_inner())
                                    .insert(id)
                            {
                                continue;
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
                        if emit_links && max_depth.is_none_or(|m| depth < m) {
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
                        if let Ok(target_meta) = fs::metadata(&p) {
                            if target_meta.is_dir() {
                                if follow_symlinks {
                                    if let Some(id) = get_file_identity(&p, &target_meta)
                                        && !visited_dirs
                                            .lock()
                                            .unwrap_or_else(|p| p.into_inner())
                                            .insert(id)
                                    {
                                        continue;
                                    }
                                    if max_depth.is_none_or(|m| depth < m) {
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
                                let entry =
                                    DirEntry::new(dir.to_path_buf(), name_bytes, false, true, None);
                                on_entry(entry)?;
                            }
                        }
                    } else if emit_files {
                        let entry =
                            DirEntry::new(dir.to_path_buf(), name_bytes, false, false, None);
                        on_entry(entry)?;
                    }
                }
            }
        }
    }

    Ok(())
}
