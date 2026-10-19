//! Stream header metadata to a pageable report without reading tensor payloads.
use crate::{paged::TextPage, report::Report, work::Cancellation};
use anyhow::{Context, Result, ensure};
use serde::de::{DeserializeSeed, Error, MapAccess, Visitor};
use std::{
    cell::Cell,
    fmt::{self, Write as _},
    fs::File,
    io::{BufReader, Read},
    path::Path,
    rc::Rc,
};

pub fn read_preview(path: &Path) -> Result<TextPage> {
    read_preview_cancellable(path, &Cancellation::default())
}
pub fn read_preview_cancellable(path: &Path, cancel: &Cancellation) -> Result<TextPage> {
    match read_report(path, cancel) {
        Err(error)
            if error
                .to_string()
                .contains("Tensor metadata record exceeds working budget") =>
        {
            let mut file = File::open(path)?;
            let mut length = [0; 8];
            file.read_exact(&mut length)?;
            let length = u64::from_le_bytes(length);
            ensure!(
                length <= file.metadata()?.len().saturating_sub(8),
                "Truncated safetensors header"
            );
            crate::paged::compressed(
                path,
                crate::streamed::Compression::Slice { offset: 8, length },
                cancel,
            )
        }
        result => result,
    }
}
fn read_report(path: &Path, cancel: &Cancellation) -> Result<TextPage> {
    let mut file = File::open(path)?;
    let size = file.metadata()?.len();
    let mut prefix = [0; 8];
    file.read_exact(&mut prefix)
        .context("Missing safetensors header length")?;
    let length = u64::from_le_bytes(prefix);
    ensure!(
        length <= size.saturating_sub(8),
        "Truncated safetensors header"
    );
    let payload = size - 8 - length;
    let mut text = Report::new()?;
    writeln!(
        text,
        "SAFETENSORS\n\nFile size       {size} bytes\nHeader          {length} bytes\nTensor payload  {payload} bytes\n\nTENSORS AND METADATA"
    )?;
    let budget = Rc::new(Cell::new(crate::limits::IO_RECORD_BYTES));
    let input = MetadataInput {
        inner: crate::work::Reader {
            inner: file.take(length),
            cancel: cancel.clone(),
        },
        budget: budget.clone(),
    };
    let index = tempfile::NamedTempFile::new()?;
    let types = rusqlite::Connection::open(index.path())?;
    types.pragma_update(None, "cache_size", -crate::limits::SQLITE_CACHE_KIB)?;
    types.execute_batch(
        "PRAGMA journal_mode=OFF; CREATE TABLE types (name TEXT PRIMARY KEY, count INTEGER); BEGIN",
    )?;
    let mut decoder = serde_json::Deserializer::from_reader(BufReader::new(input));
    let mut header = Header {
        text: &mut text,
        payload,
        count: 0,
        parameters: 0,
        types: &types,
        budget: budget.clone(),
        cancel,
    };
    (&mut header).deserialize(&mut decoder)?;
    decoder.end()?;
    let count = header.count;
    let parameters = header.parameters;
    drop(header);
    writeln!(
        text,
        "\nTensors         {count}\nParameters      {parameters}\n\nPRECISION"
    )?;
    types.execute_batch("COMMIT")?;
    let mut precision = types.prepare("SELECT name,count FROM types ORDER BY name")?;
    let mut rows = precision.query([])?;
    while let Some(row) = rows.next()? {
        cancel.check()?;
        let dtype: String = row.get(0)?;
        let count: u64 = row.get(1)?;
        writeln!(text, "{dtype:<12} {count} tensors")?;
    }
    text.push_str("\nHeader inspection only; weights were not loaded. Offsets are range-checked; this is not full model validation.\n");
    text.finish(path)
}

