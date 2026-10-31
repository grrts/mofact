//! The one place the app touches raw window handles.
#![allow(unsafe_code)]

use mofact_render::wgpu;
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};

/// Create a wgpu surface for `window`.
///
/// The surface borrows the native window without a lifetime, so the caller
/// must drop it before the window. [`crate::App`] does this by declaring its
/// `target` field before its `window` field, and by dropping the target
/// whenever the platform takes the window away (going to the background).
pub fn create(
    instance: &wgpu::Instance,
    window: &sdl3::video::Window,
) -> Result<wgpu::Surface<'static>, String> {
    let target = wgpu::SurfaceTargetUnsafe::RawHandle {
        raw_display_handle: Some(
            window
                .display_handle()
                .map_err(|e| format!("no display handle: {e}"))?
                .as_raw(),
        ),
        raw_window_handle: window
            .window_handle()
            .map_err(|e| format!("no window handle: {e}"))?
            .as_raw(),
    };
    // SAFETY: both handles come from a live SDL window, and the surface is
    // dropped before that window is (see above).
    unsafe { instance.create_surface_unsafe(target) }.map_err(|e| e.to_string())
}
