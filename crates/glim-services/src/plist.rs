//! Decode Apple binary property lists to private, read-only XML preview pages.
use crate::{paged::TextPage, work::Cancellation};
use anyhow::{Result, ensure};
use std::{
    io::Read,
    path::Path,
    process::{Command, Stdio},
};

pub fn is_binary(path: &Path) -> bool {
    if !path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("plist"))
    {
        return false;
    }
    let mut header = [0; 8];
    std::fs::File::open(path)
        .and_then(|mut file| file.read_exact(&mut header))
        .is_ok()
        && &header == b"bplist00"
}
pub fn read(path: &Path, cancel: &Cancellation) -> Result<TextPage> {
    cancel.check()?;
    let output = tempfile::NamedTempFile::new()?;
    let status = crate::work::run(
        Command::new("/usr/bin/plutil")
            .args(["-convert", "xml1", "-o"])
            .arg(output.path())
            .arg("--")
            .arg(path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null()),
        cancel,
    )?;
    ensure!(status.success(), "Cannot decode binary property list");
    cancel.check()?;
    crate::paged::generated(path, output)
}