struct Header<'a> {
    text: &'a mut Report,
    payload: u64,
    count: u64,
    parameters: u128,
    types: &'a rusqlite::Connection,
    budget: Rc<Cell<usize>>,
    cancel: &'a Cancellation,
}
impl<'de> DeserializeSeed<'de> for &mut Header<'_> {
    type Value = ();
    fn deserialize<D: serde::Deserializer<'de>>(
        self,
        decoder: D,
    ) -> std::result::Result<(), D::Error> {
        decoder.deserialize_map(self)
    }
}
impl<'de> Visitor<'de> for &mut Header<'_> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a safetensors header object")
    }
    fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> std::result::Result<(), M::Error> {
        loop {
            self.budget.set(crate::limits::IO_RECORD_BYTES);
            let Some(name) = map.next_key::<String>()? else {
                break;
            };
            self.cancel.check().map_err(M::Error::custom)?;
            if name == "__metadata__" {
                map.next_value_seed(Metadata(self.text, self.budget.clone()))?;
                continue;
            }
            #[derive(serde::Deserialize)]
            struct Tensor {
                dtype: String,
                shape: Vec<u64>,
                data_offsets: [u64; 2],
            }
            let tensor: Tensor = map.next_value()?;
            let [begin, end] = tensor.data_offsets;
            if begin > end || end > self.payload {
                return Err(M::Error::custom("Tensor extends beyond the file"));
            }
            let elements = tensor
                .shape
                .iter()
                .try_fold(1u128, |n, dim| n.checked_mul(u128::from(*dim)))
                .ok_or_else(|| M::Error::custom("Tensor shape overflow"))?;
            self.parameters = self
                .parameters
                .checked_add(elements)
                .ok_or_else(|| M::Error::custom("Parameter count overflow"))?;
            self.count += 1;
            self.types
                .execute(
                    "INSERT INTO types VALUES (?1,1) ON CONFLICT(name) DO UPDATE SET count=count+1",
                    [&tensor.dtype],
                )
                .map_err(M::Error::custom)?;
            write!(
                self.text,
                "{}  ·  {}  ·  ",
                name.escape_debug(),
                tensor.dtype.escape_debug()
            )
            .map_err(M::Error::custom)?;
            if tensor.shape.is_empty() {
                self.text.push_str("[] (scalar)");
            }
            for (index, dimension) in tensor.shape.iter().enumerate() {
                if index > 0 {
                    self.text.push_str(" × ");
                }
                write!(self.text, "{dimension}").map_err(M::Error::custom)?;
            }
            writeln!(self.text, "  ·  {} bytes", end - begin).map_err(M::Error::custom)?;
        }
        Ok(())
    }
}
struct Metadata<'a>(&'a mut Report, Rc<Cell<usize>>);
impl<'de> DeserializeSeed<'de> for Metadata<'_> {
    type Value = ();
    fn deserialize<D: serde::Deserializer<'de>>(
        self,
        decoder: D,
    ) -> std::result::Result<(), D::Error> {
        decoder.deserialize_map(self)
    }
}
impl<'de> Visitor<'de> for Metadata<'_> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("string metadata")
    }
    fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> std::result::Result<(), M::Error> {
        self.0.push_str("\nMETADATA\n");
        loop {
            self.1.set(crate::limits::IO_RECORD_BYTES);
            let Some((key, value)) = map.next_entry::<String, String>()? else {
                break;
            };
            writeln!(self.0, "{}: {}", key.escape_debug(), value.escape_debug())
                .map_err(M::Error::custom)?;
        }
        Ok(())
    }
}

struct MetadataInput<R> {
    inner: R,
    budget: Rc<Cell<usize>>,
}
impl<R: Read> Read for MetadataInput<R> {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        if bytes.is_empty() {
            return Ok(0);
        }
        let size = bytes.len().min(self.budget.get());
        if size == 0 {
            return Err(std::io::Error::other(
                "Tensor metadata record exceeds working budget",
            ));
        }
        let read = self.inner.read(&mut bytes[..size])?;
        self.budget.set(self.budget.get() - read);
        Ok(read)
    }
}
