use glim_services::drafts::{self, Client, Draft};
use gpui_kit::*;

struct Recovery {
    client: Client,
    worker: Option<Task<anyhow::Result<()>>>,
    _monitor: Option<Task<()>>,
}
impl Global for Recovery {}
impl Drop for Recovery {
    fn drop(&mut self) {
        self.client.shutdown();
    }
}

pub(super) fn init(cx: &mut App) {
    let (client, worker) = drafts::channel();
    let background = cx.background_executor().spawn(async move { worker.run() });
    let observer = client.clone();
    let monitor = cx.spawn(async move |cx| {
        let mut restored = false;
        loop {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(250))
                .await;
            if !restored && let Some(paths) = observer.recovered() {
                restored = true;
                for path in paths {
                    let (cancel, guard) = glim_services::work::Cancellation::task();
                    let loaded = cx
                        .background_executor()
                        .spawn(async move { drafts::read_cancellable(&path, &cancel) })
                        .await;
                    drop(guard);
                    cx.update(|cx| match loaded {
                        Ok(draft) => {
                            if super::OpenWorkspaces::windows(cx)
                                .iter()
                                .all(|(view, _)| view.upgrade().is_none())
                            {
                                super::open_workspace(None, cx);
                            }
                            if let Some((view, window)) = super::OpenWorkspaces::windows(cx)
                                .into_iter()
                                .find(|(view, _)| view.upgrade().is_some())
                            {
                                let _ = window.update(cx, |_, window, cx| {
                                    let _ = view.update(cx, |view, cx| {
                                        view.restore_draft(draft, window, cx)
                                    });
                                });
                            }
                        }
                        Err(error) => report(
                            format!(
                                "{}: {error:#}",
                                glim_services::locale::tr("Could not recover a note")
                            ),
                            cx,
                        ),
                    });
                }
            }
            if let Some(error) = observer.take_error() {
                cx.update(|cx| {
                    report(
                        format!(
                            "{}: {error}",
                            glim_services::locale::tr(
                                "Draft recovery is unavailable. Save important notes to a file."
                            )
                        ),
                        cx,
                    )
                });
            }
        }
    });
    cx.set_global(Recovery {
        client,
        worker: Some(background),
        _monitor: Some(monitor),
    });
}
fn report(message: String, cx: &mut App) {
    log::error!("{message}");
    for (view, _) in super::OpenWorkspaces::windows(cx) {
        if let Some(view) = view.upgrade() {
            view.update(cx, |view, cx| {
                view.report_recovery_error(message.clone(), cx)
            });
            break;
        }
    }
}
pub(crate) fn checkpoint(draft: Draft, cx: &App) {
    if let Some(recovery) = cx.try_global::<Recovery>() {
        recovery.client.checkpoint(draft);
    }
}
pub(crate) fn remove(id: &str, cx: &App) {
    if let Some(recovery) = cx.try_global::<Recovery>() {
        recovery.client.remove(id);
    }
}
pub(super) fn shutdown(cx: &mut App) -> Option<Task<anyhow::Result<()>>> {
    cx.try_global::<Recovery>()?;
    let recovery = cx.global_mut::<Recovery>();
    recovery._monitor = None;
    recovery.client.shutdown();
    recovery.worker.take()
}
