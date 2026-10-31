//! The desktop entry point. Phones start the same game through their
//! platform shims (`platform/android`, `platform/ios`).

fn main() -> std::process::ExitCode {
    mofact::main()
}
