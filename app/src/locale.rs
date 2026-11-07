//! The device's languages, most preferred first (T65), from SDL: the
//! phone's language list on iOS and Android, the desktop's on the rest.
//! `MOFACT_LANG` (such as `ja`) stands in for them, for screenshots and tests.
#![allow(unsafe_code)]

use mofact_ui::i18n::Lang;
use std::ffi::CStr;

/// The device's preferred locales, as tags such as `de-AT`.
pub fn preferred() -> Vec<String> {
    if let Ok(tag) = std::env::var("MOFACT_LANG") {
        return vec![tag];
    }
    let mut out = Vec::new();
    let mut count = 0;
    // SAFETY: SDL hands back one allocation: an array of `count` pointers to
    // locales whose strings live in the same block. It is read here, then
    // freed once, with SDL_free.
    unsafe {
        let list = sdl3::sys::locale::SDL_GetPreferredLocales(&mut count);
        if list.is_null() {
            return out;
        }
        for i in 0..usize::try_from(count).unwrap_or(0) {
            let locale = *list.add(i);
            if locale.is_null() || (*locale).language.is_null() {
                continue;
            }
            let mut tag = CStr::from_ptr((*locale).language)
                .to_string_lossy()
                .into_owned();
            if !(*locale).country.is_null() {
                tag.push('-');
                tag.push_str(&CStr::from_ptr((*locale).country).to_string_lossy());
            }
            out.push(tag);
        }
        sdl3::sys::stdinc::SDL_free(list.cast());
    }
    out
}

/// The language to show when the player hasn't picked one.
pub fn device() -> Lang {
    let tags = preferred();
    Lang::for_device(tags.iter().map(String::as_str))
}
