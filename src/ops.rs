// src/ops.rs — Transactional filesystem operations with WAL journal, dry-run previews, and instant undo.
//
// Every mutating operation (mv:, cp:, rm:, rename:, chmod:) creates an atomic transaction record
// in $XDG_DATA_HOME/grx/transactions/<tx_id>.json. If an operation is interrupted or fails halfway,
// completed operations are rolled back automatically. Running `grx undo` restores previous state.

use crate::config::Config;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Type of filesystem action requested by the DSL or CLI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionKind {
    /// Move matching items into destination directory.
    Move(PathBuf),
    /// Copy matching items into destination directory.
    Copy(PathBuf),
    /// Safely remove matching items by staging them into the transaction trash cache.
    Trash,
    /// Batch rename entries matching a find-and-replace pattern.
    Rename {
        pattern: String,
        replacement: String,
    },
    /// Modify file permissions (e.g. +x, 0755).
    Chmod(String),
}

/// A recorded individual file operation within an atomic transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileOp {
    /// File or directory moved from source to destination.
    Move {
        src: PathBuf,
        dst: PathBuf,
        overwritten_backup: Option<PathBuf>,
    },
    /// File or directory copied from source to destination.
    Copy {
        src: PathBuf,
        dst: PathBuf,
        is_dir: bool,
        overwritten_backup: Option<PathBuf>,
    },
    /// File or directory moved into the staging trash.
    Trash {
        original_path: PathBuf,
        trash_path: PathBuf,
        is_dir: bool,
    },
    /// Entry renamed.
    Rename {
        old_path: PathBuf,
        new_path: PathBuf,
    },
    /// Permissions altered.
    Chmod {
        path: PathBuf,
        old_mode: u32,
        new_mode: u32,
    },
}

/// An atomic transaction representing a set of related filesystem operations.
#[derive(Debug, Clone)]
pub struct Transaction {
    /// Unique transaction ID (e.g. `tx-1789150000`).
    pub id: String,
    /// Timestamp when transaction occurred.
    pub timestamp_epoch_secs: u64,
    /// Human-readable summary of the action performed.
    pub summary: String,
    /// Sequence of individual operations in execution order.
    pub ops: Vec<FileOp>,
}

/// Planned execution containing planned operations, destination paths, and detected collisions.
#[derive(Debug, Clone)]
pub struct OperationPlan {
    pub tx_id: String,
    pub action: ActionKind,
    pub ops: Vec<FileOp>,
    pub conflicts: Vec<String>,
    pub force: bool,
}

impl OperationPlan {
    /// Check if any unresolvable conflicts were detected.
    pub fn has_conflicts(&self) -> bool {
        !self.conflicts.is_empty()
    }

    /// Render a color-coded dry-run table for terminal preview.
    pub fn render_preview(&self, use_color: bool) -> String {
        let mut out = String::new();
        let (bold, reset, yellow, green, red, cyan) = if use_color {
            (
                "\x1b[1m", "\x1b[0m", "\x1b[33m", "\x1b[32m", "\x1b[31m", "\x1b[36m",
            )
        } else {
            ("", "", "", "", "", "")
        };

        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let fmt_p = |p: &Path| -> String {
            if let Ok(rel) = p.strip_prefix(&cwd) {
                rel.display().to_string()
            } else {
                p.display().to_string()
            }
        };

        out.push_str(&format!(
            "{bold}[DRY-RUN] Planned Operations ({count} items, {conflicts} conflicts):{reset}\n",
            count = self.ops.len(),
            conflicts = self.conflicts.len()
        ));

        for op in &self.ops {
            match op {
                FileOp::Move { src, dst, .. } => {
                    out.push_str(&format!(
                        "  {cyan}MOVE{reset}  {src}  {yellow}->{reset}  {dst}\n",
                        src = fmt_p(src),
                        dst = fmt_p(dst)
                    ));
                }
                FileOp::Copy { src, dst, .. } => {
                    out.push_str(&format!(
                        "  {green}COPY{reset}  {src}  {yellow}->{reset}  {dst}\n",
                        src = fmt_p(src),
                        dst = fmt_p(dst)
                    ));
                }
                FileOp::Trash { original_path, .. } => {
                    out.push_str(&format!(
                        "  {red}TRASH{reset} {path}  {yellow}->{reset}  [staging trash]\n",
                        path = fmt_p(original_path)
                    ));
                }
                FileOp::Rename { old_path, new_path } => {
                    out.push_str(&format!(
                        "  {cyan}RENAME{reset} {old}  {yellow}->{reset}  {new}\n",
                        old = fmt_p(old_path),
                        new = fmt_p(new_path)
                    ));
                }
                FileOp::Chmod {
                    path,
                    old_mode,
                    new_mode,
                } => {
                    out.push_str(&format!(
                        "  {yellow}CHMOD{reset}  {path} ({old:o} -> {new:o})\n",
                        path = fmt_p(path),
                        old = old_mode,
                        new = new_mode
                    ));
                }
            }
        }

        if !self.conflicts.is_empty() {
            out.push_str(&format!("\n{bold}{red}Conflicts Detected:{reset}\n"));
            for c in &self.conflicts {
                out.push_str(&format!("  ! {c}\n"));
            }
            out.push_str("Use --force to proceed anyway.\n");
        } else {
            out.push_str(&format!(
                "\n{green}No conflicts detected.{reset} Run without --dry-run or dry: to apply.\n"
            ));
        }

        out
    }
}

fn strip_verbatim_prefix(path: &Path) -> &Path {
    let s = path.to_str().unwrap_or("");
    if let Some(stripped) = s.strip_prefix(r"\\?\") {
        Path::new(stripped)
    } else {
        path
    }
}

fn is_same_path(a: &Path, b: &Path) -> bool {
    let a_clean = strip_verbatim_prefix(a);
    let b_clean = strip_verbatim_prefix(b);
    #[cfg(not(windows))]
    {
        a_clean == b_clean
    }
    #[cfg(windows)]
    {
        if a_clean == b_clean {
            return true;
        }
        let a_str = a_clean
            .to_string_lossy()
            .to_ascii_lowercase()
            .replace('/', "\\");
        let b_str = b_clean
            .to_string_lossy()
            .to_ascii_lowercase()
            .replace('/', "\\");
        a_str.trim_end_matches('\\') == b_str.trim_end_matches('\\')
    }
}

