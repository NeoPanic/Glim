mod app;
mod views;

fn main() -> anyhow::Result<()> {
    if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("--render-markdown")) {
        return app::diagram_engine::run();
    }

    if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("--prepare-matroska")) {
        return app::media_prepare::run();
    }
    if std::env::args_os().nth(1).as_deref()
        == Some(std::ffi::OsStr::new("--prepare-spreadsheet-page"))
    {
        return app::spreadsheet_prepare::run();
    }
    app::run()
}
