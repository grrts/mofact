//! SDL's UIKit code checks the OS version with `@available`, which clang
//! compiles into calls to `__isPlatformVersionAtLeast` in its own runtime
//! library. Rust links without clang's runtime, so name it here.

use std::process::Command;

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("ios") {
        return;
    }
    let out = Command::new("xcrun")
        .args(["clang", "--print-resource-dir"])
        .output()
        .expect("xcrun clang (Xcode's command line tools)");
    let dir = String::from_utf8(out.stdout).expect("a path");
    let sim = std::env::var("CARGO_CFG_TARGET_ABI").as_deref() == Ok("sim")
        || std::env::var("TARGET").is_ok_and(|t| t.ends_with("-sim"));
    println!("cargo:rustc-link-search=native={}/lib/darwin", dir.trim());
    println!(
        "cargo:rustc-link-lib=static={}",
        if sim { "clang_rt.iossim" } else { "clang_rt.ios" }
    );
}