fn is_subpath(parent: &Path, child: &Path) -> bool {
    let p = strip_verbatim_prefix(parent);
    let c = strip_verbatim_prefix(child);
    #[cfg(not(windows))]
    {
        c.starts_with(p)
    }
    #[cfg(windows)]
    {
        if c.starts_with(p) {
            return true;
        }
        let p_str = p.to_string_lossy().to_ascii_lowercase().replace('/', "\\");
        let c_str = c.to_string_lossy().to_ascii_lowercase().replace('/', "\\");
        let p_trimmed = p_str.trim_end_matches('\\');
        if c_str == p_trimmed
            || (c_str.starts_with(p_trimmed) && c_str[p_trimmed.len()..].starts_with('\\'))
        {
            return true;
        }
        false
    }
}

/// Plan operations for a set of matching entry paths under an action.
pub fn plan_action(
    entries: &[PathBuf],
    action: &ActionKind,
    force: bool,
) -> Result<OperationPlan, String> {
    let mut ops = Vec::new();
    let mut conflicts = Vec::new();

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let millis = now.as_millis();
    let pid = std::process::id();
    let tx_id = format!("tx-{millis:016}-{pid:06}");
    let trash_dir = Config::data_dir().join("trash").join(&tx_id);

    for (idx, entry) in entries.iter().enumerate() {
        match action {
            ActionKind::Move(dest_dir) => {
                let file_name = entry
                    .file_name()
                    .ok_or_else(|| format!("Cannot move root path '{}'", entry.display()))?;
                let dst = dest_dir.join(file_name);

                let dest_abs = if let Ok(can_d) = dest_dir.canonicalize() {
                    can_d
                } else if dest_dir.is_absolute() {
                    dest_dir.clone()
                } else {
                    cwd.join(dest_dir)
                };
                let abs_src = if let Ok(can) = entry.canonicalize() {
                    can
                } else if entry.is_absolute() {
                    entry.clone()
                } else {
                    cwd.join(entry)
                };

                // Skip moving the destination directory into itself, or error if moving into subdirectory of itself
                if is_same_path(&abs_src, &dest_abs) {
                    continue;
                }
                if is_subpath(&abs_src, &dest_abs) {
                    conflicts.push(format!(
                        "Cannot move '{}' into a subdirectory of itself ('{}')",
                        entry.display(),
                        dest_dir.display()
                    ));
                    continue;
                }

                if dst.exists() && !force {
                    conflicts.push(format!(
                        "Destination '{}' already exists (use --force to overwrite)",
                        dst.display()
                    ));
                }

                let abs_dst = if dst.is_absolute() {
                    dst
                } else {
                    cwd.join(&dst)
                };

                ops.push(FileOp::Move {
                    src: abs_src,
                    dst: abs_dst,
                    overwritten_backup: None,
                });
            }
            ActionKind::Copy(dest_dir) => {
                let file_name = entry
                    .file_name()
                    .ok_or_else(|| format!("Cannot copy root path '{}'", entry.display()))?;
                let dst = dest_dir.join(file_name);

                let dest_abs = if let Ok(can_d) = dest_dir.canonicalize() {
                    can_d
                } else if dest_dir.is_absolute() {
                    dest_dir.clone()
                } else {
                    cwd.join(dest_dir)
                };
                let abs_src = if let Ok(can) = entry.canonicalize() {
                    can
                } else if entry.is_absolute() {
                    entry.clone()
                } else {
                    cwd.join(entry)
                };

                // Skip copying the destination directory into itself, or error if copying into subdirectory of itself
                if is_same_path(&abs_src, &dest_abs) {
                    continue;
                }
                if is_subpath(&abs_src, &dest_abs) {
                    conflicts.push(format!(
                        "Cannot copy '{}' into a subdirectory of itself ('{}')",
                        entry.display(),
                        dest_dir.display()
                    ));
                    continue;
                }

                if dst.exists() && !force {
                    conflicts.push(format!(
                        "Destination '{}' already exists (use --force to overwrite)",
                        dst.display()
                    ));
                }

                let abs_dst = if dst.is_absolute() {
                    dst
                } else {
                    cwd.join(&dst)
                };

                ops.push(FileOp::Copy {
                    src: abs_src,
                    dst: abs_dst,
                    is_dir: entry.is_dir(),
                    overwritten_backup: None,
                });
            }
            ActionKind::Trash => {
                let file_name = entry
                    .file_name()
                    .unwrap_or_else(|| std::ffi::OsStr::new("entry"));
                let trash_path =
                    trash_dir.join(format!("{idx:04}_{}", file_name.to_string_lossy()));
                let abs_entry = if let Ok(can) = entry.canonicalize() {
                    can
                } else if entry.is_absolute() {
                    entry.clone()
                } else {
                    cwd.join(entry)
                };
                ops.push(FileOp::Trash {
                    original_path: abs_entry,
                    trash_path,
                    is_dir: entry.is_dir(),
                });
            }
            ActionKind::Rename {
                pattern,
                replacement,
            } => {
                let name = entry
                    .file_name()
                    .map(|f| f.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let new_name = name.replace(pattern, replacement);
                if new_name != name {
                    let parent = entry.parent().unwrap_or_else(|| Path::new(""));
                    let new_path = parent.join(&new_name);
                    if new_path.exists() && !force {
                        conflicts.push(format!(
                            "Rename destination '{}' already exists",
                            new_path.display()
                        ));
                    }
                    let abs_old = if let Ok(can) = entry.canonicalize() {
                        can
                    } else if entry.is_absolute() {
                        entry.clone()
                    } else {
                        cwd.join(entry)
                    };
                    let abs_new = if new_path.is_absolute() {
                        new_path
                    } else {
                        cwd.join(&new_path)
                    };
                    ops.push(FileOp::Rename {
                        old_path: abs_old,
                        new_path: abs_new,
                    });
                }
            }
            ActionKind::Chmod(mode_str) => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let meta = fs::metadata(entry)
                        .map_err(|e| format!("Cannot stat '{}': {e}", entry.display()))?;
                    let old_mode = meta.permissions().mode() & 0o7777;
                    let new_mode = parse_chmod_mode(old_mode, mode_str)?;
                    let abs_entry = if let Ok(can) = entry.canonicalize() {
                        can
                    } else if entry.is_absolute() {
                        entry.clone()
                    } else {
                        cwd.join(entry)
                    };
                    ops.push(FileOp::Chmod {
                        path: abs_entry,
                        old_mode,
                        new_mode,
                    });
                }
                #[cfg(not(unix))]
                {
                    let _ = mode_str;
                    return Err("chmod action is only supported on Unix targets".into());
                }
            }
        }
    }

    Ok(OperationPlan {
        tx_id,
        action: action.clone(),
        ops,
        conflicts,
        force,
    })
}

