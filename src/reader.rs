use crate::core::{BufferRef, ContentReader};
use memmap2::Mmap;
use std::fs::File;
use std::io::{self, Read, Seek};
use std::path::Path;

/// Adaptive content reader that automatically switches between reusable thread-local
/// buffered reads and memory-mapped files based on file size thresholds.
pub struct AdaptiveReader {
    /// File size threshold in bytes above which mmap is chosen.
    pub mmap_threshold: u64,
    /// Maximum allowed file size to read (None = unlimited).
    pub max_file_size: Option<u64>,
    /// Configured scratch buffer allocation size.
    pub buffer_size: usize,
}

impl AdaptiveReader {
    pub fn new(mmap_threshold: u64, max_file_size: Option<u64>, buffer_size: usize) -> Self {
        Self {
            mmap_threshold,
            max_file_size,
            buffer_size,
        }
    }
}

impl ContentReader for AdaptiveReader {
    fn read<'a>(&self, path: &Path, scratch: &'a mut Vec<u8>) -> io::Result<BufferRef<'a>> {
        let mut file = File::open(path)?;

        if self.mmap_threshold > 0 {
            scratch.clear();
            let probe_capacity =
                usize::try_from(self.mmap_threshold.saturating_add(1)).unwrap_or(usize::MAX);
            let required_capacity = self.buffer_size.max(probe_capacity);
            if scratch.capacity() < required_capacity {
                scratch
                    .try_reserve_exact(required_capacity - scratch.capacity())
                    .map_err(io::Error::other)?;
            }

            // Read up to mmap_threshold + 1 bytes.
            // For files <= mmap_threshold, this single read_to_end completes the read
            // without needing an fstat(2) syscall.
            let mut handle = (&mut file).take(self.mmap_threshold + 1);
            handle.read_to_end(scratch)?;

            if (scratch.len() as u64) <= self.mmap_threshold {
                if let Some(max) = self.max_file_size
                    && scratch.len() as u64 > max
                {
                    return Ok(BufferRef::Borrowed(&[]));
                }
                return Ok(BufferRef::Borrowed(scratch.as_slice()));
            }
        }

        // File exceeds mmap_threshold: inspect metadata for size bounds and mmap
        let metadata = file.metadata()?;
        let len = metadata.len();

        if let Some(max) = self.max_file_size
            && len > max
        {
            return Ok(BufferRef::Borrowed(&[]));
        }

        if len == 0 {
            return Ok(BufferRef::Borrowed(&[]));
        }

        match unsafe { Mmap::map(&file) } {
            Ok(mmap) => {
                #[cfg(unix)]
                {
                    use std::os::unix::io::AsRawFd;
                    unsafe {
                        libc::posix_fadvise(
                            file.as_raw_fd(),
                            0,
                            len as libc::off_t,
                            libc::POSIX_FADV_SEQUENTIAL,
                        );
                    }
                }
                Ok(BufferRef::Mmap(mmap))
            }
            Err(_) => {
                // Fallback to sequential read
                scratch.clear();
                file.rewind()?;
                file.read_to_end(scratch)?;
                Ok(BufferRef::Borrowed(scratch.as_slice()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_adaptive_reader_small_file() {
        let mut tmp = NamedTempFile::new().unwrap();
        tmp.write_all(b"small file content").unwrap();

        let reader = AdaptiveReader::new(64 * 1024, None, 64 * 1024);
        let mut scratch = Vec::new();
        let buf = reader.read(tmp.path(), &mut scratch).unwrap();

        assert_eq!(&*buf, b"small file content");
    }

    #[test]
    fn test_adaptive_reader_mmap_threshold() {
        let mut tmp = NamedTempFile::new().unwrap();
        let data = vec![b'x'; 1000];
        tmp.write_all(&data).unwrap();

        // Set threshold to 500 bytes so this file triggers mmap branch
        let reader = AdaptiveReader::new(500, None, 64 * 1024);
        let mut scratch = Vec::new();
        let buf = reader.read(tmp.path(), &mut scratch).unwrap();

        assert_eq!(buf.len(), 1000);
        assert_eq!(&buf[..5], b"xxxxx");
    }

    #[test]
    fn test_adaptive_reader_empty_file() {
        let tmp = NamedTempFile::new().unwrap();
        let reader = AdaptiveReader::new(64 * 1024, None, 64 * 1024);
        let mut scratch = Vec::new();
        let buf = reader.read(tmp.path(), &mut scratch).unwrap();
        assert!(buf.is_empty());
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn test_adaptive_reader_proc_virtual_file() {
        let path = Path::new("/proc/version");
        if path.exists() {
            let reader = AdaptiveReader::new(64 * 1024, None, 64 * 1024);
            let mut scratch = Vec::new();
            let buf = reader.read(path, &mut scratch).unwrap();
            assert!(!buf.is_empty());
            assert!(buf.starts_with(b"Linux version"));
        }
    }
}
