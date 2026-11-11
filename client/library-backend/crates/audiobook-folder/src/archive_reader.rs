//! A canonical ZIP backed by its original files instead of a temporary copy.
//! The normal ZIP writer still computes CRCs and patches its own headers.

use std::cell::RefCell;
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use audiobook_archive::Track;

enum Contents {
    Bytes(Vec<u8>),
    Source { path: PathBuf, length: u64 },
}

struct Block {
    start: u64,
    contents: Contents,
}

impl Block {
    fn length(&self) -> u64 {
        match &self.contents {
            Contents::Bytes(bytes) => bytes.len() as u64,
            Contents::Source { length, .. } => *length,
        }
    }
}

#[derive(Default)]
struct Layout {
    blocks: Vec<Block>,
    position: u64,
    length: u64,
}

impl Layout {
    fn source(&mut self, path: &Path, length: u64) -> io::Result<()> {
        if self.position != self.length {
            return Err(io::Error::other("audiobook payload does not follow its ZIP header"));
        }
        if length != 0 {
            let end = self.length.checked_add(length).ok_or_else(|| io::Error::other("audiobook archive size overflow"))?;
            self.blocks.push(Block { start: self.position, contents: Contents::Source { path: path.to_owned(), length } });
            self.length = end;
        }
        Ok(())
    }

    fn block_index(&self) -> usize {
        self.blocks.partition_point(|block| block.start <= self.position).saturating_sub(1)
    }

    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let position = match from {
            SeekFrom::Start(position) => Some(position),
            SeekFrom::Current(offset) => self.position.checked_add_signed(offset),
            SeekFrom::End(offset) => self.length.checked_add_signed(offset),
        }
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid audiobook archive seek"))?;
        self.position = position;
        Ok(position)
    }
}

struct Writer(Rc<RefCell<Layout>>);

impl Write for Writer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.is_empty() {
            return Ok(0);
        }
        let mut layout = self.0.borrow_mut();
        if layout.position > layout.length {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "audiobook ZIP writer skipped unwritten bytes"));
        }
        if layout.position == layout.length {
            let start = layout.position;
            let end = start.checked_add(bytes.len() as u64).ok_or_else(|| io::Error::other("audiobook archive size overflow"))?;
            match layout.blocks.last_mut() {
                Some(Block { contents: Contents::Bytes(previous), .. }) => previous.extend_from_slice(bytes),
                _ => layout.blocks.push(Block { start, contents: Contents::Bytes(bytes.to_vec()) }),
            }
            layout.position = end;
            layout.length = end;
            return Ok(bytes.len());
        }
        let index = layout.block_index();
        let offset = layout.position - layout.blocks[index].start;
        let block = &mut layout.blocks[index];
        let count = bytes.len().min((block.length() - offset).min(usize::MAX as u64) as usize);
        if let Contents::Bytes(header) = &mut block.contents {
            let offset = offset as usize;
            header[offset..offset + count].copy_from_slice(&bytes[..count]);
        }
        // Payload bytes already live in the original file. ZIP's writer still
        // receives them and computes the same CRC as a materialized archive.
        layout.position += count as u64;
        Ok(count)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Seek for Writer {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        self.0.borrow_mut().seek(from)
    }
}

/// Reject a changing source before it can spill beyond its reserved ZIP range.
struct Source<R> {
    reader: R,
    remaining: u64,
}

impl<R: Read> Read for Source<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        if self.remaining == 0 {
            return match self.reader.read(&mut [0])? {
                0 => Ok(0),
                _ => Err(io::Error::new(io::ErrorKind::InvalidData, "audiobook source grew while archiving")),
            };
        }
        let maximum = buffer.len().min(self.remaining.min(usize::MAX as u64) as usize);
        let count = self.reader.read(&mut buffer[..maximum])?;
        if count == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "audiobook source shrank while archiving"));
        }
        self.remaining -= count as u64;
        Ok(count)
    }
}

/// Seekable canonical archive bytes. Only ZIP headers occupy memory; audio and
/// sidecar payloads are read from the original files on demand.
/// The ingestion owner must validate source revisions around its use: this is
/// a file-backed view, not a snapshot of mutable source files.
pub struct FolderArchive {
    layout: Layout,
    source: Option<(usize, File, u64)>,
}

impl Read for FolderArchive {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() || self.layout.position >= self.layout.length {
            return Ok(0);
        }
        let index = self.layout.block_index();
        let block = &self.layout.blocks[index];
        let offset = self.layout.position - block.start;
        let maximum = buffer.len().min((block.length() - offset).min(usize::MAX as u64) as usize);
        let count = match &block.contents {
            Contents::Bytes(bytes) => {
                buffer[..maximum].copy_from_slice(&bytes[offset as usize..offset as usize + maximum]);
                maximum
            }
            Contents::Source { path, .. } => {
                if self.source.as_ref().is_none_or(|(active, _, _)| *active != index) {
                    self.source = Some((index, File::open(path)?, 0));
                }
                let (_, file, position) = self.source.as_mut().unwrap();
                if *position != offset {
                    file.seek(SeekFrom::Start(offset))?;
                }
                let count = file.read(&mut buffer[..maximum])?;
                if count == 0 {
                    return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "audiobook source shrank while reading archive"));
                }
                *position = offset + count as u64;
                count
            }
        };
        self.layout.position += count as u64;
        Ok(count)
    }
}

impl Seek for FolderArchive {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        self.layout.seek(from)
    }
}

/// Build the ordinary archive layout and CRCs without copying payloads to disk.
pub fn folder_archive_reader<R: Read>(folder: &Path, tracks: &[Track], mut open: impl FnMut(&Path) -> io::Result<R>) -> io::Result<FolderArchive> {
    let layout = Rc::new(RefCell::new(Layout::default()));
    let writer = super::write_folder_archive_with_reader(folder, Writer(layout.clone()), tracks, |path| {
        let reader = open(path)?;
        let length = fs::metadata(path)?.len();
        layout.borrow_mut().source(path, length)?;
        Ok(Source { reader, remaining: length })
    })?;
    drop(writer);
    let mut layout = Rc::try_unwrap(layout).map_err(|_| io::Error::other("audiobook archive writer is still active"))?.into_inner();
    layout.position = 0;
    Ok(FolderArchive { layout, source: None })
}