/// Execute a planned set of operations with atomic rollback on error.
pub fn execute_plan(plan: &OperationPlan) -> Result<Transaction, String> {
    if plan.has_conflicts() && !plan.force {
        return Err(format!(
            "Cannot execute action: {} conflict(s) detected. Rerun with --force or fix conflicts.",
            plan.conflicts.len()
        ));
    }

    let tx_id = plan.tx_id.clone();
    let mut executed_ops: Vec<FileOp> = Vec::new();

    let backup_dir = Config::data_dir().join("backups").join(&tx_id);

    let summary = match &plan.action {
        ActionKind::Move(dest) => format!("Moved {} items into {}", plan.ops.len(), dest.display()),
        ActionKind::Copy(dest) => {
            format!("Copied {} items into {}", plan.ops.len(), dest.display())
        }
        ActionKind::Trash => format!("Trashed {} items", plan.ops.len()),
        ActionKind::Rename {
            pattern,
            replacement,
        } => {
            format!(
                "Renamed {} items ('{pattern}' -> '{replacement}')",
                plan.ops.len()
            )
        }
        ActionKind::Chmod(mode) => format!("Chmod {} items to {}", plan.ops.len(), mode),
    };

    // Persist pre-execution WAL intent before applying filesystem mutations
    let pre_tx = Transaction {
        id: tx_id.clone(),
        timestamp_epoch_secs: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        summary: format!("[in-progress] {summary}"),
        ops: plan.ops.clone(),
    };
    save_transaction(&pre_tx)?;

    let abort_and_rollback = |executed: &[FileOp]| {
        rollback_executed(executed);
        let path = Config::data_dir()
            .join("transactions")
            .join(format!("{tx_id}.json"));
        let _ = fs::remove_file(path);
    };

    for (idx, op) in plan.ops.iter().enumerate() {
        match op {
            FileOp::Move { src, dst, .. } => {
                if let Some(parent) = dst.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                let backup = if dst.exists() {
                    let _ = fs::create_dir_all(&backup_dir);
                    let b = backup_dir.join(format!(
                        "{idx:04}_{}",
                        dst.file_name().unwrap_or_default().to_string_lossy()
                    ));
                    if let Err(e) = move_path(dst, &b) {
                        abort_and_rollback(&executed_ops);
                        return Err(format!(
                            "Failed to backup existing '{}': {e}",
                            dst.display()
                        ));
                    }
                    Some(b)
                } else {
                    None
                };

                if let Err(e) = move_path(src, dst) {
                    if let Some(b) = &backup {
                        let _ = move_path(b, dst);
                    }
                    abort_and_rollback(&executed_ops);
                    return Err(format!(
                        "Failed to move '{}' to '{}': {e}",
                        src.display(),
                        dst.display()
                    ));
                }
                executed_ops.push(FileOp::Move {
                    src: src.clone(),
                    dst: dst.clone(),
                    overwritten_backup: backup,
                });
            }
            FileOp::Copy {
                src,
                dst,
                is_dir,
                overwritten_backup: _,
            } => {
                if let Some(parent) = dst.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                let backup = if dst.exists() {
                    let _ = fs::create_dir_all(&backup_dir);
                    let b = backup_dir.join(format!(
                        "{idx:04}_{}",
                        dst.file_name().unwrap_or_default().to_string_lossy()
                    ));
                    if let Err(e) = move_path(dst, &b) {
                        abort_and_rollback(&executed_ops);
                        return Err(format!(
                            "Failed to backup existing '{}': {e}",
                            dst.display()
                        ));
                    }
                    Some(b)
                } else {
                    None
                };

                if let Err(e) = copy_recursive(src, dst) {
                    if let Some(b) = &backup {
                        let _ = move_path(b, dst);
                    }
                    abort_and_rollback(&executed_ops);
                    return Err(format!(
                        "Failed to copy '{}' to '{}': {e}",
                        src.display(),
                        dst.display()
                    ));
                }
                executed_ops.push(FileOp::Copy {
                    src: src.clone(),
                    dst: dst.clone(),
                    is_dir: *is_dir,
                    overwritten_backup: backup,
                });
            }
            FileOp::Trash {
                original_path,
                trash_path,
                is_dir,
            } => {
                if let Some(parent) = trash_path.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                if let Err(e) = move_path(original_path, trash_path) {
                    abort_and_rollback(&executed_ops);
                    return Err(format!(
                        "Failed to stage '{}' into trash: {e}",
                        original_path.display()
                    ));
                }
                executed_ops.push(FileOp::Trash {
                    original_path: original_path.clone(),
                    trash_path: trash_path.clone(),
                    is_dir: *is_dir,
                });
            }
            FileOp::Rename { old_path, new_path } => {
                if let Err(e) = move_path(old_path, new_path) {
                    abort_and_rollback(&executed_ops);
                    return Err(format!(
                        "Failed to rename '{}' to '{}': {e}",
                        old_path.display(),
                        new_path.display()
                    ));
                }
                executed_ops.push(FileOp::Rename {
                    old_path: old_path.clone(),
                    new_path: new_path.clone(),
                });
            }
            FileOp::Chmod {
                path,
                old_mode,
                new_mode,
            } => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let perms = fs::Permissions::from_mode(*new_mode);
                    if let Err(e) = fs::set_permissions(path, perms) {
                        abort_and_rollback(&executed_ops);
                        return Err(format!("Failed to chmod '{}': {e}", path.display()));
                    }
                    executed_ops.push(FileOp::Chmod {
                        path: path.clone(),
                        old_mode: *old_mode,
                        new_mode: *new_mode,
                    });
                }
                #[cfg(not(unix))]
                {
                    let _ = (path, old_mode, new_mode);
                }
            }
        }
    }

    let tx = Transaction {
        id: tx_id,
        timestamp_epoch_secs: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        summary,
        ops: executed_ops,
    };

    save_transaction(&tx)?;
    Ok(tx)
}

