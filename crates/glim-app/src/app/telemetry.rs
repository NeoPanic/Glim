use gpui_kit::*;

#[derive(Default)]
pub(crate) struct Telemetry {
    pub enabled: bool,
    report: Option<Task<()>>,
    cancel: Option<glim_services::work::CancelOnDrop>,
    save: Option<Task<()>>,
    started: Option<std::time::Instant>,
    resources: Option<Task<()>>,
}
impl Global for Telemetry {}

pub(crate) fn initialize(cx: &mut App) {
    let enabled = glim_services::preferences::telemetry_enabled();
    crate::views::set_native_telemetry_enabled(enabled);
    cx.set_global(Telemetry {
        enabled,
        ..Default::default()
    });
    if enabled {
        start("app_started", cx);
    }
}
fn start(event: &'static str, cx: &mut App) {
    let worker = glim_services::telemetry::start(env!("GLIM_BUILD_NUMBER"));
    glim_services::telemetry::track(
        event,
        [(
            "updater_enabled",
            glim_services::telemetry::flag(cfg!(feature = "macos-updater")),
        )],
    );
    let (cancel, guard) = glim_services::work::Cancellation::task();
    let task = cx.background_executor().spawn(async move {
        worker.run(&cancel);
    });
    let state = cx.global_mut::<Telemetry>();
    state.started = Some(std::time::Instant::now());
    state.cancel = Some(guard);
    state.report = Some(task);
    start_resource_sampling(cx);
}
pub(crate) fn set_enabled(enabled: bool, cx: &mut App) {
    let state = cx.global_mut::<Telemetry>();
    if state.enabled == enabled {
        return;
    }
    state.enabled = enabled;
    crate::views::set_native_telemetry_enabled(enabled);
    glim_services::telemetry::stop(false);
    state.cancel.take();
    state.report.take();
    state.resources.take();
    let previous = state.save.take();
    let task = cx.spawn(async move |cx| {
        if let Some(previous) = previous {
            previous.await;
        }
        cx.background_executor()
            .spawn(async move {
                let _ = glim_services::preferences::save_telemetry_enabled(enabled);
            })
            .await;
    });
    cx.global_mut::<Telemetry>().save = Some(task);
    if enabled {
        start("telemetry_enabled", cx);
    }
    for (view, _) in super::OpenWorkspaces::windows(cx) {
        let _ = view.update(cx, |view, _| view.reset_usage());
    }
    cx.refresh_windows();
}
pub(crate) fn shutdown(cx: &mut App) -> Option<Task<()>> {
    let state = cx.global_mut::<Telemetry>();
    if state.enabled {
        glim_services::telemetry::track(
            "app_exited",
            [(
                "session_duration_s",
                glim_services::telemetry::number(
                    state.started.map_or(0, |start| start.elapsed().as_secs()),
                ),
            )],
        );
    }
    glim_services::telemetry::stop(true);
    state.resources.take();
    let report = state.report.take();
    let cancel = state.cancel.take();
    let save = state.save.take();
    if report.is_none() && save.is_none() {
        return None;
    }
    Some(cx.spawn(async move |cx| {
        // Normal quit gets a short flush window. Opt-out cancels immediately.
        let executor = cx.background_executor().clone();
        let timeout = cx.background_executor().spawn(async move {
            executor.timer(std::time::Duration::from_secs(2)).await;
            drop(cancel);
        });
        if let Some(report) = report {
            report.await;
        }
        drop(timeout);
        if let Some(save) = save {
            save.await;
        }
    }))
}

