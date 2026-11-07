//! The iOS entry point (T02). On iOS, SDL has to own `main` so it can start
//! UIKit first; `sdl3_main::main` hands it over and runs the game once the
//! app is up. Other platforms build an empty binary.
//!
//! Haptics go through UIKit's feedback generators; the game runs on the
//! main thread here, as UIKit wants.

// The macro's generated main, and the Objective-C calls.
#![cfg_attr(target_os = "ios", allow(unsafe_code))]

#[cfg(target_os = "ios")]
#[sdl3_main::main]
fn main() -> bool {
    let platform = mofact::Platform {
        shop: None,
        haptic: Some(haptic),
        // iCloud needs the Apple account (T62).
        cloud: None,
    };
    match mofact::run_with(platform) {
        Ok(()) => true,
        Err(e) => {
            eprintln!("mofact: {e}");
            false
        }
    }
}

/// Play a feel: a light impact for a tick, a notification otherwise.
#[cfg(target_os = "ios")]
fn haptic(feel: mofact::haptics::Haptic) {
    use mofact::haptics::Haptic;
    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject};
    // SAFETY: plain UIKit calls on the main thread, with the argument types
    // UIKit declares (CGFloat is f64 and NSInteger isize on 64-bit iOS).
    unsafe {
        match feel {
            Haptic::Tick => {
                let Some(class) = AnyClass::get(c"UIImpactFeedbackGenerator") else {
                    return;
                };
                let g: Retained<AnyObject> = msg_send![class, new];
                let _: () = msg_send![&*g, impactOccurredWithIntensity: 0.5f64];
            }
            Haptic::Warning | Haptic::Success => {
                let Some(class) = AnyClass::get(c"UINotificationFeedbackGenerator") else {
                    return;
                };
                let g: Retained<AnyObject> = msg_send![class, new];
                // UINotificationFeedbackType: success 0, warning 1.
                let kind: isize = if feel == Haptic::Success { 0 } else { 1 };
                let _: () = msg_send![&*g, notificationOccurred: kind];
            }
        }
    }
}

#[cfg(not(target_os = "ios"))]
fn main() {
    eprintln!("mofact-ios only runs on iOS; use `cargo run -p mofact` here.");
}
