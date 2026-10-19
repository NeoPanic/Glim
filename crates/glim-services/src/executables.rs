//! Bounded, read-only inspection of executable files; never run the inspected file.
use crate::limits as reader_limits;
use crate::{paged::TextPage, report::Report, work::Cancellation};
use anyhow::{Context, Result, ensure};
use object::{
    Object, ObjectSection, ObjectSegment,
    read::{ReadRef, macho::FatArch},
};
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeSet, HashSet},
    fmt::Write as _,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

// object::ReadCache never evicts references, and its built-in C-string reader
// stops after 4096 bytes. Bound its live blocks before allocation and scan
// strings through shared aligned blocks, keeping full names within this budget.
#[derive(Default)]
struct CacheUsage {
    bytes: u64,
    ranges: HashSet<(u64, u64)>,
}
struct MetadataCache<'budget> {
    inner: object::read::ReadCache<crate::work::Reader<std::fs::File>>,
    usage: RefCell<CacheUsage>,
    exhausted: &'budget Cell<bool>,
}
impl<'budget> MetadataCache<'budget> {
    fn range(&self, offset: u64, size: u64) -> MetadataRange<'_, 'budget> {
        MetadataRange {
            cache: self,
            offset,
            size,
        }
    }
}
impl<'a> ReadRef<'a> for &'a MetadataCache<'_> {
    fn len(self) -> std::result::Result<u64, ()> {
        (&self.inner).len()
    }
    fn read_bytes_at(self, offset: u64, size: u64) -> std::result::Result<&'a [u8], ()> {
        if size == 0 {
            return Ok(&[]);
        }
        if offset
            .checked_add(size)
            .is_none_or(|end| end > self.len().unwrap_or(0))
        {
            return Err(());
        }
        let mut usage = self.usage.borrow_mut();
        if !usage.ranges.contains(&(offset, size)) {
            let bookkeeping = (std::mem::size_of::<(u64, u64, Box<[u8]>)>() * 4) as u64;
            let charge = size.checked_add(bookkeeping).ok_or(())?;
            if charge > reader_limits::IO_CACHE_BYTES.saturating_sub(usage.bytes) {
                self.exhausted.set(true);
                return Err(());
            }
            usage.bytes += charge;
            usage.ranges.insert((offset, size));
        }
        drop(usage);
        (&self.inner).read_bytes_at(offset, size)
    }
    fn read_bytes_at_until(
        self,
        range: std::ops::Range<u64>,
        delimiter: u8,
    ) -> std::result::Result<&'a [u8], ()> {
        let length = self.len()?;
        if range.start > range.end || range.end > length {
            return Err(());
        }
        let mut offset = range.start;
        while offset < range.end {
            let block = offset / reader_limits::IO_BUFFER_BYTES as u64
                * reader_limits::IO_BUFFER_BYTES as u64;
            let size = (reader_limits::IO_BUFFER_BYTES as u64).min(length - block);
            let bytes = self.read_bytes_at(block, size)?;
            let begin = (offset - block) as usize;
            let end = (range.end - block).min(size) as usize;
            if let Some(index) = bytes[begin..end].iter().position(|byte| *byte == delimiter) {
                return self.read_bytes_at(range.start, offset - range.start + index as u64);
            }
            offset = block + size;
        }
        Err(())
    }
}
#[derive(Clone, Copy)]
struct MetadataRange<'a, 'budget> {
    cache: &'a MetadataCache<'budget>,
    offset: u64,
    size: u64,
}
impl<'a> ReadRef<'a> for MetadataRange<'a, '_> {
    fn len(self) -> std::result::Result<u64, ()> {
        Ok(self.size)
    }
    fn read_bytes_at(self, offset: u64, size: u64) -> std::result::Result<&'a [u8], ()> {
        if offset.checked_add(size).is_none_or(|end| end > self.size) {
            return Err(());
        }
        self.cache
            .read_bytes_at(self.offset.checked_add(offset).ok_or(())?, size)
    }
    fn read_bytes_at_until(
        self,
        range: std::ops::Range<u64>,
        delimiter: u8,
    ) -> std::result::Result<&'a [u8], ()> {
        if range.start > range.end || range.end > self.size {
            return Err(());
        }
        self.cache.read_bytes_at_until(
            self.offset.checked_add(range.start).ok_or(())?
                ..self.offset.checked_add(range.end).ok_or(())?,
            delimiter,
        )
    }
}

