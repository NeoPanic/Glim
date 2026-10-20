//! Native path hints live above embedded document views, independently of readers.
#![allow(unsafe_code)]
use gpui_kit::{Bounds, Pixels, Window};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{ffi::c_void, marker::PhantomData, ptr::NonNull, rc::Rc};

unsafe extern "C" {
    fn glim_path_hint_create(
        parent: *mut c_void,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        title: *const u8,
        title_len: usize,
        path: *const u8,
        path_len: usize,
        directory: bool,
    ) -> *mut c_void;
    fn glim_path_hint_release(handle: *mut c_void);
}
struct NativeHint {
    handle: NonNull<c_void>,
    _main_thread: PhantomData<Rc<()>>,
}
impl Drop for NativeHint {
    fn drop(&mut self) {
        // SAFETY: this retained AppKit owner is released only on the UI thread.
        unsafe { glim_path_hint_release(self.handle.as_ptr()) }
    }
}
#[derive(Default)]
pub(super) struct PathHint {
    active: Option<NativeHint>,
    target: Option<(String, Bounds<Pixels>)>,
}
impl PathHint {
    pub fn hide(&mut self) {
        self.active = None;
        self.target = None;
    }
    pub fn hide_for(&mut self, path: &str, bounds: Bounds<Pixels>) {
        if self
            .target
            .as_ref()
            .is_some_and(|(current, anchor)| current == path && *anchor == bounds)
        {
            self.hide();
        }
    }
    pub fn show(
        &mut self,
        title: &str,
        path: &str,
        directory: bool,
        bounds: Bounds<Pixels>,
        window: &Window,
    ) {
        self.hide();
        if bounds.size.width <= gpui_kit::px(0.) || bounds.size.height <= gpui_kit::px(0.) {
            return;
        }
        let Ok(handle) = HasWindowHandle::window_handle(window) else {
            return;
        };
        let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
            return;
        };
        // SAFETY: the live window owns the parent view; Swift copies the strings
        // and anchor immediately, then manages its delayed panel on the main queue.
        let pointer = unsafe {
            glim_path_hint_create(
                handle.ns_view.as_ptr(),
                f32::from(bounds.origin.x) as f64,
                f32::from(bounds.origin.y) as f64,
                f32::from(bounds.size.width) as f64,
                f32::from(bounds.size.height) as f64,
                title.as_ptr(),
                title.len(),
                path.as_ptr(),
                path.len(),
                directory,
            )
        };
        self.target = Some((path.to_owned(), bounds));
        self.active = NonNull::new(pointer).map(|handle| NativeHint {
            handle,
            _main_thread: PhantomData,
        });
    }
}
