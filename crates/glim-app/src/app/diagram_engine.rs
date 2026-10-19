//! Private renderer subprocess entry point, before GPUI initialization. The
//! system WebView exists only in this process; Markdown HTML never enters it.
use glim_services::limits as reader_limits;

include!(concat!(env!("OUT_DIR"), "/diagram_assets.rs"));

pub fn run() -> anyhow::Result<()> {
    use anyhow::{Context, ensure};
    use std::{
        borrow::Cow,
        path::PathBuf,
        time::{Duration, Instant},
    };
    use tao::{
        event::Event,
        event_loop::{ControlFlow, EventLoopBuilder},
        platform::run_return::EventLoopExtRunReturn,
        window::WindowBuilder,
    };
    use wry::{WebViewBuilder, http::Response};

    let input = PathBuf::from(
        std::env::args_os()
            .nth(2)
            .context("Missing renderer input")?,
    );
    let output = PathBuf::from(
        std::env::args_os()
            .nth(3)
            .context("Missing renderer output")?,
    );
    ensure!(
        std::fs::metadata(&input)?.len() <= reader_limits::DIAGRAM_REQUEST_BYTES,
        "Renderer input too large"
    );
    let request = std::fs::read(input)?;
    let values: Vec<serde_json::Value> = serde_json::from_slice(&request)?;
    ensure!(
        values.len() <= glim_services::markdown_preview::MAX_DIAGRAMS,
        "Too many diagrams"
    );
    for value in &values {
        ensure!(
            value
                .get("source")
                .and_then(|v| v.as_str())
                .is_some_and(|s| s.len() <= glim_services::markdown_preview::MAX_SOURCE),
            "Invalid diagram source"
        );
    }
    let count = values.len();
    let initialization = format!(
        "window.GLIM_LIMITS = {}; window.GLIM_REQUESTS = {}; window.addEventListener('error', function(event) {{ window.ipc.postMessage(JSON.stringify({{ fatal: String(event.message || 'Cannot load ' + (event.target.src || 'renderer asset')).slice(0, 2000) }})); }}, true); window.addEventListener('unhandledrejection', function(event) {{ window.ipc.postMessage(JSON.stringify({{ fatal: String(event.reason).slice(0, 2000) }})); }});",
        glim_services::limits::JSON,
        serde_json::to_string(&values)?
    );
    let mut results = vec![serde_json::Value::Null; count];
    let mut event_loop = EventLoopBuilder::<serde_json::Value>::with_user_event().build();

    {
        use tao::platform::macos::{
            ActivationPolicy, EventLoopExtMacOS, EventLoopWindowTargetExtMacOS,
        };
        event_loop.set_activation_policy(ActivationPolicy::Prohibited);
        event_loop.set_activation_policy_at_runtime(ActivationPolicy::Prohibited);
        event_loop.set_dock_visibility(false);
        event_loop.set_activate_ignoring_other_apps(false);
    }
    let proxy = event_loop.create_proxy();
    let window = WindowBuilder::new()
        .with_visible(false)
        .with_inner_size(tao::dpi::LogicalSize::new(1200.0, 900.0))
        .build(&event_loop)?;
    let _webview = WebViewBuilder::new()
        .with_incognito(true)
        .with_devtools(false)
        .with_initialization_script(initialization)
        .with_custom_protocol("glim".into(), |_, incoming| {
            let route = if incoming.uri().path() == "/" {
                "/index.html"
            } else {
                incoming.uri().path()
            };
            let Some((_, mime, bytes)) = DIAGRAM_ASSETS.iter().find(|(path, _, _)| *path == route)
            else {
                return Response::builder()
                    .status(404)
                    .body(Cow::Borrowed(&b""[..]))
                    .unwrap();
            };
            let body: Cow<'static, [u8]> = Cow::Borrowed(bytes);
            Response::builder()
                .header("Content-Type", *mime)
                .header("Access-Control-Allow-Origin", "*")
                .body(body)
                .unwrap()
        })
        .with_navigation_handler(|url| {
            matches!(
                url.as_str(),
                "glim://localhost/" | "http://glim.localhost/" | "https://glim.localhost/"
            )
        })
        .with_new_window_req_handler(|_, _| wry::NewWindowResponse::Deny)
        .with_download_started_handler(|_, _| false)
        .with_ipc_handler(move |request| {
            if request.body().len() <= reader_limits::DIAGRAM_MESSAGE_BYTES
                && let Ok(value) = serde_json::from_str(request.body())
            {
                let _ = proxy.send_event(value);
            }
        })
        .with_url("glim://localhost/")
        .build(&window)?;
    let started = Instant::now();
    let mut bytes_used = 0;
    let mut write_error = None;
    let mut timed_out = false;
    event_loop.run_return(|event, _, control_flow| {
        *control_flow = ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(100));
        if let Event::UserEvent(value) = event {
            if let Some(error) = value.get("fatal").and_then(|v| v.as_str()) {
                for slot in &mut results {
                    if slot.is_null() {
                        *slot = serde_json::json!({ "error": format!("离线图表引擎加载失败：{error}") });
                    }
                }
                if let Err(error) = std::fs::write(&output, serde_json::to_vec(&results).unwrap_or_default()) {
                    write_error = Some(error.into());
                }
                *control_flow = ControlFlow::Exit;
            } else if value.get("done").and_then(|v| v.as_bool()) == Some(true) {
                *control_flow = ControlFlow::Exit;
            } else if let Some(index) = value.get("index").and_then(|v| v.as_u64())
                && let Some(slot) = results.get_mut(index as usize)
                && slot.is_null()
            {
                bytes_used += value
                    .get("svg")
                    .and_then(|v| v.as_str())
                    .map_or(0, str::len);
                *slot = if bytes_used <= reader_limits::DIAGRAM_AGGREGATE_ENCODED_BYTES {
                    value
                } else {
                    serde_json::json!({ "error": "图表输出超过 64 MiB" })
                };
                // Preserve completed blocks if a later engine stalls or the
                // parent kills this child at its document deadline.
                if let Err(error) = serde_json::to_vec(slot)
                    .map_err(anyhow::Error::from)
                    .and_then(|bytes| {
                        ensure!(
                            bytes.len() <= reader_limits::DIAGRAM_IPC_BYTES as usize,
                            "Renderer IPC budget exceeded"
                        );
                        let destination = output.with_extension(format!("{index}.json"));
                        let temporary = destination.with_extension("tmp");
                        std::fs::write(&temporary, bytes)?;
                        std::fs::rename(temporary, destination)?;
                        Ok(())
                    })
                {
                    write_error = Some(error);
                    *control_flow = ControlFlow::Exit;
                }
            }
        }
        if started.elapsed() > Duration::from_secs(10 + count as u64 * 8) {
            timed_out = true;
            *control_flow = ControlFlow::Exit;
        }
    });
    if let Some(error) = write_error {
        return Err(error);
    }
    for slot in &mut results {
        if slot.is_null() {
            *slot = serde_json::json!({ "error": if timed_out {
                "整页图表渲染达到等待上限，此块尚未完成"
            } else {
                "离线渲染进程结束，此块未返回结果"
            }});
        }
    }
    std::fs::write(&output, serde_json::to_vec(&results)?)?;
    Ok(())
}