pub fn supports(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    let mut header = [0; 64];
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let Ok(count) = file.read(&mut header) else {
        return false;
    };
    if header.starts_with(b"MZ") {
        if count < 64 {
            return false;
        }
        let offset = u32::from_le_bytes(header[60..64].try_into().unwrap()) as u64;
        if !file
            .metadata()
            .is_ok_and(|m| m.is_file() && offset + 4 <= m.len())
        {
            return false;
        }
        let mut signature = [0; 4];
        return file.seek(SeekFrom::Start(offset)).is_ok()
            && file.read_exact(&mut signature).is_ok()
            && signature == *b"PE\0\0";
    }
    has_magic(&header[..count])
}
fn has_magic(bytes: &[u8]) -> bool {
    let Some(magic) = bytes.get(..4) else {
        return false;
    };
    if magic == b"\x7fELF" {
        return matches!(bytes.get(4), Some(1 | 2)) && matches!(bytes.get(5), Some(1 | 2));
    }
    match magic {
        b"\xfe\xed\xfa\xce" | b"\xce\xfa\xed\xfe" | b"\xfe\xed\xfa\xcf" | b"\xcf\xfa\xed\xfe" => {
            true
        }
        // Java class files share CAFEBABE. Require a plausible Mach-O fat
        // architecture count and first CPU type before routing to this reader.
        b"\xca\xfe\xba\xbe" | b"\xca\xfe\xba\xbf" => {
            let Some(count) = bytes.get(4..8) else {
                return false;
            };
            let Some(cpu) = bytes.get(8..12) else {
                return false;
            };
            let count = u32::from_be_bytes(count.try_into().unwrap());
            let cpu = u32::from_be_bytes(cpu.try_into().unwrap());
            count > 0 && matches!(cpu, 7 | 12 | 18 | 0x01000007 | 0x0100000c | 0x01000012)
        }
        _ => false,
    }
}
pub fn read_preview(path: &Path) -> Result<TextPage> {
    read_preview_cancellable(path, &Cancellation::default())
}
pub fn read_preview_cancellable(path: &Path, cancel: &Cancellation) -> Result<TextPage> {
    let exhausted = Cell::new(false);
    let result = read_report(path, cancel, &exhausted);
    if exhausted.get() {
        cancel.check()?;
        return crate::paged::read_binary_page(path, 0);
    }
    result
}
fn read_report(path: &Path, cancel: &Cancellation, exhausted: &Cell<bool>) -> Result<TextPage> {
    let file = std::fs::File::open(path)?;
    let metadata = file.metadata()?;
    ensure!(
        metadata.is_file(),
        "Executable preview requires a regular file"
    );
    let mut prefix = Vec::new();
    (&file)
        .take(reader_limits::EXECUTABLE_HEX_BYTES as u64)
        .read_to_end(&mut prefix)?;
    let cache = MetadataCache {
        inner: object::read::ReadCache::new(crate::work::Reader {
            inner: file,
            cancel: cancel.clone(),
        }),
        usage: RefCell::new(CacheUsage::default()),
        exhausted,
    };
    let mut text = Report::new()?;
    writeln!(
        text,
        "Executable · Read-only inspection\nFile: {}\nSize: {} bytes\n",
        path.file_name().unwrap_or_default().to_string_lossy(),
        metadata.len()
    )?;
    match object::FileKind::parse(&cache).context("Invalid executable header")? {
        object::FileKind::MachOFat32 => {
            let fat = object::read::macho::MachOFatFile32::parse(&cache)?;
            writeln!(
                text,
                "Container: Universal Mach-O · {} architectures\n",
                fat.arches().len()
            )?;
            for arch in fat.arches() {
                cancel.check()?;
                let (offset, size) = arch.file_range();
                describe(cache.range(offset, size), &mut text, cancel)?;
            }
        }
        object::FileKind::MachOFat64 => {
            let fat = object::read::macho::MachOFatFile64::parse(&cache)?;
            writeln!(
                text,
                "Container: Universal Mach-O · {} architectures\n",
                fat.arches().len()
            )?;
            for arch in fat.arches() {
                cancel.check()?;
                let (offset, size) = arch.file_range();
                describe(cache.range(offset, size), &mut text, cancel)?;
            }
        }
        _ => describe(&cache, &mut text, cancel)?,
    }
    writeln!(
        text,
        "\nHex preview · first {} bytes\nOffset    Hex bytes                                         ASCII",
        prefix.len()
    )?;
    for (row, chunk) in prefix
        .iter()
        .take(reader_limits::EXECUTABLE_HEX_BYTES)
        .copied()
        .collect::<Vec<_>>()
        .chunks(16)
        .enumerate()
    {
        write!(text, "{:08x}  ", row * 16)?;
        for index in 0..16 {
            match chunk.get(index) {
                Some(byte) => write!(text, "{byte:02x} ")?,
                None => text.push_str("   "),
            }
        }
        text.push(' ');
        for &byte in chunk {
            text.push(if byte.is_ascii_graphic() || byte == b' ' {
                char::from(byte)
            } else {
                '.'
            });
        }
        text.push('\n');
    }
    text.finish(path)
}
fn describe<'a, R: object::read::ReadRef<'a>>(
    bytes: R,
    text: &mut Report,
    cancel: &Cancellation,
) -> Result<()> {
    let file = object::File::parse(bytes).context("Invalid or unsupported executable structure")?;
    ensure!(
        matches!(
            file.format(),
            object::BinaryFormat::MachO | object::BinaryFormat::Elf | object::BinaryFormat::Pe
        ),
        "Unsupported executable format"
    );
    writeln!(
        text,
        "Format: {:?}\nArchitecture: {:?}\nKind: {:?}\nWord size: {} bits\nByte order: {}\nEntry: 0x{:x}\n",
        file.format(),
        file.architecture(),
        file.kind(),
        if file.is_64() { 64 } else { 32 },
        if file.is_little_endian() {
            "little-endian"
        } else {
            "big-endian"
        },
        file.entry()
    )?;
    text.push_str("Segments (preview)\nAddress           Size              File offset       File size         Name\n");
    for segment in file.segments() {
        cancel.check()?;
        let (offset, size) = segment.file_range();
        writeln!(
            text,
            "{:016x}  {:016x}  {:016x}  {:016x}  {}",
            segment.address(),
            segment.size(),
            offset,
            size,
            segment
                .name()
                .ok()
                .flatten()
                .unwrap_or("")
                .chars()
                .collect::<String>()
        )?;
    }
    text.push_str("\nSections (preview)\nAddress           Size              Name\n");
    for section in file.sections() {
        cancel.check()?;
        writeln!(
            text,
            "{:016x}  {:016x}  {}",
            section.address(),
            section.size(),
            section
                .name()
                .unwrap_or("<invalid name>")
                .chars()
                .collect::<String>()
        )?;
    }
    text.push_str("\nImported libraries\n");
    match libraries(&file, cancel) {
        Ok(libraries) => {
            if libraries.is_empty() {
                text.push_str("No library names available in these headers.\n");
            }
            for library in libraries {
                writeln!(text, "{library}")?;
            }
        }
        Err(_) => text.push_str("Dependency metadata unavailable or malformed.\n"),
    }
    text.push('\n');
    Ok(())
}
fn libraries<'a, R: object::read::ReadRef<'a>>(
    file: &object::File<'a, R>,
    cancel: &Cancellation,
) -> Result<BTreeSet<String>> {
    use object::read::elf::Dyn;
    let mut result = BTreeSet::new();
    let short = |bytes: &[u8]| String::from_utf8_lossy(bytes).into_owned();
    macro_rules! macho {
        ($file:expr) => {{
            let f = $file;
            let mut commands = f.macho_load_commands()?;
            loop {
                cancel.check()?;
                let Some(command) = commands.next()? else {
                    break;
                };
                if command.cmd() != object::macho::LC_ID_DYLIB {
                    if let Some(dylib) = command.dylib()? {
                        result.insert(short(command.string(f.endian(), dylib.dylib.name)?));
                    }
                }
            }
        }};
    }
    macro_rules! pe {
        ($file:expr) => {{
            if let Some(table) = $file.import_table()? {
                let mut descriptors = table.descriptors()?;
                loop {
                    cancel.check()?;
                    let Some(descriptor) = descriptors.next()? else {
                        break;
                    };
                    result.insert(short(
                        table.name(descriptor.name.get(object::LittleEndian))?,
                    ));
                }
            }
        }};
    }
    macro_rules! elf {
        ($file:expr) => {{
            let f = $file;
            let sections = f.elf_section_table();
            if let Some((dynamic, index)) = sections.dynamic(f.endian(), f.data())? {
                let strings = sections.strings(f.endian(), f.data(), index)?;
                for entry in dynamic.iter() {
                    if entry.tag32(f.endian()) == Some(object::elf::DT_NEEDED) {
                        result.insert(short(
                            strings
                                .get(
                                    entry
                                        .val32(f.endian())
                                        .context("Invalid ELF string offset")?,
                                )
                                .map_err(|_| anyhow::anyhow!("Invalid ELF string table"))?,
                        ));
                    }
                }
            }
        }};
    }
    match file {
        object::File::MachO32(f) => macho!(f),
        object::File::MachO64(f) => macho!(f),
        object::File::Pe32(f) => pe!(f),
        object::File::Pe64(f) => pe!(f),
        object::File::Elf32(f) => elf!(f),
        object::File::Elf64(f) => elf!(f),
        _ => {}
    }
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn magic_distinguishes_text_and_java_from_native_binaries() {
        assert!(has_magic(b"\xcf\xfa\xed\xfe\x0c\0\0\x01\0\0\0\0"));
        assert!(has_magic(b"\x7fELF\x02\x01\x01\0"));
        assert!(!has_magic(b"MZ\0\0"));
        assert!(!has_magic(b"#!/bin/sh\n"));
        assert!(!has_magic(b"\xca\xfe\xba\xbe\0\0\0\x3d\0\x10\x07\0"));
        assert!(!has_magic(b"\xca\xfe\xba\xbe\0\0\0\x01\0\0\0\0"));
    }
    #[test]
    fn mz_text_is_not_misclassified_as_pe_and_pe_offsets_are_checked() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file");
        std::fs::write(&path, b"MZ is ordinary text, not an executable.").unwrap();
        assert!(!supports(&path));
        let mut bytes = vec![0; 128];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[60..64].copy_from_slice(&64_u32.to_le_bytes());
        bytes[64..68].copy_from_slice(b"PE\0\0");
        std::fs::write(&path, &bytes).unwrap();
        assert!(supports(&path));
        bytes[60..64].copy_from_slice(&u32::MAX.to_le_bytes());
        std::fs::write(&path, &bytes).unwrap();
        assert!(!supports(&path));
    }
    #[test]
    fn minimal_arm64_macho_has_read_only_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("native");
        let bytes = [0xfeedfacf_u32, 0x0100000c, 0, 2, 0, 0, 0, 0]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>();
        std::fs::write(&path, &bytes).unwrap();
        let preview = read_preview(&path).unwrap();
        assert!(preview.text.contains("MachO"));
        assert!(preview.text.contains("Aarch64"));
        assert!(preview.text.contains("64 bits"));
        assert!(preview.text.contains("Hex preview"));
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }
    #[test]
    fn malformed_binary_is_rejected_without_running_or_modifying_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("native");
        let bytes = b"\xcf\xfa\xed\xfe\0\0\0\0\0\0\0\0";
        std::fs::write(&path, bytes).unwrap();
        assert!(supports(&path));
        assert!(read_preview(&path).is_err());
        assert_eq!(std::fs::read(path).unwrap(), bytes);
        assert!(!supports(dir.path()));
    }
}
