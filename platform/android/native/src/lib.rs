//! The Android entry point (T02). SDL's Java activity (`MofactActivity` in
//! `platform/android/app`) loads `libmain.so` and calls `SDL_main` on its
//! own thread; from there the game runs as on desktop, selling the full
//! game through RevenueCat in the activity (T61, [`store`]).

// Exporting unmangled symbols and the raw JNI calls count as unsafe code.
#![allow(unsafe_code)]

pub mod store;

#[cfg(target_os = "android")]
mod android {
    use jni::objects::{JClass, JObject, JString};
    use jni::sys::jint;
    use jni::JNIEnv;
    use std::ffi::{c_char, c_int};

    /// Called by SDL once the activity is up. The arguments are SDL's, not ours.
    #[no_mangle]
    pub extern "C" fn SDL_main(_argc: c_int, _argv: *mut *mut c_char) -> c_int {
        let platform = mofact::Platform {
            shop: Some(super::store::make(call)),
            haptic: Some(haptic),
        };
        match mofact::run_with(platform) {
            Ok(()) => 0,
            Err(e) => {
                sdl3::log::log(&format!("mofact: {e}"));
                1
            }
        }
    }

    /// `MofactActivity.nativeStoreEvent`: the store's answers.
    #[no_mangle]
    pub extern "system" fn Java_com_mofact_game_MofactActivity_nativeStoreEvent(
        mut env: JNIEnv,
        _class: JClass,
        kind: jint,
        text: JString,
    ) {
        let text = env.get_string(&text).map(String::from).unwrap_or_default();
        super::store::answer(kind, text);
    }

    /// Play a feel through the activity.
    fn haptic(feel: mofact::haptics::Haptic) {
        with_activity(|env, activity| {
            env.call_method(
                activity,
                "haptic",
                "(I)Z",
                &[jni::objects::JValue::Int(feel.code())],
            )
            .and_then(|v| v.z())
        });
    }

    /// Call one of the activity's `()Z` methods from the game's thread.
    fn call(method: &str) -> bool {
        with_activity(|env, activity| {
            env.call_method(activity, method, "()Z", &[])
                .and_then(|v| v.z())
        })
    }

    /// Run `f` with this thread's JNIEnv and the activity; false if
    /// either is missing or Java threw.
    fn with_activity(f: impl FnOnce(&mut JNIEnv, &JObject) -> jni::errors::Result<bool>) -> bool {
        // SAFETY: SDL hands out this thread's JNIEnv, attaching it if need
        // be, and a fresh local reference to the activity, which is
        // released below.
        unsafe {
            let env = sdl3::sys::system::SDL_GetAndroidJNIEnv() as *mut jni::sys::JNIEnv;
            let Ok(mut env) = JNIEnv::from_raw(env) else {
                return false;
            };
            let activity = sdl3::sys::system::SDL_GetAndroidActivity() as jni::sys::jobject;
            if activity.is_null() {
                return false;
            }
            let activity = JObject::from_raw(activity);
            let answer = f(&mut env, &activity);
            if env.exception_check().unwrap_or(false) {
                let _ = env.exception_describe();
                let _ = env.exception_clear();
            }
            let _ = env.delete_local_ref(activity);
            answer.unwrap_or(false)
        }
    }
}
