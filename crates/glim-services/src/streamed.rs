//! Bounded rolling cache for seekless compressed text; decoding advances on demand.
use crate::{
    limits,
    work::{Cancellation, SwitchReader},
};
use anyhow::{Context, Result, ensure};
use std::{
    fs::File,
    io::{BufReader, Read, Seek, SeekFrom, Write},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::SystemTime,
};
#[derive(Clone, Copy)]
pub enum Compression {
    Gzip,
    Bzip2,
    Xz,
    Slice {
        offset: u64,
        length: u64,
    },
    Zip {
        offset: u64,
        compressed: u64,
        size: u64,
        crc: u32,
        deflate: bool,
    },
}
pub struct StreamedText {
    path: PathBuf,
    compression: Compression,
    reader: Box<dyn Read + Send>,
    cancel: Arc<Mutex<Cancellation>>,
    lifetime: Cancellation,
    pub file: Arc<tempfile::NamedTempFile>,
    pub start: u64,
    pub end: u64,
    pub complete: bool,
    restart: bool,
    stamp: (u64, Option<SystemTime>),
}
impl StreamedText {
    fn open(
        path: &std::path::Path,
        compression: Compression,
        cancel: Arc<Mutex<Cancellation>>,
    ) -> Result<Box<dyn Read + Send>> {
        let input = SwitchReader {
            inner: BufReader::with_capacity(limits::IO_BUFFER_BYTES, File::open(path)?),
            cancel,
        };
        Ok(match compression {
            Compression::Slice { offset, length } => {
                let mut input = input;
                input.inner.seek(SeekFrom::Start(offset))?;
                Box::new(input.take(length))
            }
            Compression::Zip {
                offset,
                compressed,
                size,
                crc,
                deflate,
            } => {
                let mut input = input;
                input.inner.seek(SeekFrom::Start(offset))?;
                let input = input.take(compressed);
                let reader: Box<dyn Read + Send> = if deflate {
                    Box::new(flate2::read::DeflateDecoder::new(input))
                } else {
                    Box::new(input)
                };
                Box::new(CheckedMember {
                    reader,
                    hash: crc32fast::Hasher::new(),
                    remaining: size,
                    crc,
                })
            }
            Compression::Gzip => Box::new(flate2::read::MultiGzDecoder::new(input)),
            Compression::Bzip2 => Box::new(bzip2::read::MultiBzDecoder::new(input)),
            Compression::Xz => Box::new(xz2::read::XzDecoder::new_multi_decoder(input)),
        })
    }
    pub fn new(path: PathBuf, compression: Compression, lifetime: Cancellation) -> Result<Self> {
        let cancel = Arc::new(Mutex::new(lifetime.clone()));
        let reader = Self::open(&path, compression, cancel.clone())?;
        let meta = std::fs::metadata(&path)?;
        Ok(Self {
            path,
            compression,
            reader,
            cancel,
            lifetime,
            file: Arc::new(tempfile::NamedTempFile::new()?),
            start: 0,
            end: 0,
            complete: false,
            restart: false,
            stamp: (meta.len(), meta.modified().ok()),
        })
    }
    pub fn ensure_range(
        &mut self,
        start: u64,
        length: u64,
        operation: &Cancellation,
    ) -> Result<()> {
        *self
            .cancel
            .lock()
            .map_err(|_| anyhow::anyhow!("Reader interrupted"))? = self.lifetime.linked(operation);
        let result = self.fill(start, length, operation);
        if result.is_err() {
            self.restart = true;
        }
        *self
            .cancel
            .lock()
            .map_err(|_| anyhow::anyhow!("Reader interrupted"))? = self.lifetime.clone();
        result
    }
    fn fill(&mut self, start: u64, length: u64, operation: &Cancellation) -> Result<()> {
        let meta = std::fs::metadata(&self.path)?;
        ensure!(
            (meta.len(), meta.modified().ok()) == self.stamp,
            "Compressed source changed; reopen it"
        );
        let wanted = start.checked_add(length).context("Page offset overflow")?;
        if self.restart || start < self.start {
            self.reader = Self::open(&self.path, self.compression, self.cancel.clone())?;
            self.start = 0;
            self.end = 0;
            self.complete = false;
            self.restart = false;
            self.file.as_file().set_len(0)?;
        }
        if !self.complete && wanted > self.end {
            let mut buffer = vec![0; limits::IO_BUFFER_BYTES];
            let mut cache = self.file.reopen()?;
            if start > self.end {
                // Seeking past the cached range discards all older physical bytes.
                // Otherwise a short tail page can expose stale data after its end.
                cache.set_len(0)?;
                self.start = self.end;
            }
            while self.end < wanted && !self.complete {
                operation.check()?;
                self.lifetime.check()?;
                let size = usize::try_from((wanted - self.end).min(buffer.len() as u64))?;
                let read = self.reader.read(&mut buffer[..size])?;
                if read == 0 {
                    self.complete = true;
                    break;
                }
                // Skip uncached history while reconstructing an earlier requested page.
                let next = self.end + read as u64;
                if next > start {
                    if self.end - self.start + read as u64 > limits::IO_CACHE_BYTES
                        && self.start < start
                    {
                        // Retain only the requested range; the source can reproduce history.
                        let keep = self.end.saturating_sub(start);
                        if keep > 0 {
                            let mut previous = self.file.reopen()?;
                            previous.seek(SeekFrom::Start(start - self.start))?;
                            let mut retained = tempfile::NamedTempFile::new()?;
                            operation.copy(previous.take(keep), retained.as_file_mut())?;
                            cache.set_len(0)?;
                            cache.seek(SeekFrom::Start(0))?;
                            retained.as_file_mut().seek(SeekFrom::Start(0))?;
                            operation.copy(retained.as_file_mut(), &mut cache)?;
                        } else {
                            cache.set_len(0)?;
                        }
                        self.start = start.min(self.end);
                    }
                    let skip = start.saturating_sub(self.end).min(read as u64) as usize;
                    if self.end < start {
                        self.start = start;
                    }
                    cache.seek(SeekFrom::Start(self.end.max(start) - self.start))?;
                    cache.write_all(&buffer[skip..read])?;
                } else {
                    self.start = next;
                }
                self.end = next;
            }
        }
        Ok(())
    }
}

struct CheckedMember {
    reader: Box<dyn Read + Send>,
    hash: crc32fast::Hasher,
    remaining: u64,
    crc: u32,
}
impl Read for CheckedMember {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        let count = self.reader.read(buffer)?;
        if count as u64 > self.remaining {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "ZIP member size mismatch",
            ));
        }
        self.remaining -= count as u64;
        self.hash.update(&buffer[..count]);
        if count == 0 && (self.remaining != 0 || self.hash.clone().finalize() != self.crc) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "ZIP member integrity check failed",
            ));
        }
        Ok(count)
    }
}
