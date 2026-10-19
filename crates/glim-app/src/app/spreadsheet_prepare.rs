//! Cancellable workbook-page subprocess entry; runs before GUI startup.
pub fn run() -> anyhow::Result<()> {
    use anyhow::{Context, ensure};
    let mut args = std::env::args_os().skip(2);
    let cache = std::path::PathBuf::from(args.next().context("Missing workbook cache")?);
    let sheet = args
        .next()
        .context("Missing worksheet")?
        .to_str()
        .context("Invalid worksheet")?
        .parse()?;
    let row = args
        .next()
        .context("Missing row offset")?
        .to_str()
        .context("Invalid row offset")?
        .parse()?;
    let column = args
        .next()
        .context("Missing column offset")?
        .to_str()
        .context("Invalid column offset")?
        .parse()?;
    let output = std::path::PathBuf::from(args.next().context("Missing preview destination")?);
    ensure!(args.next().is_none(), "Unexpected workbook arguments");
    glim_services::spreadsheet::render_page(
        &cache,
        sheet,
        row,
        column,
        &output,
        &glim_services::work::Cancellation::default(),
    )
}
