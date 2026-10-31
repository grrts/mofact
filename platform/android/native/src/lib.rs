//! The Android entry point (T02). SDL's Java activity (`MofactActivity` in
//! `platform/android/app`) loads `libmain.so` and calls `SDL_main` on its
//! own thread; from there the game runs as on desktop.

#![cfg(target_os = "android")]
// Exporting an unmangled symbol counts as unsafe code; this is the only one.
#![allow(unsafe_code)]

use std::ffi::{c_char, c_int};

/// Called by SDL once the activity is up. The arguments are SDL's, not ours.
#[no_mangle]
pub extern "C" fn SDL_main(_argc: c_int, _argv: *mut *mut c_char) -> c_int {
    match mofact::run() {
        Ok(()) => 0,
        Err(e) => {
            sdl3::log::log(&format!("mofact: {e}"));
            1
        }
    }
}