/// Recursively copy a file or directory tree, preserving symlinks to avoid infinite loops.
fn copy_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    let meta = fs::symlink_metadata(src)?;
    if meta.is_symlink() {
        #[cfg(unix)]
        {
            let target = fs::read_link(src)?;
            std::os::unix::fs::symlink(target, dst)?;
            return Ok(());
        }
        #[cfg(not(unix))]
        {
            fs::copy(src, dst)?;
            return Ok(());
        }
    }

    if meta.is_dir() {
        fs::create_dir_all(dst)?;
        for entry in fs::read_dir(src)? {
            let entry = entry?;
            let child_src = entry.path();
            let child_dst = dst.join(entry.file_name());
            copy_recursive(&child_src, &child_dst)?;
        }
        Ok(())
    } else {
        fs::copy(src, dst)?;
        Ok(())
    }
}

/// Move a file or directory, falling back to copy + delete if crossing filesystem boundaries (EXDEV).
fn move_path(src: &Path, dst: &Path) -> std::io::Result<()> {
    if let Err(err) = fs::rename(src, dst) {
        if src.exists() {
            copy_recursive(src, dst)?;
            if src.is_dir() {
                fs::remove_dir_all(src)?;
            } else {
                fs::remove_file(src)?;
            }
            Ok(())
        } else {
            Err(err)
        }
    } else {
        Ok(())
    }
}

/// Rollback executed operations in reverse order if a batch fails halfway.
fn rollback_executed(ops: &[FileOp]) {
    for op in ops.iter().rev() {
        match op {
            FileOp::Move {
                src,
                dst,
                overwritten_backup,
            } => {
                let _ = move_path(dst, src);
                if let Some(backup) = overwritten_backup {
                    let _ = move_path(backup, dst);
                }
            }
            FileOp::Copy {
                dst,
                is_dir,
                overwritten_backup,
                ..
            } => {
                if *is_dir {
                    let _ = fs::remove_dir_all(dst);
                } else {
                    let _ = fs::remove_file(dst);
                }
                if let Some(backup) = overwritten_backup {
                    let _ = move_path(backup, dst);
                }
            }
            FileOp::Trash {
                original_path,
                trash_path,
                ..
            } => {
                let _ = move_path(trash_path, original_path);
            }
            FileOp::Rename { old_path, new_path } => {
                let _ = move_path(new_path, old_path);
            }
            FileOp::Chmod { path, old_mode, .. } => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let perms = fs::Permissions::from_mode(*old_mode);
                    let _ = fs::set_permissions(path, perms);
                }
                #[cfg(not(unix))]
                {
                    let _ = (path, old_mode);
                }
            }
        }
    }
}

/// Parse chmod specification (+x, -w, 0755, etc.).
#[cfg(unix)]
fn parse_chmod_mode(current: u32, spec: &str) -> Result<u32, String> {
    if let Ok(octal) = u32::from_str_radix(spec, 8) {
        return Ok(octal);
    }
    match spec {
        "+x" => Ok(current | 0o111),
        "-x" => Ok(current & !0o111),
        "+w" => Ok(current | 0o222),
        "-w" => Ok(current & !0o222),
        "+r" => Ok(current | 0o444),
        "-r" => Ok(current & !0o444),
        other => Err(format!(
            "Unsupported chmod spec '{other}'. Use octal (0755) or +x/-x/+w/-w/+r/-r"
        )),
    }
}

/// Revert an entire transaction, restoring the filesystem to its previous state.
pub fn revert_transaction(tx: &Transaction) -> Result<(), String> {
    for op in tx.ops.iter().rev() {
        match op {
            FileOp::Move {
                src,
                dst,
                overwritten_backup,
            } => {
                if let Some(parent) = src.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                move_path(dst, src).map_err(|e| {
                    format!(
                        "Failed to restore '{}' to '{}': {e}",
                        dst.display(),
                        src.display()
                    )
                })?;
                if let Some(backup) = overwritten_backup {
                    let _ = move_path(backup, dst);
                }
            }
            FileOp::Copy {
                dst,
                is_dir,
                overwritten_backup,
                ..
            } => {
                if *is_dir {
                    let _ = fs::remove_dir_all(dst);
                } else {
                    let _ = fs::remove_file(dst);
                }
                if let Some(backup) = overwritten_backup {
                    let _ = move_path(backup, dst);
                }
            }
            FileOp::Trash {
                original_path,
                trash_path,
                ..
            } => {
                if let Some(parent) = original_path.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                move_path(trash_path, original_path).map_err(|e| {
                    format!(
                        "Failed to restore trashed item to '{}': {e}",
                        original_path.display()
                    )
                })?;
            }
            FileOp::Rename { old_path, new_path } => {
                move_path(new_path, old_path).map_err(|e| {
                    format!(
                        "Failed to rename '{}' back to '{}': {e}",
                        new_path.display(),
                        old_path.display()
                    )
                })?;
            }
            FileOp::Chmod { path, old_mode, .. } => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let perms = fs::Permissions::from_mode(*old_mode);
                    let _ = fs::set_permissions(path, perms);
                }
                #[cfg(not(unix))]
                {
                    let _ = (path, old_mode);
                }
            }
        }
    }

    // Remove transaction record once cleanly reverted
    let tx_path = Config::data_dir()
        .join("transactions")
        .join(format!("{}.json", tx.id));
    let _ = fs::remove_file(tx_path);

    Ok(())
}

