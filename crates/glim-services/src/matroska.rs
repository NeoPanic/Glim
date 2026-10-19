//! Stream Matroska media; sample timing indexes live on disk and MP4 output is fragmented.
use crate::{limits as reader_limits, work::Cancellation};
use anyhow::{Context, Result, bail, ensure};
use matroska_demuxer::{Frame, MatroskaFile, TrackEntry, TrackType};
use mp4::{AacConfig, AvcConfig, MediaConfig, TrackConfig};
use std::{
    cell::Cell,
    fs::File,
    io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write},
    path::Path,
    rc::Rc,
    sync::Arc,
};
const MAX_FRAME: usize = reader_limits::MKV_FRAME_BYTES;
const TIMESCALE: u32 = 1_000_000;

pub(crate) fn has_header(path: &Path) -> Result<bool> {
    let mut file = File::open(path)?;
    let mut header = [0; 4];
    Ok(file.read(&mut header)? == 4 && header == [0x1a, 0x45, 0xdf, 0xa3])
}
struct Budget {
    bytes: Cell<u64>,
    operations: Cell<u64>,
}
impl Budget {
    fn new() -> Self {
        let budget = Self {
            bytes: Cell::new(0),
            operations: Cell::new(0),
        };
        budget.reset();
        budget
    }
    fn reset(&self) {
        self.bytes.set(reader_limits::MKV_PARSER_READ_BYTES);
        self.operations.set(reader_limits::MKV_PARSER_OPERATIONS);
    }
    fn operation(&self) -> std::io::Result<()> {
        self.operations.set(
            self.operations
                .get()
                .checked_sub(1)
                .ok_or_else(|| std::io::Error::other("MKV parser made no frame progress"))?,
        );
        Ok(())
    }
}
struct Mask {
    start: u64,
    header: Vec<u8>,
}
struct PreviewInput {
    input: BufReader<File>,
    position: u64,
    masks: Arc<Vec<Mask>>,
    budget: Rc<Budget>,
    cancel: Cancellation,
}
impl PreviewInput {
    fn open(
        path: &Path,
        masks: Arc<Vec<Mask>>,
        budget: Rc<Budget>,
        cancel: &Cancellation,
    ) -> Result<Self> {
        budget.reset();
        Ok(Self {
            input: BufReader::with_capacity(reader_limits::IO_BUFFER_BYTES, File::open(path)?),
            position: 0,
            masks,
            budget,
            cancel: cancel.clone(),
        })
    }
}
impl Read for PreviewInput {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        self.cancel.check().map_err(std::io::Error::other)?;
        self.budget.operation()?;
        if self.budget.bytes.get() == 0 {
            return Err(std::io::Error::other("MKV parser made no frame progress"));
        }
        let size = bytes.len().min(self.budget.bytes.get() as usize);
        let count = self.input.read(&mut bytes[..size])?;
        self.budget
            .bytes
            .set(self.budget.bytes.get() - count as u64);
        let end = self.position + count as u64;
        // Optional seek/chapters/tags metadata becomes EBML Void, so the demuxer
        // neither retains giant indexes nor follows their cyclic pointers.
        let first = self
            .masks
            .partition_point(|mask| mask.start + mask.header.len() as u64 <= self.position);
        for mask in &self.masks[first..] {
            if mask.start >= end {
                break;
            }
            let start = self.position.max(mask.start);
            let stop = end.min(mask.start + mask.header.len() as u64);
            bytes[(start - self.position) as usize..(stop - self.position) as usize]
                .copy_from_slice(
                    &mask.header[(start - mask.start) as usize..(stop - mask.start) as usize],
                );
        }
        self.position = end;
        Ok(count)
    }
}
impl Seek for PreviewInput {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        self.cancel.check().map_err(std::io::Error::other)?;
        self.budget.operation()?;
        self.position = self.input.seek(position)?;
        Ok(self.position)
    }
}
struct Track {
    number: u64,
    config: TrackConfig,
    nal_length: Option<usize>,
    fallback_duration: Option<u64>,
    duration: u64,
    decode_time: u64,
}

