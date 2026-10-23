//! Foreground occupancy, not attention: no input contents or local target identity
//! leave this module. Segments stop at loading, prompts and window deactivation.
use super::{Page, Workspace, header::Target};
use glim_services::telemetry::{self, Property, flag, number, text};
use gpui_kit::*;
use std::time::{Duration, Instant};

pub(super) struct Segment {
    key: (Page, Option<Target>, bool),
    started: Instant,
    props: Vec<(&'static str, Property)>,
}
impl Workspace {
    pub(crate) fn telemetry_tab_count(&self) -> usize {
        self.tabs.len()
    }
    pub(crate) fn usage_context(&self) -> Vec<(&'static str, Property)> {
        let mut props = vec![(
            "page",
            text(match self.page {
                Page::Document => "document",
                Page::Start => "start",
                Page::Settings => "settings",
            }),
        )];
        if self.page == Page::Document
            && let Some(reader) = self.active_reader()
        {
            props.extend(telemetry::file_properties(&reader.path));
            props.push(("reader_kind", text(reader.telemetry_kind())));
            props.push(("preview", flag(reader.toggle_label() == "Source")));
        }
        props
    }

    pub(crate) fn reset_usage(&mut self) {
        self.usage = None;
        self.scroll_usage = None;
        self.scroll_usage_task = None;
    }
    pub(crate) fn flush_usage(&mut self) {
        self.flush_scroll();
        if let Some(segment) = self.usage.take() {
            let elapsed = segment.started.elapsed().as_millis().min(u64::MAX as u128) as u64;
            let mut props = segment.props;
            props.push(("duration_ms", number(elapsed)));
            telemetry::track("foreground_segment", props);
        }
    }
    pub(super) fn update_usage(&mut self, window: &Window, cx: &App) {
        let enabled = cx
            .try_global::<crate::app::telemetry::Telemetry>()
            .is_some_and(|t| t.enabled);
        if !enabled {
            self.usage = None;
            return;
        }
        if !window.is_window_active() || window.has_active_prompt() || self.loading || self.picking
        {
            self.flush_usage();
            return;
        }
        let reader = if self.page == Page::Document {
            self.active_reader()
        } else {
            None
        };
        let preview = reader.is_some_and(|r| r.toggle_label() == "Source");
        let key = (self.page, reader.map(Target::from_reader), preview);
        if self.usage.as_ref().is_some_and(|s| s.key == key) {
            return;
        }
        let mut props = vec![
            (
                "page",
                text(match self.page {
                    Page::Document => "document",
                    Page::Settings => "settings",
                    Page::Start => "start",
                }),
            ),
            ("preview", flag(preview)),
        ];
        if let Some(reader) = reader {
            props.extend(telemetry::file_properties(&reader.path));
            props.push(("reader_kind", text(reader.telemetry_kind())));
        }
        self.flush_usage();
        self.usage = Some(Segment {
            key,
            started: Instant::now(),
            props,
        });
    }
    pub(super) fn start_usage_timer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.usage_task = Some(cx.spawn_in(window, async move |view, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_secs(30))
                    .await;
                if view
                    .update_in(cx, |view, window, cx| {
                        view.flush_usage();
                        view.update_usage(window, cx);
                    })
                    .is_err()
                {
                    break;
                }
            }
        }));
    }
}

pub(super) struct ScrollUsage {
    started: Instant,
    last: Instant,
    events: u64,
    pixels_x: f64,
    pixels_y: f64,
    lines_x: f64,
    lines_y: f64,
    horizontal_positive: u64,
    horizontal_negative: u64,
    vertical_positive: u64,
    vertical_negative: u64,
    props: Vec<(&'static str, Property)>,
}
impl Workspace {
    pub(super) fn record_scroll(
        &mut self,
        event: &ScrollWheelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !cx
            .try_global::<crate::app::telemetry::Telemetry>()
            .is_some_and(|t| t.enabled)
        {
            return;
        }
        let now = Instant::now();
        let props = self.usage_context();
        let usage = self.scroll_usage.get_or_insert_with(|| ScrollUsage {
            started: now,
            last: now,
            events: 0,
            pixels_x: 0.,
            pixels_y: 0.,
            lines_x: 0.,
            lines_y: 0.,
            horizontal_positive: 0,
            horizontal_negative: 0,
            vertical_positive: 0,
            vertical_negative: 0,
            props,
        });
        usage.last = now;
        usage.events += 1;
        let (x, y) = match event.delta {
            ScrollDelta::Pixels(delta) => {
                let (x, y) = (f64::from(f32::from(delta.x)), f64::from(f32::from(delta.y)));
                usage.pixels_x += x.abs();
                usage.pixels_y += y.abs();
                (x, y)
            }
            ScrollDelta::Lines(delta) => {
                let (x, y) = (f64::from(delta.x), f64::from(delta.y));
                usage.lines_x += x.abs();
                usage.lines_y += y.abs();
                (x, y)
            }
        };
        usage.horizontal_positive += u64::from(x > 0.);
        usage.horizontal_negative += u64::from(x < 0.);
        usage.vertical_positive += u64::from(y > 0.);
        usage.vertical_negative += u64::from(y < 0.);
        if self.scroll_usage_task.is_none() {
            self.scroll_usage_task = Some(cx.spawn_in(window, async move |view, cx| {
                loop {
                    cx.background_executor()
                        .timer(Duration::from_millis(800))
                        .await;
                    let done = view
                        .update(cx, |view, _| {
                            let ready = view.scroll_usage.as_ref().is_none_or(|u| {
                                u.last.elapsed() >= Duration::from_millis(700)
                                    || u.started.elapsed() >= Duration::from_secs(10)
                            });
                            if ready {
                                view.flush_scroll();
                                view.scroll_usage_task = None;
                            }
                            ready
                        })
                        .unwrap_or(true);
                    if done {
                        break;
                    }
                }
            }));
        }
    }
    fn flush_scroll(&mut self) {
        let Some(usage) = self.scroll_usage.take() else {
            return;
        };
        let mut props = usage.props;
        props.extend([
            ("capture", text("gpui_document_input")),
            ("input_events", number(usage.events)),
            (
                "duration_ms",
                number(usage.last.duration_since(usage.started).as_millis() as u64),
            ),
            ("horizontal_points", number(usage.pixels_x.round() as u64)),
            ("vertical_points", number(usage.pixels_y.round() as u64)),
            (
                "horizontal_lines_x100",
                number((usage.lines_x * 100.).round() as u64),
            ),
            (
                "vertical_lines_x100",
                number((usage.lines_y * 100.).round() as u64),
            ),
            ("positive_x_events", number(usage.horizontal_positive)),
            ("negative_x_events", number(usage.horizontal_negative)),
            ("positive_y_events", number(usage.vertical_positive)),
            ("negative_y_events", number(usage.vertical_negative)),
        ]);
        telemetry::track("reader_scroll", props);
    }
}
