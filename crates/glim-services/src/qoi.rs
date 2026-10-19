//! QOI-to-PNG conversion uses fixed output chunks and PNG row buffers.
use anyhow::{Result, ensure};
use std::{
    fs::File,
    io::{BufReader, Read, Write},
    path::Path,
};
pub(crate) fn prepare(
    path: &Path,
    cancel: &crate::work::Cancellation,
) -> Result<tempfile::NamedTempFile> {
    prepare_with_size(path, cancel).map(|(file, _)| file)
}
pub(crate) fn prepare_with_size(
    path: &Path,
    cancel: &crate::work::Cancellation,
) -> Result<(tempfile::NamedTempFile, (u32, u32))> {
    let mut input = BufReader::with_capacity(crate::limits::IO_BUFFER_BYTES, File::open(path)?);
    let before = input.get_ref().metadata()?;
    let mut header = [0; 14];
    input.read_exact(&mut header)?;
    ensure!(&header[..4] == b"qoif", "Invalid QOI signature");
    let width = u32::from_be_bytes(header[4..8].try_into()?);
    let height = u32::from_be_bytes(header[8..12].try_into()?);
    ensure!(
        width > 0 && height > 0 && matches!(header[12], 3 | 4) && header[13] <= 1,
        "Invalid QOI header"
    );
    let edge = ((crate::limits::IMAGE_DECODED_BYTES / 4) as f64)
        .sqrt()
        .floor()
        .max(1.) as u64;
    let longest = u64::from(width.max(height));
    let edge = edge.min(longest);
    let output_width = (u64::from(width) * edge / longest).max(1) as u32;
    let output_height = (u64::from(height) * edge / longest).max(1) as u32;
    let mut file = tempfile::Builder::new().suffix(".png").tempfile()?;
    {
        let mut encoder = png::Encoder::new(file.as_file_mut(), output_width, output_height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        if header[13] == 0 {
            encoder.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
        } else {
            encoder.set_source_gamma(png::ScaledFloat::new(1.));
        }
        let mut writer = encoder.write_header()?;
        let mut stream = writer.stream_writer_with_size(crate::limits::IO_BUFFER_BYTES)?;
        let mut output = Vec::with_capacity(crate::limits::IO_BUFFER_BYTES);
        let mut previous = [0u8, 0, 0, 255];
        let mut index = [[0u8; 4]; 64];
        let mut run = 0;
        let (mut x, mut y) = (0_u32, 0_u32);
        let sample = |x: u32, y: u32| {
            let sx = (u64::from(x) * 2 + 1) * u64::from(width) / (u64::from(output_width) * 2);
            let sy = (u64::from(y) * 2 + 1) * u64::from(height) / (u64::from(output_height) * 2);
            sy * u64::from(width) + sx
        };
        let mut next_pixel = sample(x, y);
        for pixel in 0..u64::from(width) * u64::from(height) {
            if pixel % crate::limits::IO_BUFFER_BYTES as u64 == 0 {
                cancel.check()?;
            }
            if run > 0 {
                run -= 1;
            } else {
                let mut byte = [0];
                input.read_exact(&mut byte)?;
                match byte[0] {
                    0xfe => input.read_exact(&mut previous[..3])?,
                    0xff => input.read_exact(&mut previous)?,
                    value => match value >> 6 {
                        0 => previous = index[(value & 63) as usize],
                        1 => {
                            previous[0] =
                                previous[0].wrapping_add(((value >> 4) & 3).wrapping_sub(2));
                            previous[1] =
                                previous[1].wrapping_add(((value >> 2) & 3).wrapping_sub(2));
                            previous[2] = previous[2].wrapping_add((value & 3).wrapping_sub(2));
                        }
                        2 => {
                            input.read_exact(&mut byte)?;
                            let green = (value & 63).wrapping_sub(32);
                            previous[0] = previous[0]
                                .wrapping_add(green)
                                .wrapping_add((byte[0] >> 4).wrapping_sub(8));
                            previous[1] = previous[1].wrapping_add(green);
                            previous[2] = previous[2]
                                .wrapping_add(green)
                                .wrapping_add((byte[0] & 15).wrapping_sub(8));
                        }
                        _ => run = value & 63,
                    },
                }
                let hash = (u32::from(previous[0]) * 3
                    + u32::from(previous[1]) * 5
                    + u32::from(previous[2]) * 7
                    + u32::from(previous[3]) * 11)
                    % 64;
                index[hash as usize] = previous;
            }
            if pixel == next_pixel {
                output.extend_from_slice(&previous);
                x += 1;
                if x == output_width {
                    x = 0;
                    y += 1;
                }
                next_pixel = if y < output_height {
                    sample(x, y)
                } else {
                    u64::MAX
                };
            }
            if output.len() >= crate::limits::IO_BUFFER_BYTES {
                cancel.check()?;
                stream.write_all(&output)?;
                output.clear();
            }
        }
        ensure!(run == 0, "QOI run exceeds image dimensions");
        let mut end = [0; 8];
        input.read_exact(&mut end)?;
        ensure!(end == [0, 0, 0, 0, 0, 0, 0, 1], "Invalid QOI end marker");
        cancel.check()?;
        stream.write_all(&output)?;
        stream.finish()?;
        writer.finish()?;
    }
    let after = std::fs::metadata(path)?;
    ensure!(
        before.len() == after.len() && before.modified().ok() == after.modified().ok(),
        "Image changed during preparation; reopen it"
    );
    Ok((file, (width, height)))
}