pub(crate) fn prepare(
    path: &Path,
    directory: &Path,
    cancel: &Cancellation,
) -> Result<tempfile::NamedTempFile> {
    let before = std::fs::metadata(path)?;
    ensure!(before.is_file(), "MKV source is not a file");
    let mut scan = BufReader::new(File::open(path)?);
    let mut masks = Vec::new();
    validate_elements(&mut scan, 0, before.len(), 0, &mut masks, cancel)?;
    let masks = Arc::new(masks);
    let budget = Rc::new(Budget::new());
    let mut input = MatroskaFile::open(PreviewInput::open(
        path,
        masks.clone(),
        budget.clone(),
        cancel,
    )?)?;
    let scale = input.info().timestamp_scale().get();
    let mut tracks = input
        .tracks()
        .iter()
        .filter(|track| {
            track.flag_enabled()
                && matches!(track.track_type(), TrackType::Video | TrackType::Audio)
        })
        .map(track_config)
        .collect::<Result<Vec<_>>>()?;
    ensure!(!tracks.is_empty(), "MKV has no playable tracks");
    let index_dir = tempfile::tempdir_in(directory)?;
    let mut index = rusqlite::Connection::open(index_dir.path().join("timing.sqlite"))?;
    index.execute_batch(&format!("PRAGMA temp_store=FILE; PRAGMA synchronous=OFF; PRAGMA cache_size=-{}; CREATE TABLE samples(track INTEGER, pts INTEGER, PRIMARY KEY(track,pts)); CREATE TABLE seek(track INTEGER, time INTEGER, offset INTEGER);", reader_limits::SQLITE_CACHE_KIB))?;
    let cancellation = cancel.clone();
    index.progress_handler(
        reader_limits::MKV_PARSER_OPERATIONS.min(i32::MAX as u64) as i32,
        Some(move || cancellation.check().is_err()),
    );
    let transaction = index.transaction()?;
    let mut frame = Frame::default();
    let mut origin = u64::MAX;
    {
        let mut insert = transaction.prepare("INSERT INTO samples VALUES (?1,?2)")?;
        while input.next_frame(&mut frame)? {
            budget.reset();
            cancel.check()?;
            ensure!(
                frame.data.len() <= MAX_FRAME,
                "MKV frame exceeds the working buffer budget"
            );
            let Some(track) = tracks.iter_mut().find(|track| track.number == frame.track) else {
                continue;
            };
            let pts = frame
                .timestamp
                .checked_mul(scale)
                .context("MKV timestamp overflow")?
                / 1000;
            insert
                .execute(rusqlite::params![track.number, pts])
                .context("MKV has duplicate/laced timestamps or an invalid timing range")?;
            origin = origin.min(pts);
            if let MediaConfig::AvcConfig(config) = &mut track.config.media_conf {
                for nal in nal_units(&frame.data, track.nal_length)? {
                    let nal = nal?;
                    match nal[0] & 31 {
                        7 if config.seq_param_set.is_empty() => config.seq_param_set = nal.to_vec(),
                        8 if config.pic_param_set.is_empty() => config.pic_param_set = nal.to_vec(),
                        _ => {}
                    }
                }
            }
        }
    }
    transaction.commit()?;
    ensure!(origin != u64::MAX, "MKV has no frames");
    for track in &mut tracks {
        cancel.check()?;
        if let MediaConfig::AvcConfig(config) = &track.config.media_conf {
            ensure!(
                config.seq_param_set.len() >= 4 && !config.pic_param_set.is_empty(),
                "MKV H.264 parameter sets are missing"
            );
            ensure!(
                config.seq_param_set.len() <= u16::MAX as usize
                    && config.pic_param_set.len() <= u16::MAX as usize,
                "Invalid H.264 parameter set length"
            );
        }
        let interval: Option<u64> = index.query_row("SELECT min(delta) FROM (SELECT pts-lag(pts) OVER (ORDER BY pts) AS delta FROM samples WHERE track=?1)", [track.number], |row| row.get(0))?;
        track.fallback_duration = track.fallback_duration.or(interval);
        let fallback = track
            .fallback_duration
            .context("MKV frame duration is missing")?;
        let last: Option<u64> = index.query_row(
            "SELECT max(pts) FROM samples WHERE track=?1",
            [track.number],
            |row| row.get(0),
        )?;
        track.duration = last
            .context("MKV has an empty track")?
            .checked_add(fallback)
            .context("MKV duration overflow")?
            - origin;
    }
    let mut output = tempfile::Builder::new()
        .prefix("glim-mkv-")
        .suffix(".mp4")
        .tempfile_in(directory)?;
    {
        let mut writer = BufWriter::new(output.as_file_mut());
        crate::fragmented_mp4::start(
            &mut writer,
            &tracks.iter().map(|t| t.config.clone()).collect::<Vec<_>>(),
            &tracks.iter().map(|t| t.duration).collect::<Vec<_>>(),
            TIMESCALE,
        )?;
        let mut position = writer.stream_position()?;
        let mut input =
            MatroskaFile::open(PreviewInput::open(path, masks, budget.clone(), cancel)?)?;
        let transaction = index.transaction()?;
        let mut following =
            transaction.prepare("SELECT min(pts) FROM samples WHERE track=?1 AND pts>?2")?;
        let mut seek = transaction.prepare("INSERT INTO seek VALUES (?1,?2,?3)")?;
        let mut sequence = 0u32;
        while input.next_frame(&mut frame)? {
            budget.reset();
            cancel.check()?;
            let Some((track_id, track)) = tracks
                .iter_mut()
                .enumerate()
                .find(|(_, t)| t.number == frame.track)
            else {
                continue;
            };
            let pts = frame
                .timestamp
                .checked_mul(scale)
                .context("MKV timestamp overflow")?
                / 1000;
            let next: Option<u64> =
                following.query_row(rusqlite::params![track.number, pts], |row| row.get(0))?;
            let duration = next.map_or(track.fallback_duration.unwrap(), |next| next - pts);
            let duration = u32::try_from(duration)
                .context("MKV sample duration cannot be represented in MP4")?;
            let mut sync = frame.is_keyframe.unwrap_or(false);
            let data = if matches!(track.config.media_conf, MediaConfig::AvcConfig(_)) {
                let mut data = Vec::with_capacity(frame.data.len());
                for nal in nal_units(&frame.data, track.nal_length)? {
                    let nal = nal?;
                    sync |= nal[0] & 31 == 5;
                    data.extend_from_slice(&u32::try_from(nal.len())?.to_be_bytes());
                    data.extend_from_slice(nal);
                }
                data
            } else {
                sync = true;
                frame.data.clone()
            };
            let offset = i64::try_from(pts - origin)? - i64::try_from(track.decode_time)?;
            sequence = sequence.wrapping_add(1);
            if sync {
                seek.execute(rusqlite::params![track_id + 1, track.decode_time, position])?;
            }
            position += crate::fragmented_mp4::sample(
                &mut writer,
                sequence,
                u32::try_from(track_id + 1)?,
                track.decode_time,
                duration,
                i32::try_from(offset)
                    .context("MKV composition offset cannot be represented in MP4")?,
                sync,
                &data,
            )?;
            track.decode_time += u64::from(duration);
        }
        drop(following);
        drop(seek);
        transaction.commit()?;
        write_seek_index(&mut writer, &index, tracks.len(), cancel)?;
        writer.flush()?;
    }
    let after = std::fs::metadata(path)?;
    ensure!(
        before.len() == after.len() && before.modified().ok() == after.modified().ok(),
        "MKV changed during preparation; reopen it"
    );
    Ok(output)
}
fn write_seek_index(
    out: &mut impl Write,
    index: &rusqlite::Connection,
    tracks: usize,
    cancel: &Cancellation,
) -> Result<()> {
    let mut counts = Vec::new();
    let mut length = 8u64 + 16;
    for track in 1..=tracks {
        let count: u32 =
            index.query_row("SELECT count(*) FROM seek WHERE track=?1", [track], |row| {
                row.get(0)
            })?;
        length = length
            .checked_add(24 + u64::from(count) * 19)
            .context("MP4 seek index overflow")?;
        counts.push(count);
    }
    let length = u32::try_from(length).context("MP4 seek index exceeds format range")?;
    out.write_all(&length.to_be_bytes())?;
    out.write_all(b"mfra")?;
    let mut select = index.prepare("SELECT time, offset FROM seek WHERE track=?1 ORDER BY time")?;
    for (track, count) in counts.into_iter().enumerate() {
        out.write_all(&(24 + count * 19).to_be_bytes())?;
        out.write_all(b"tfra")?;
        out.write_all(&[1, 0, 0, 0])?;
        out.write_all(&u32::try_from(track + 1)?.to_be_bytes())?;
        out.write_all(&[0; 4])?;
        out.write_all(&count.to_be_bytes())?;
        let mut rows = select.query([track + 1])?;
        while let Some(row) = rows.next()? {
            cancel.check()?;
            out.write_all(&row.get::<_, u64>(0)?.to_be_bytes())?;
            out.write_all(&row.get::<_, u64>(1)?.to_be_bytes())?;
            out.write_all(&[1, 1, 1])?;
        }
    }
    out.write_all(&16u32.to_be_bytes())?;
    out.write_all(b"mfro")?;
    out.write_all(&[0; 4])?;
    out.write_all(&length.to_be_bytes())?;
    Ok(())
}
fn track_config(track: &TrackEntry) -> Result<Track> {
    ensure!(
        track.content_encodings().is_none_or(|c| c.is_empty()),
        "Compressed/encrypted MKV tracks are unsupported"
    );
    ensure!(
        track.codec_delay().unwrap_or(0) == 0,
        "MKV codec delay is unsupported"
    );
    let mut nal_length = None;
    let media_conf = match track.codec_id() {
        "V_MPEG4/ISO/AVC" => {
            let video = track.video().context("MKV video dimensions are missing")?;
            let mut config = AvcConfig {
                width: video.pixel_width().get().try_into()?,
                height: video.pixel_height().get().try_into()?,
                seq_param_set: Vec::new(),
                pic_param_set: Vec::new(),
            };
            if let Some(private) = track.codec_private().filter(|p| !p.is_empty()) {
                ensure!(
                    private.len() >= 7 && private[0] == 1,
                    "Invalid MKV AVC configuration"
                );
                nal_length = Some(usize::from((private[4] & 3) + 1));
                let mut pos = 6;
                for kind in [7, 8] {
                    let count = if kind == 7 {
                        private[5] & 31
                    } else {
                        let n = *private.get(pos).context("Missing AVC PPS count")?;
                        pos += 1;
                        n
                    };
                    for _ in 0..count {
                        let len = private
                            .get(pos..pos + 2)
                            .context("Invalid AVC parameter length")?;
                        let len = u16::from_be_bytes([len[0], len[1]]) as usize;
                        pos += 2;
                        let nal = private
                            .get(pos..pos + len)
                            .context("Truncated AVC parameter set")?;
                        pos += len;
                        ensure!(
                            !nal.is_empty() && nal[0] & 31 == kind,
                            "Invalid AVC parameter set"
                        );
                        if kind == 7 && config.seq_param_set.is_empty() {
                            config.seq_param_set = nal.to_vec();
                        }
                        if kind == 8 && config.pic_param_set.is_empty() {
                            config.pic_param_set = nal.to_vec();
                        }
                    }
                }
            }
            MediaConfig::AvcConfig(config)
        }
        "A_AAC" => {
            let private = track
                .codec_private()
                .context("MKV AAC configuration is missing")?;
            ensure!(private.len() >= 2, "Invalid MKV AAC configuration");
            let profile = private[0] >> 3;
            let frequency = ((private[0] & 7) << 1) | (private[1] >> 7);
            let channels = (private[1] >> 3) & 15;
            ensure!(
                profile == 2 && frequency < 13 && channels > 0 && channels <= 7,
                "MKV audio preview requires AAC-LC with a standard sample rate/channel layout"
            );
            MediaConfig::AacConfig(AacConfig {
                bitrate: 0,
                profile: profile.try_into()?,
                freq_index: frequency.try_into()?,
                chan_conf: channels.try_into()?,
            })
        }
        codec => bail!(
            "MKV codec {codec} is not supported; preview supports H.264 video and AAC-LC audio"
        ),
    };
    let mut config = TrackConfig::from(media_conf);
    config.timescale = TIMESCALE;
    Ok(Track {
        number: track.track_number().get(),
        config,
        nal_length,
        fallback_duration: track.default_duration().map(|d| d.get() / 1000),
        duration: 0,
        decode_time: 0,
    })
}