fn start_resource_sampling(cx: &mut App) {
    use glim_services::telemetry::{flag, number, text, track};
    let task = cx.spawn(async move |cx| {
        let mut previous: Option<(std::time::Instant, u64)> = None;
        let mut initial = true;
        loop {
            let (available, values) = cx
                .background_executor()
                .spawn(async { crate::views::resource_snapshot() })
                .await;
            let now = std::time::Instant::now();
            let mut props = vec![
                ("scope", text("main_process")),
                ("includes_helpers", flag(false)),
                ("memory_available", flag(available & 1 != 0)),
                ("cpu_available", flag(available & 2 != 0)),
                ("gpu_counter_available", flag(available & 4 != 0)),
                ("gpu_counter_scope", text("metal_device_allocations")),
                ("gpu_total_vram_available", flag(false)),
                (
                    "thermal_state",
                    text(match values[12] {
                        0 => "nominal",
                        1 => "fair",
                        2 => "serious",
                        3 => "critical",
                        _ => "unknown",
                    }),
                ),
                ("low_power_available", flag(available & 8 != 0)),
            ];
            if available & 1 != 0 {
                for (key, index) in [
                    ("resident_bytes", 0),
                    ("physical_footprint_bytes", 1),
                    ("virtual_bytes", 2),
                    ("compressed_bytes", 3),
                ] {
                    props.push((key, number(values[index])));
                }
            }
            if available & 2 != 0 {
                props.extend([
                    ("cpu_total_ns", number(values[4])),
                    ("peak_resident_bytes", number(values[5])),
                ]);
                if let Some((at, cpu)) = previous {
                    let wall = now.duration_since(at).as_nanos().max(1);
                    let basis_points = (u128::from(values[4].saturating_sub(cpu)) * 10_000 / wall)
                        .min(u128::from(u64::MAX)) as u64;
                    props.push(("cpu_percent_x100", number(basis_points)));
                    props.push((
                        "sample_interval_ms",
                        number(now.duration_since(at).as_millis() as u64),
                    ));
                }
                previous = Some((now, values[4]));
            }
            if available & 4 != 0 {
                props.push(("metal_allocated_bytes", number(values[10])));
            }
            if available & 8 != 0 {
                props.push(("low_power_enabled", flag(values[13] != 0)));
            }
            let context = cx.update(|cx| {
                let workspaces = super::OpenWorkspaces::windows(cx);
                let mut props = vec![("workspace_windows", number(workspaces.len() as u64))];
                let mut tabs = 0;
                let mut active = false;
                for (view, window) in workspaces {
                    let _ = window.update(cx, |_, window, cx| {
                        let Some(view) = view.upgrade() else {
                            return;
                        };
                        tabs += view.read(cx).telemetry_tab_count();
                        if window.is_window_active() {
                            active = true;
                            props.extend(view.read(cx).usage_context());
                        }
                    });
                }
                props.push(("open_tabs", number(tabs as u64)));
                props.push(("workspace_active", flag(active)));
                props
            });
            props.extend(context);
            if initial {
                let mut profile = vec![
                    ("build_number", text(env!("GLIM_BUILD_NUMBER"))),
                    ("hardware_memory_bytes", number(values[6])),
                    ("logical_processors", number(values[7])),
                    ("active_processors", number(values[8])),
                    ("unified_memory", flag(values[11] != 0)),
                    ("metal_devices", number(values[14])),
                    ("system_uptime_s", number(values[9])),
                ];
                profile.push(("resource_scope", text("main_process")));
                profile.push(("rosetta_detection_available", flag(available & 16 != 0)));
                if available & 16 != 0 {
                    profile.push(("rosetta_translated", flag(values[15] != 0)));
                }
                profile.push(("debug_build", flag(cfg!(debug_assertions))));
                track("system_profile", profile);
                initial = false;
            }
            track("resource_sample", props);
            cx.background_executor()
                .timer(std::time::Duration::from_secs(30))
                .await;
        }
    });
    cx.global_mut::<Telemetry>().resources = Some(task);
}

pub(crate) fn observe_commands(cx: &mut App) {
    cx.observe_keystrokes(|event, window, cx| {
        let Some(action) = &event.action else {
            return;
        };
        // Resolved compile-time command names only. Never log typed keys,
        // text, input context values or clipboard contents.
        use glim_services::telemetry::{flag, text, track};
        let modifiers = event.keystroke.modifiers;
        let key = match event.keystroke.key.as_str() {
            "left" => "left",
            "right" => "right",
            "up" => "up",
            "down" => "down",
            "pageup" => "pageup",
            "pagedown" => "pagedown",
            "home" => "home",
            "end" => "end",
            "backspace" => "backspace",
            "delete" => "delete",
            "enter" => "enter",
            "escape" => "escape",
            "tab" => "tab",
            "space" => "space",
            "f1" => "f1",
            "," => "comma",
            "[" => "left_bracket",
            "]" => "right_bracket",
            "+" | "=" => "plus",
            "-" => "minus",
            "0" => "zero",
            "1" => "one",
            key if modifiers.platform || modifiers.control || modifiers.alt => match key {
                "a" => "a",
                "b" => "b",
                "c" => "c",
                "e" => "e",
                "f" => "f",
                "i" => "i",
                "n" => "n",
                "o" => "o",
                "p" => "p",
                "q" => "q",
                "r" => "r",
                "s" => "s",
                "t" => "t",
                "v" => "v",
                "w" => "w",
                "x" => "x",
                "z" => "z",
                _ => "other_bound_key",
            },
            _ => "other_bound_key",
        };
        let mut props = Vec::new();
        for (view, handle) in super::OpenWorkspaces::windows(cx) {
            if handle.window_id() == window.window_handle().window_id()
                && let Some(view) = view.upgrade()
            {
                props.extend(view.read(cx).usage_context());
                break;
            }
        }
        props.extend([
            ("action", text(action.name())),
            ("key_token", text(key)),
            ("command", flag(modifiers.platform)),
            ("control", flag(modifiers.control)),
            ("option", flag(modifiers.alt)),
            ("shift", flag(modifiers.shift)),
        ]);
        track("shortcut_used", props);
    })
    .detach();
}
