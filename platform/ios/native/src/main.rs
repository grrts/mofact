//! The iOS entry point (T02). On iOS, SDL has to own `main` so it can start
//! UIKit first; `sdl3_main::main` hands it over and runs the game once the
//! app is up. Other platforms build an empty binary.

#![cfg_attr(target_os = "ios", allow(unsafe_code))] // the macro's generated main

#[cfg(target_os = "ios")]
#[sdl3_main::main]
fn main() -> bool {
    match mofact::run() {
        Ok(()) => true,
        Err(e) => {
            eprintln!("mofact: {e}");
            false
        }
    }
}

#[cfg(not(target_os = "ios"))]
fn main() {
    eprintln!("mofact-ios only runs on iOS; use `cargo run -p mofact` here.");
}