/// Revert the latest transaction recorded in the WAL.
pub fn undo_latest() -> Result<Transaction, String> {
    let tx = load_latest_transaction()?;
    revert_transaction(&tx)?;
    Ok(tx)
}

/// Revert a specific transaction identified by its ID.
pub fn undo_transaction(tx_id: &str) -> Result<Transaction, String> {
    let tx = load_transaction_by_id(tx_id)?;
    revert_transaction(&tx)?;
    Ok(tx)
}

/// Load a specific transaction identified by its ID.
pub fn load_transaction_by_id(tx_id: &str) -> Result<Transaction, String> {
    let dir = Config::data_dir().join("transactions");
    let path = dir.join(format!("{tx_id}.json"));
    if !path.exists() {
        return Err(format!("Transaction '{tx_id}' not found in WAL."));
    }
    load_transaction(&path)
}

/// Serialize transaction to disk in JSON format without external serde dependency.
pub fn save_transaction(tx: &Transaction) -> Result<(), String> {
    let dir = Config::data_dir().join("transactions");
    fs::create_dir_all(&dir).map_err(|e| format!("Failed to create transactions dir: {e}"))?;
    let path = dir.join(format!("{}.json", tx.id));

    let mut json = String::new();
    let escape_json_str = |s: &str| -> String {
        let mut out = String::with_capacity(s.len() + 16);
        for c in s.chars() {
            match c {
                '\\' => out.push_str("\\\\"),
                '\"' => out.push_str("\\\""),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                c if (c as u32) < 0x20 => {
                    use std::fmt::Write;
                    let _ = write!(out, "\\u{:04x}", c as u32);
                }
                c => out.push(c),
            }
        }
        out
    };

    json.push_str("{\n");
    json.push_str(&format!("  \"id\": \"{}\",\n", tx.id));
    json.push_str(&format!("  \"timestamp\": {},\n", tx.timestamp_epoch_secs));
    json.push_str(&format!(
        "  \"summary\": \"{}\",\n",
        escape_json_str(&tx.summary)
    ));
    json.push_str("  \"ops\": [\n");

    let escape_json_path = |p: &Path| -> String { escape_json_str(&p.display().to_string()) };

    for (i, op) in tx.ops.iter().enumerate() {
        let comma = if i + 1 < tx.ops.len() { "," } else { "" };
        match op {
            FileOp::Move {
                src,
                dst,
                overwritten_backup,
            } => {
                let backup_str = overwritten_backup
                    .as_ref()
                    .map(|b| format!("\"{}\"", escape_json_path(b)))
                    .unwrap_or_else(|| "null".to_string());
                json.push_str(&format!(
                    "    {{\"type\":\"move\",\"src\":\"{}\",\"dst\":\"{}\",\"backup\":{}}}{comma}\n",
                    escape_json_path(src),
                    escape_json_path(dst),
                    backup_str
                ));
            }
            FileOp::Copy {
                src,
                dst,
                is_dir,
                overwritten_backup,
            } => {
                let backup_str = overwritten_backup
                    .as_ref()
                    .map(|b| format!("\"{}\"", escape_json_path(b)))
                    .unwrap_or_else(|| "null".to_string());
                json.push_str(&format!(
                    "    {{\"type\":\"copy\",\"src\":\"{}\",\"dst\":\"{}\",\"is_dir\":{},\"backup\":{}}}{comma}\n",
                    escape_json_path(src),
                    escape_json_path(dst),
                    is_dir,
                    backup_str
                ));
            }
            FileOp::Trash {
                original_path,
                trash_path,
                is_dir,
            } => {
                json.push_str(&format!(
                    "    {{\"type\":\"trash\",\"original\":\"{}\",\"trash\":\"{}\",\"is_dir\":{}}}{comma}\n",
                    escape_json_path(original_path),
                    escape_json_path(trash_path),
                    is_dir
                ));
            }
            FileOp::Rename { old_path, new_path } => {
                json.push_str(&format!(
                    "    {{\"type\":\"rename\",\"old\":\"{}\",\"new\":\"{}\"}}{comma}\n",
                    escape_json_path(old_path),
                    escape_json_path(new_path)
                ));
            }
            FileOp::Chmod {
                path,
                old_mode,
                new_mode,
            } => {
                json.push_str(&format!(
                    "    {{\"type\":\"chmod\",\"path\":\"{}\",\"old\":{},\"new\":{}}}{comma}\n",
                    escape_json_path(path),
                    old_mode,
                    new_mode
                ));
            }
        }
    }
    json.push_str("  ]\n}\n");

    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&path)
        .map_err(|e| format!("Failed to open transaction log '{}': {e}", path.display()))?;
    file.write_all(json.as_bytes())
        .map_err(|e| format!("Failed to write transaction log: {e}"))?;
    Ok(())
}

/// Load the most recent transaction from the WAL directory.
pub fn load_latest_transaction() -> Result<Transaction, String> {
    let dir = Config::data_dir().join("transactions");
    if !dir.exists() {
        return Err("No transactions found to undo.".into());
    }

    let mut entries = Vec::new();
    for entry in fs::read_dir(&dir).map_err(|e| format!("Failed to read transactions: {e}"))? {
        let entry = entry.map_err(|e| format!("Entry error: {e}"))?;
        let p = entry.path();
        if p.extension().is_some_and(|e| e == "json") {
            entries.push(p);
        }
    }

    entries.sort();
    let latest = entries
        .last()
        .ok_or_else(|| "No transactions found to undo.".to_string())?;

    load_transaction(latest)
}

