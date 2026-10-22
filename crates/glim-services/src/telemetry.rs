//! Optional, anonymous Aptabase startup reporting. All failures are discarded.
use crate::work::Cancellation;
use anyhow::{Result, ensure};
use std::{
    io::Read,
    process::{Command, Stdio},
};

const CONFIG_URL: &str =
    "https://raw.githubusercontent.com/glimapp/glim/main/config/telemetry.json";

#[derive(serde::Deserialize)]
struct Config {
    host: String,
    app_key: String,
}
impl Config {
    fn valid(&self) -> bool {
        let Ok(url) = url::Url::parse(&self.host) else {
            return false;
        };
        matches!(url.scheme(), "http" | "https")
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none()
            && self.app_key.starts_with("A-SH-")
            && self.app_key.len() > 5
            && self.app_key.len() < 128
            && self
                .app_key
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-')
    }
}
fn curl() -> Command {
    let mut command = Command::new("/usr/bin/curl");
    command
        .args([
            "--disable",
            "--silent",
            "--fail",
            "--connect-timeout",
            "2",
            "--max-time",
            "5",
            "--proto",
            "=http,https",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}
fn remote_config(cancel: &Cancellation) -> Result<Config> {
    let file = tempfile::NamedTempFile::new()?;
    let status = crate::work::run(
        curl()
            .args(["--max-filesize", "16384", "--url", CONFIG_URL])
            .stdout(Stdio::from(file.reopen()?)),
        cancel,
    )?;
    ensure!(status.success(), "Unavailable configuration");
    let config: Config = serde_json::from_reader(file.reopen()?.take(16385))?;
    ensure!(config.valid(), "Invalid configuration");
    Ok(config)
}

pub fn report_startup(cancel: &Cancellation) {
    let _ = send_startup(cancel);
}
fn send_startup(cancel: &Cancellation) -> Result<()> {
    cancel.check()?;
    let config = remote_config(cancel).unwrap_or_else(|_| Config {
        host: option_env!("GLIM_TELEMETRY_HOST")
            .unwrap_or("https://glim.wansu.tech")
            .into(),
        app_key: option_env!("GLIM_TELEMETRY_APP_KEY").unwrap_or("").into(),
    });
    ensure!(config.valid(), "Telemetry unconfigured");
    cancel.check()?;
    let now = chrono::Utc::now();
    let payload = serde_json::json!({
        "timestamp": now.to_rfc3339(),
        // Aptabase decodes numeric IDs as epoch seconds followed by 8 digits.
        "sessionId": format!("{}{:08}", now.timestamp(), now.timestamp_subsec_nanos() % 100_000_000),
        "eventName": "app_started",
        "systemProps": {
            "osName": "macOS", "architecture": std::env::consts::ARCH,
            "appVersion": env!("CARGO_PKG_VERSION"),
            "isDebug": cfg!(debug_assertions), "sdkVersion": "glim@1"
        },
        "props": {}
    });
    let body = tempfile::NamedTempFile::new()?;
    serde_json::to_writer(body.reopen()?, &payload)?;
    let _ = crate::work::run(
        curl().args([
            "--request",
            "POST",
            "--header",
            "Content-Type: application/json",
            "--header",
            &format!("App-Key: {}", config.app_key),
            "--data-binary",
            &format!("@{}", body.path().display()),
            "--url",
            &format!("{}/api/v0/event", config.host.trim_end_matches('/')),
        ]),
        cancel,
    )?;
    Ok(())
}
