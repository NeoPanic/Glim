use anyhow::Result;
use glim_services::{paged, safetensors};
use std::fs;

#[test]
fn paging_completes_utf8_and_bounds_full_reads() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("large.json");
    let text = "a".repeat(paged::PAGE_BYTES as usize - 1) + "界tail";
    fs::write(&path, &text)?;
    let first = paged::read_page(&path, 0, false)?;
    let mut joined = first.text;
    let mut offset = first.end;
    while offset < text.len() as u64 {
        let next = paged::read_page(&path, offset, false)?;
        assert!(next.end > offset);
        joined.push_str(&next.text);
        offset = next.end;
    }
    assert_eq!(joined, text);
    fs::File::create(&path)?.set_len(paged::FULL_BYTES + 1)?;
    assert!(paged::read_page(&path, 0, true).is_err());
    fs::write(&path, [0xff])?;
    assert!(paged::read_page(&path, 0, false).is_err());
    Ok(())
}
#[test]
fn safetensors_metadata_without_loading_large_payload() -> Result<()> {
    use std::io::Write;
    let mut file = tempfile::NamedTempFile::new()?;
    let header = br#"{"weight":{"dtype":"F32","shape":[2],"data_offsets":[0,8]},"__metadata__":{"format":"pt"}}"#;
    file.write_all(&(header.len() as u64).to_le_bytes())?;
    file.write_all(header)?;
    file.as_file().set_len(2 * 1024 * 1024 * 1024)?;
    let preview = safetensors::read_preview(file.path())?;
    assert!(preview.text.contains("Parameters      2"));
    assert!(preview.text.contains("format: pt"));
    assert!(preview.text.len() < 4096);
    file.as_file().set_len(8 + header.len() as u64 + 4)?;
    assert!(safetensors::read_preview(file.path()).is_err());
    Ok(())
}
