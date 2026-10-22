use gpui_kit::*;

#[derive(Default)]
pub(crate) struct Telemetry {
    pub enabled: bool,
    report: Option<Task<()>>,
    cancel: Option<glim_services::work::CancelOnDrop>,
    save: Option<Task<()>>,
}
impl Global for Telemetry {}

pub(crate) fn initialize(cx: &mut App) {
    let enabled = glim_services::preferences::telemetry_enabled();
    cx.set_global(Telemetry {
        enabled,
        ..Default::default()
    });
    if enabled {
        start(cx);
    }
}
fn start(cx: &mut App) {
    let (cancel, guard) = glim_services::work::Cancellation::task();
    let task = cx.background_executor().spawn(async move {
        glim_services::telemetry::report_startup(&cancel);
    });
    let state = cx.global_mut::<Telemetry>();
    state.cancel = Some(guard);
    state.report = Some(task);
}
pub(crate) fn set_enabled(enabled: bool, cx: &mut App) {
    let state = cx.global_mut::<Telemetry>();
    if state.enabled == enabled {
        return;
    }
    state.enabled = enabled;
    state.cancel.take();
    state.report.take();
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
        start(cx);
    }
    cx.refresh_windows();
}

pub(crate) fn shutdown(cx: &mut App) -> Option<Task<()>> {
    let state = cx.global_mut::<Telemetry>();
    state.cancel.take();
    state.report.take();
    state.save.take()
}