/// Load and parse a transaction from a JSON file.
pub fn load_transaction(path: &Path) -> Result<Transaction, String> {
    let mut file = File::open(path)
        .map_err(|e| format!("Cannot open transaction '{}': {e}", path.display()))?;
    let mut content = String::new();
    file.read_to_string(&mut content)
        .map_err(|e| format!("Cannot read transaction: {e}"))?;

    let id = extract_json_field(&content, "id").unwrap_or_else(|| "unknown".to_string());
    let summary =
        extract_json_field(&content, "summary").unwrap_or_else(|| "operation".to_string());
    let ts = extract_json_field(&content, "timestamp")
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);

    let mut ops = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim().trim_end_matches(',');
        if !trimmed.starts_with('{') || !trimmed.contains("\"type\":") {
            continue;
        }
        let op_type = extract_json_field(trimmed, "type").unwrap_or_default();
        match op_type.as_str() {
            "move" => {
                if let (Some(src), Some(dst)) = (
                    extract_json_field(trimmed, "src"),
                    extract_json_field(trimmed, "dst"),
                ) {
                    let backup = extract_json_field(trimmed, "backup").map(PathBuf::from);
                    ops.push(FileOp::Move {
                        src: PathBuf::from(src),
                        dst: PathBuf::from(dst),
                        overwritten_backup: backup,
                    });
                }
            }
            "copy" => {
                if let (Some(src), Some(dst)) = (
                    extract_json_field(trimmed, "src"),
                    extract_json_field(trimmed, "dst"),
                ) {
                    let is_dir = trimmed.contains("\"is_dir\":true");
                    let backup = extract_json_field(trimmed, "backup").map(PathBuf::from);
                    ops.push(FileOp::Copy {
                        src: PathBuf::from(src),
                        dst: PathBuf::from(dst),
                        is_dir,
                        overwritten_backup: backup,
                    });
                }
            }
            "trash" => {
                if let (Some(orig), Some(trash)) = (
                    extract_json_field(trimmed, "original"),
                    extract_json_field(trimmed, "trash"),
                ) {
                    let is_dir = trimmed.contains("\"is_dir\":true");
                    ops.push(FileOp::Trash {
                        original_path: PathBuf::from(orig),
                        trash_path: PathBuf::from(trash),
                        is_dir,
                    });
                }
            }
            "rename" => {
                if let (Some(old), Some(new)) = (
                    extract_json_field(trimmed, "old"),
                    extract_json_field(trimmed, "new"),
                ) {
                    ops.push(FileOp::Rename {
                        old_path: PathBuf::from(old),
                        new_path: PathBuf::from(new),
                    });
                }
            }
            "chmod" => {
                let p = extract_json_field(trimmed, "path");
                let old = extract_json_field(trimmed, "old").and_then(|o| o.parse::<u32>().ok());
                let new = extract_json_field(trimmed, "new").and_then(|n| n.parse::<u32>().ok());
                if let (Some(path), Some(old_mode), Some(new_mode)) = (p, old, new) {
                    ops.push(FileOp::Chmod {
                        path: PathBuf::from(path),
                        old_mode,
                        new_mode,
                    });
                }
            }
            _ => {}
        }
    }

    Ok(Transaction {
        id,
        timestamp_epoch_secs: ts,
        summary,
        ops,
    })
}

/// Helper extracting string value from simple key-value JSON string.
fn extract_json_field(line: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\":");
    let pos = line.find(&needle)?;
    let rest = line[pos + needle.len()..].trim_start();
    if let Some(stripped) = rest.strip_prefix('"') {
        let mut end = None;
        let mut escaped = false;
        for (idx, b) in stripped.bytes().enumerate() {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                end = Some(idx);
                break;
            }
        }
        let end = end?;
        let mut unescaped = String::with_capacity(end);
        let mut chars = stripped[..end].chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                match chars.next() {
                    Some('"') => unescaped.push('"'),
                    Some('\\') => unescaped.push('\\'),
                    Some('/') => unescaped.push('/'),
                    Some('n') => unescaped.push('\n'),
                    Some('r') => unescaped.push('\r'),
                    Some('t') => unescaped.push('\t'),
                    Some('b') => unescaped.push('\x08'),
                    Some('f') => unescaped.push('\x0c'),
                    Some('u') => {
                        let hex: String = chars.by_ref().take(4).collect();
                        if let Ok(val) = u32::from_str_radix(&hex, 16)
                            && let Some(ch) = char::from_u32(val)
                        {
                            unescaped.push(ch);
                        }
                    }
                    Some(other) => {
                        unescaped.push('\\');
                        unescaped.push(other);
                    }
                    None => unescaped.push('\\'),
                }
            } else {
                unescaped.push(c);
            }
        }
        Some(unescaped)
    } else {
        let end = rest.find([',', '}', ' ']).unwrap_or(rest.len());
        let val = &rest[..end];
        if val == "null" {
            None
        } else {
            Some(val.to_string())
        }
    }
}

/// List all available transaction records.
pub fn list_transactions() -> Result<Vec<(String, u64, String)>, String> {
    let dir = Config::data_dir().join("transactions");
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut list = Vec::new();
    for entry in fs::read_dir(&dir).map_err(|e| format!("Failed to read transactions: {e}"))? {
        let entry = entry.map_err(|e| format!("Entry error: {e}"))?;
        let p = entry.path();
        if p.extension().is_some_and(|e| e == "json")
            && let Ok(tx) = load_transaction(&p)
        {
            list.push((tx.id, tx.timestamp_epoch_secs, tx.summary));
        }
    }
    list.sort_by_key(|a| std::cmp::Reverse(a.1));
    Ok(list)
}

/// Clean up the staging trash directory permanently.
pub fn clean_trash() -> Result<usize, String> {
    let trash_dir = Config::data_dir().join("trash");
    if !trash_dir.exists() {
        return Ok(0);
    }
    let mut count = 0;
    for entry in fs::read_dir(&trash_dir).map_err(|e| format!("Failed to read trash: {e}"))? {
        let entry = entry.map_err(|e| format!("Entry error: {e}"))?;
        let p = entry.path();
        let removed = if p.is_dir() {
            fs::remove_dir_all(&p).is_ok()
        } else {
            fs::remove_file(&p).is_ok()
        };
        if removed {
            count += 1;
        }
    }
    Ok(count)
}