struct Nals<'a> {
    data: &'a [u8],
    pos: usize,
    length: Option<usize>,
    failed: bool,
}
fn start_code(data: &[u8], start: usize) -> Option<(usize, usize)> {
    (start..data.len()).find_map(|pos| {
        if data[pos..].starts_with(&[0, 0, 0, 1]) {
            Some((pos, 4))
        } else if data[pos..].starts_with(&[0, 0, 1]) {
            Some((pos, 3))
        } else {
            None
        }
    })
}
fn nal_units(data: &[u8], length: Option<usize>) -> Result<Nals<'_>> {
    let pos = if length.is_some() {
        0
    } else {
        let (pos, n) = start_code(data, 0).context("Missing Annex-B start code")?;
        pos + n
    };
    Ok(Nals {
        data,
        pos,
        length,
        failed: false,
    })
}
impl<'a> Iterator for Nals<'a> {
    type Item = Result<&'a [u8]>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.failed || self.pos >= self.data.len() {
            return None;
        }
        let result = (|| {
            if let Some(length) = self.length {
                let size = self
                    .data
                    .get(self.pos..self.pos + length)
                    .context("Truncated NAL length")?
                    .iter()
                    .fold(0usize, |n, b| (n << 8) | usize::from(*b));
                self.pos += length;
                ensure!(size > 0, "Empty NAL unit");
                let nal = self
                    .data
                    .get(self.pos..self.pos + size)
                    .context("Truncated NAL unit")?;
                self.pos += size;
                Ok(nal)
            } else {
                let next = start_code(self.data, self.pos);
                let end = next.map_or(self.data.len(), |n| n.0);
                let mut nal = &self.data[self.pos..end];
                self.pos = next.map_or(self.data.len(), |n| n.0 + n.1);
                while nal.last() == Some(&0) {
                    nal = &nal[..nal.len() - 1];
                }
                ensure!(!nal.is_empty(), "Empty Annex-B NAL unit");
                Ok(nal)
            }
        })();
        self.failed = result.is_err();
        Some(result)
    }
}
fn vint(input: &mut impl Read, id: bool) -> Result<(u64, bool, u64)> {
    let mut first = [0];
    input.read_exact(&mut first)?;
    let n = first[0].leading_zeros() as usize + 1;
    ensure!(n <= if id { 4 } else { 8 }, "Invalid EBML integer");
    let mut value = u64::from(first[0]);
    for _ in 1..n {
        let mut byte = [0];
        input.read_exact(&mut byte)?;
        value = (value << 8) | u64::from(byte[0]);
    }
    let mask = (1u64 << (7 * n)) - 1;
    Ok((
        if id { value } else { value & mask },
        !id && value & mask == mask,
        n as u64,
    ))
}
fn validate_elements(
    input: &mut (impl Read + Seek),
    mut pos: u64,
    end: u64,
    depth: usize,
    masks: &mut Vec<Mask>,
    cancel: &Cancellation,
) -> Result<()> {
    ensure!(
        depth <= reader_limits::MKV_NESTING,
        "MKV metadata nesting exceeds the parser stack budget"
    );
    while pos < end {
        cancel.check()?;
        input.seek(SeekFrom::Start(pos))?;
        let header_start = pos;
        let (id, _, a) = vint(input, true)?;
        let (size, unknown, b) = vint(input, false)?;
        pos += a + b;
        ensure!(pos <= end, "Truncated MKV header");
        if unknown {
            ensure!(
                matches!(id, 0x18538067 | 0x1f43b675),
                "Unsupported unknown-size MKV element"
            );
            continue;
        }
        let next = pos
            .checked_add(size)
            .filter(|n| *n <= end)
            .context("MKV element exceeds its container")?;
        if matches!(
            id,
            0x114d9b74 | 0x1c53bb6b | 0x1043a770 | 0x1254c367 | 0x1941a469
        ) {
            let span = next - header_start;
            let mut header = vec![0xec];
            if span >= 9 {
                let payload = span - 9;
                ensure!(payload < (1u64 << 56) - 1, "Invalid MKV metadata extent");
                header.extend_from_slice(&((1u64 << 56) | payload).to_be_bytes());
            } else {
                ensure!(span >= 2, "Invalid MKV metadata extent");
                header.push(0x80 | (span - 2) as u8);
            }
            masks.push(Mask {
                start: header_start,
                header,
            });
        } else if matches!(
            id,
            0x1a45dfa3
                | 0x18538067
                | 0x1549a966
                | 0x1654ae6b
                | 0xae
                | 0xe0
                | 0xe1
                | 0x1f43b675
                | 0xa0
                | 0x6d80
                | 0x6240
                | 0x5034
                | 0x5035
                | 0x55b0
                | 0x55d0
                | 0x7670
        ) {
            validate_elements(input, pos, next, depth + 1, masks, cancel)?;
        } else if id != 0xec {
            ensure!(
                size <= MAX_FRAME as u64,
                "MKV element exceeds the active frame/metadata buffer budget"
            );
        }
        pos = next;
    }
    Ok(())
}
