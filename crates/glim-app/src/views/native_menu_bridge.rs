//! Main-thread AppKit context-menu ownership, independent of document readers.
#![allow(unsafe_code)]
use gpui_kit::{AsyncWindowContext, Window};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{ffi::c_void, marker::PhantomData, ptr::NonNull, rc::Rc};

unsafe extern "C" {
    fn glim_context_menu_create(
        parent: *mut c_void,
        x: f64,
        y: f64,
        bytes: *const u8,
        length: usize,
    ) -> *mut c_void;
    fn glim_context_menu_selection(handle: *mut c_void) -> i64;
    fn glim_context_menu_release(handle: *mut c_void);
}
pub(super) struct NativeMenu {
    handle: NonNull<c_void>,
    _main_thread: PhantomData<Rc<()>>,
}
impl NativeMenu {
    pub fn new(items: &[(Option<String>, bool)], window: &Window) -> Option<Self> {
        let handle = HasWindowHandle::window_handle(window).ok()?;
        let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
            return None;
        };
        let position = window.mouse_position();
        let items = items
            .iter()
            .map(|(title, enabled)| serde_json::json!({"title": title, "enabled": enabled}))
            .collect::<Vec<_>>();
        let bytes = serde_json::to_vec(&items).expect("Context menu titles");
        // SAFETY: AppKit's live parent view is owned by this window. Swift copies
        // titles and schedules menu tracking after the current Rust update ends.
        let pointer = unsafe {
            glim_context_menu_create(
                handle.ns_view.as_ptr(),
                f32::from(position.x) as f64,
                f32::from(position.y) as f64,
                bytes.as_ptr(),
                bytes.len(),
            )
        };
        Some(Self {
            handle: NonNull::new(pointer)?,
            _main_thread: PhantomData,
        })
    }
    pub async fn choose(self, cx: &mut AsyncWindowContext) -> Option<usize> {
        loop {
            if let Some(selection) = self.selection() {
                return selection;
            }
            cx.background_executor()
                .timer(std::time::Duration::from_millis(100))
                .await;
        }
    }
    /// None while tracking; Some(None) on dismissal; otherwise the item index.
    pub fn selection(&self) -> Option<Option<usize>> {
        // SAFETY: the retained controller is accessed only on the UI thread.
        match unsafe { glim_context_menu_selection(self.handle.as_ptr()) } {
            -2 => None,
            -1 => Some(None),
            index => Some(usize::try_from(index).ok()),
        }
    }
}
impl Drop for NativeMenu {
    fn drop(&mut self) {
        // SAFETY: release this sole retained controller and cancel open tracking.
        unsafe { glim_context_menu_release(self.handle.as_ptr()) }
    }
}