#[cfg(test)]
pub(crate) static TEST_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_plan_move_and_execute_with_undo() {
        let _lock = TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = tempdir().unwrap();
        let src_file = tmp.path().join("source.txt");
        let dest_dir = tmp.path().join("dest");
        fs::write(&src_file, b"content").unwrap();
        fs::create_dir(&dest_dir).unwrap();

        let plan = plan_action(
            std::slice::from_ref(&src_file),
            &ActionKind::Move(dest_dir.clone()),
            false,
        )
        .unwrap();
        assert!(!plan.has_conflicts());
        assert_eq!(plan.ops.len(), 1);

        let tx = execute_plan(&plan).unwrap();
        assert!(!src_file.exists());
        assert!(dest_dir.join("source.txt").exists());

        revert_transaction(&tx).unwrap();
        assert!(src_file.exists());
        assert!(!dest_dir.join("source.txt").exists());
    }

    #[test]
    fn test_plan_copy_and_undo() {
        let _lock = TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = tempdir().unwrap();
        let src_file = tmp.path().join("file.txt");
        let dest_dir = tmp.path().join("dest");
        fs::write(&src_file, b"copy-content").unwrap();
        fs::create_dir(&dest_dir).unwrap();

        let plan = plan_action(
            std::slice::from_ref(&src_file),
            &ActionKind::Copy(dest_dir.clone()),
            false,
        )
        .unwrap();
        let tx = execute_plan(&plan).unwrap();
        assert!(src_file.exists());
        assert!(dest_dir.join("file.txt").exists());

        revert_transaction(&tx).unwrap();
        assert!(src_file.exists());
        assert!(!dest_dir.join("file.txt").exists());
    }

    #[test]
    fn test_plan_trash_and_undo() {
        let _lock = TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = tempdir().unwrap();
        let src_file = tmp.path().join("delete_me.txt");
        fs::write(&src_file, b"important data").unwrap();

        let plan = plan_action(std::slice::from_ref(&src_file), &ActionKind::Trash, false).unwrap();
        let tx = execute_plan(&plan).unwrap();
        assert!(!src_file.exists());

        revert_transaction(&tx).unwrap();
        assert!(src_file.exists());
        assert_eq!(fs::read(&src_file).unwrap(), b"important data");
    }

    #[test]
    fn test_collision_detection_without_force() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("item.txt");
        let dest_dir = tmp.path().join("dest");
        fs::write(&src, b"1").unwrap();
        fs::create_dir(&dest_dir).unwrap();
        fs::write(dest_dir.join("item.txt"), b"2").unwrap();

        let plan = plan_action(&[src], &ActionKind::Move(dest_dir), false).unwrap();
        assert!(plan.has_conflicts());
    }

    #[test]
    fn test_reject_copy_into_subdirectory_of_self() {
        let tmp = tempdir().unwrap();
        let src_dir = tmp.path().join("source_dir");
        fs::create_dir(&src_dir).unwrap();
        let dest_dir = src_dir.join("nested_backup");

        let plan = plan_action(&[src_dir], &ActionKind::Copy(dest_dir), false).unwrap();
        assert!(plan.has_conflicts());
        assert!(plan.conflicts[0].contains("into a subdirectory of itself"));
    }

    #[test]
    fn test_plan_copy_force_preserves_and_restores_overwritten_destination() {
        let _lock = TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = tempdir().unwrap();
        let src_file = tmp.path().join("source.txt");
        let dest_dir = tmp.path().join("dest");
        fs::create_dir(&dest_dir).unwrap();
        let target_file = dest_dir.join("source.txt");

        fs::write(&src_file, b"new content").unwrap();
        fs::write(&target_file, b"original content").unwrap();

        let plan = plan_action(
            std::slice::from_ref(&src_file),
            &ActionKind::Copy(dest_dir),
            true,
        )
        .unwrap();
        let tx = execute_plan(&plan).unwrap();

        assert_eq!(fs::read(&target_file).unwrap(), b"new content");
        assert_eq!(fs::read(&src_file).unwrap(), b"new content");

        revert_transaction(&tx).unwrap();
        assert_eq!(fs::read(&target_file).unwrap(), b"original content");
        assert_eq!(fs::read(&src_file).unwrap(), b"new content");
    }

    #[test]
    fn test_move_failure_restores_overwritten_destination_backup() {
        let _lock = TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = tempdir().unwrap();
        let src_file = tmp.path().join("nonexistent_source.txt");
        let dest_dir = tmp.path().join("dest");
        fs::create_dir(&dest_dir).unwrap();
        let target_file = dest_dir.join("nonexistent_source.txt");
        fs::write(&target_file, b"should survive failure").unwrap();

        let plan = OperationPlan {
            tx_id: "test-tx-fail".to_string(),
            action: ActionKind::Move(dest_dir),
            ops: vec![FileOp::Move {
                src: src_file,
                dst: target_file.clone(),
                overwritten_backup: None,
            }],
            conflicts: Vec::new(),
            force: true,
        };

        let res = execute_plan(&plan);
        assert!(res.is_err());
        assert!(target_file.exists());
        assert_eq!(fs::read(&target_file).unwrap(), b"should survive failure");
    }

    #[test]
    fn test_backup_and_trash_filename_collisions_handled_uniquely() {
        let _lock = TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = tempdir().unwrap();
        let dir_a = tmp.path().join("dir_a");
        let dir_b = tmp.path().join("dir_b");
        fs::create_dir(&dir_a).unwrap();
        fs::create_dir(&dir_b).unwrap();

        let file_a = dir_a.join("same_name.txt");
        let file_b = dir_b.join("same_name.txt");
        fs::write(&file_a, b"content a").unwrap();
        fs::write(&file_b, b"content b").unwrap();

        let plan =
            plan_action(&[file_a.clone(), file_b.clone()], &ActionKind::Trash, false).unwrap();
        let tx = execute_plan(&plan).unwrap();
        assert!(!file_a.exists());
        assert!(!file_b.exists());

        revert_transaction(&tx).unwrap();
        assert_eq!(fs::read(&file_a).unwrap(), b"content a");
        assert_eq!(fs::read(&file_b).unwrap(), b"content b");
    }

    #[test]
    fn test_save_and_load_transaction_with_quotes_in_filenames() {
        let tmp = tempdir().unwrap();
        let quoted_src = tmp.path().join("file_with_\"quote\".txt");
        let quoted_dst = tmp.path().join("dest_with_\"quote\".txt");

        let tx = Transaction {
            id: "tx-quote-test".to_string(),
            timestamp_epoch_secs: 123456789,
            summary: "Moved 1 \"quoted\" item".to_string(),
            ops: vec![FileOp::Move {
                src: quoted_src.clone(),
                dst: quoted_dst.clone(),
                overwritten_backup: Some(tmp.path().join("backup_\"quoted\".bak")),
            }],
        };

        let tx_path = tmp.path().join("tx-quote-test.json");
        let escape_json_path = |p: &Path| -> String {
            p.display()
                .to_string()
                .replace('\\', "\\\\")
                .replace('\"', "\\\"")
        };
        let mut json = String::new();
        json.push_str("{\n");
        json.push_str(&format!("  \"id\": \"{}\",\n", tx.id));
        json.push_str(&format!("  \"timestamp\": {},\n", tx.timestamp_epoch_secs));
        json.push_str(&format!(
            "  \"summary\": \"{}\",\n",
            tx.summary.replace('\"', "\\\"")
        ));
        json.push_str("  \"ops\": [\n");
        json.push_str(&format!(
            "    {{\"type\":\"move\",\"src\":\"{}\",\"dst\":\"{}\",\"backup\":\"{}\"}}\n",
            escape_json_path(&quoted_src),
            escape_json_path(&quoted_dst),
            escape_json_path(&tmp.path().join("backup_\"quoted\".bak"))
        ));
        json.push_str("  ]\n}\n");
        fs::write(&tx_path, json).unwrap();

        let loaded = load_transaction(&tx_path).unwrap();
        assert_eq!(loaded.id, "tx-quote-test");
        assert_eq!(loaded.summary, "Moved 1 \"quoted\" item");
        assert_eq!(loaded.ops.len(), 1);
        match &loaded.ops[0] {
            FileOp::Move {
                src,
                dst,
                overwritten_backup,
            } => {
                assert_eq!(src, &quoted_src);
                assert_eq!(dst, &quoted_dst);
                assert_eq!(
                    overwritten_backup.as_ref(),
                    Some(&tmp.path().join("backup_\"quoted\".bak"))
                );
            }
            _ => panic!("Expected FileOp::Move"),
        }
    }

    #[test]
    fn test_execute_plan_persists_and_cleans_wal_on_failure() {
        let _lock = TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        let dst_dir = tmp.path().join("dest");
        fs::create_dir_all(&dst_dir).unwrap();

        let f1 = tmp.path().join("f1.txt");
        fs::write(&f1, "hello").unwrap();
        let f2 = tmp.path().join("nonexistent_f2.txt");

        let plan = OperationPlan {
            tx_id: "test-abort".to_string(),
            action: ActionKind::Move(dst_dir.clone()),
            ops: vec![
                FileOp::Move {
                    src: f1.clone(),
                    dst: dst_dir.join("f1.txt"),
                    overwritten_backup: None,
                },
                FileOp::Move {
                    src: f2.clone(),
                    dst: dst_dir.join("f2.txt"),
                    overwritten_backup: None,
                },
            ],
            conflicts: Vec::new(),
            force: false,
        };

        let res = execute_plan(&plan);
        assert!(res.is_err());

        // First file should have been rolled back by abort_and_rollback
        assert!(f1.exists(), "f1 should be rolled back to src on failure");
        assert!(!dst_dir.join("f1.txt").exists());
    }

    #[test]
    fn test_undo_transaction_by_id() {
        let _lock = TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        let old_xdg = std::env::var("XDG_DATA_HOME").ok();
        unsafe {
            std::env::set_var("XDG_DATA_HOME", tmp.path());
        }

        let src = tmp.path().join("file.txt");
        let dest_dir = tmp.path().join("dest");
        fs::create_dir_all(&dest_dir).unwrap();
        fs::write(&src, "data").unwrap();

        let plan = OperationPlan {
            tx_id: "tx-target-test-123".to_string(),
            action: ActionKind::Move(dest_dir.clone()),
            ops: vec![FileOp::Move {
                src: src.clone(),
                dst: dest_dir.join("file.txt"),
                overwritten_backup: None,
            }],
            conflicts: Vec::new(),
            force: false,
        };

        let tx = execute_plan(&plan).unwrap();
        assert!(!src.exists());
        assert!(dest_dir.join("file.txt").exists());

        let loaded = load_transaction_by_id("tx-target-test-123").unwrap();
        assert_eq!(loaded.id, "tx-target-test-123");

        let undone = undo_transaction("tx-target-test-123").unwrap();
        assert_eq!(undone.id, tx.id);
        assert!(src.exists());
        assert!(!dest_dir.join("file.txt").exists());

        // Attempting to undo again should fail as file was removed
        assert!(undo_transaction("tx-target-test-123").is_err());

        unsafe {
            if let Some(val) = old_xdg {
                std::env::set_var("XDG_DATA_HOME", val);
            } else {
                std::env::remove_var("XDG_DATA_HOME");
            }
        }
    }

    #[test]
    fn test_clean_trash_accurate_accounting() {
        let _lock = TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        let old_xdg = std::env::var("XDG_DATA_HOME").ok();
        unsafe {
            std::env::set_var("XDG_DATA_HOME", tmp.path());
        }

        let trash_dir = tmp.path().join("grx").join("trash");
        fs::create_dir_all(&trash_dir).unwrap();
        fs::write(trash_dir.join("f1.tmp"), "content").unwrap();
        fs::write(trash_dir.join("f2.tmp"), "content").unwrap();
        let sub = trash_dir.join("subdir");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join("f3.tmp"), "nested").unwrap();

        let count = clean_trash().unwrap();
        assert_eq!(
            count, 3,
            "clean_trash should delete 2 files and 1 directory"
        );
        assert_eq!(
            clean_trash().unwrap(),
            0,
            "subsequent clean should report 0"
        );

        unsafe {
            if let Some(val) = old_xdg {
                std::env::set_var("XDG_DATA_HOME", val);
            } else {
                std::env::remove_var("XDG_DATA_HOME");
            }
        }
    }
}
