//! Main-thread ownership boundary for AppKit views, plus synchronized numeric
//! telemetry preference and own-process resource calls safe off the main thread.
#![allow(unsafe_code)]

use gpui_kit::{Bounds, Pixels, Window};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{ffi::c_void, marker::PhantomData, ptr::NonNull, rc::Rc};

extern "C" fn reader_telemetry(
    kind: i32,
    action: i32,
    error: i32,
    source: i32,
    value: u64,
    secondary: u64,
    count: u64,
    duration: u64,
) {
    use glim_services::telemetry::{self, flag, number, text};
    let shortcut = action >= 100;
    let action = if shortcut { action - 100 } else { action };
    let kind = match kind {
        1 => "image",
        2 => "pdf",
        3 => "word",
        4 => "workbook",
        5 => "media",
        6 => "presentation",
        7 => "html",
        8 => "font",
        0 => "native",
        _ => return,
    };
    let action = match action {
        0 => "load",
        1 => "zoom_in",
        2 => "zoom_out",
        3 => "zoom_reset",
        4 => "fit",
        5 => "next",
        6 => "previous",
        7 => "copy",
        8 => "extract_text",
        9 => "toggle_mode",
        10 => "play_pause",
        11 => "open_external",
        12 => "layout",
        13 => "slideshow_start",
        14 => "slideshow_exit",
        15 => "select_sheet",
        16 => "select_font",
        17 => "select_all",
        18 => "previous_columns",
        19 => "next_columns",
        20 => "copy_selection",
        21 => "load_more_sheets",
        22 => "playhead_jump_observed",
        23 => "volume",
        24 => "ocr_completed",
        25 => "copy_completed",
        26 => "load_completed",
        27 => "zoom_changed",
        28 => "open_link",
        29 => "text_selection",
        30 => "scroll",
        31 => "context_menu_opened",
        32 => "scroll_lines",
        33 => "page_position_observed",
        34 => "playback_position_observed",
        35 => "playback_state_observed",
        36 => "mute_state_observed",
        _ => return,
    };
    let source = match source {
        1 => "pointer",
        2 => "keyboard",
        3 => "menu",
        _ => "unknown",
    };
    if shortcut {
        telemetry::track(
            "native_shortcut_used",
            [
                ("reader_kind", text(kind)),
                ("action", text(action)),
                ("input_source", text("keyboard")),
                ("key_code", number(secondary)),
                ("command", flag(value & (1 << 20) != 0)),
                ("control", flag(value & (1 << 18) != 0)),
                ("option", flag(value & (1 << 19) != 0)),
                ("shift", flag(value & (1 << 17) != 0)),
            ],
        );
    } else if matches!(action, "scroll" | "scroll_lines") {
        telemetry::track(
            "reader_scroll",
            [
                ("reader_kind", text(kind)),
                ("action", text(action)),
                ("input_source", text(source)),
                ("capture", text("native_document_input")),
                (
                    "delta_unit",
                    text(if action == "scroll_lines" {
                        "lines"
                    } else {
                        "points"
                    }),
                ),
                ("horizontal_delta_x100", number(value)),
                ("vertical_delta_x100", number(secondary)),
                ("input_events", number(count)),
                ("duration_ms", number(duration)),
            ],
        );
    } else {
        telemetry::native_event(kind, action, error.max(0) as u32, source, value, secondary);
    }
}
pub(crate) fn set_telemetry_enabled(enabled: bool) {
    // SAFETY: numeric-only synchronized Swift preference; no retained arguments.
    unsafe {
        glim_set_telemetry_enabled(i32::from(enabled));
    }
}
pub(crate) fn resource_snapshot() -> (i32, [u64; 16]) {
    let mut values = [0; 16];
    // SAFETY: Swift writes only within this fixed capacity and retains no pointer.
    let available = unsafe { glim_telemetry_resources(values.as_mut_ptr(), values.len()) };
    (available, values)
}
unsafe extern "C" {
    fn glim_set_telemetry_enabled(enabled: i32);
    fn glim_telemetry_resources(output: *mut u64, capacity: usize) -> i32;
    fn glim_set_reader_telemetry(
        callback: Option<extern "C" fn(i32, i32, i32, i32, u64, u64, u64, u64)>,
    );
    fn glim_preview_create(bytes: *const u8, length: usize) -> *mut c_void;
    fn glim_preview_layout(
        handle: *mut c_void,
        parent: *mut c_void,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    );
    fn glim_preview_hide(handle: *mut c_void);
    fn glim_preview_release(handle: *mut c_void);
    fn glim_preview_take_navigation(handle: *mut c_void) -> *mut std::ffi::c_char;
    fn glim_preview_free_string(string: *mut std::ffi::c_char);
    fn glim_preview_reveal_anchor(handle: *mut c_void, bytes: *const u8, length: usize);
}

pub(super) struct NativePreview {
    handle: NonNull<c_void>,
    // GPUI entities are created/updated/dropped on the main thread. Prevent this
    // owner from ever being moved into a background task.
    _main_thread: PhantomData<Rc<()>>,
}
impl NativePreview {
    pub fn new(configuration: &[u8]) -> Self {
        // SAFETY: Swift copies this valid slice before returning a retained handle.
        let handle = unsafe {
            glim_set_reader_telemetry(Some(reader_telemetry));
            glim_preview_create(configuration.as_ptr(), configuration.len())
        };
        Self {
            handle: NonNull::new(handle).expect("Swift returned a null preview"),
            _main_thread: PhantomData,
        }
    }
    pub fn layout(&self, bounds: Bounds<Pixels>, window: &Window) {
        let Ok(handle) = HasWindowHandle::window_handle(window) else {
            return;
        };
        let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
            return;
        };
        // SAFETY: the live GPUI window owns the parent NSView. Swift retains the
        // child in that view; both it and our preview handle remain alive here.
        unsafe {
            glim_preview_layout(
                self.handle.as_ptr(),
                handle.ns_view.as_ptr(),
                f32::from(bounds.origin.x) as f64,
                f32::from(bounds.origin.y) as f64,
                f32::from(bounds.size.width) as f64,
                f32::from(bounds.size.height) as f64,
            )
        }
    }
    pub fn take_navigation(&self) -> Option<String> {
        // SAFETY: called on the main thread with a retained handle. Swift returns
        // an owned, NUL-terminated UTF-8 string, freed through its matching API.
        unsafe {
            let pointer = glim_preview_take_navigation(self.handle.as_ptr());
            if pointer.is_null() {
                return None;
            }
            let url = std::ffi::CStr::from_ptr(pointer)
                .to_string_lossy()
                .into_owned();
            glim_preview_free_string(pointer);
            Some(url)
        }
    }
    pub fn reveal_anchor(&self, anchor: &str) {
        // SAFETY: Swift copies this valid UTF-8 slice during the main-thread call.
        unsafe { glim_preview_reveal_anchor(self.handle.as_ptr(), anchor.as_ptr(), anchor.len()) }
    }
    pub fn hide(&self) {
        // SAFETY: this owner retains the Swift controller, on the main thread.
        unsafe { glim_preview_hide(self.handle.as_ptr()) }
    }
}
impl Drop for NativePreview {
    fn drop(&mut self) {
        // SAFETY: release the sole retained handle exactly once, detaching its view
        // and stopping playback before the prepared SQLite snapshot is dropped.
        unsafe { glim_preview_release(self.handle.as_ptr()) }
    }
}
