# iOS

The iOS build (T02): the `mofact-ios` binary (`native/`), started through
SDL's UIKit main, in an app bundle described by `Info.plist`. The unlock purchase goes through RevenueCat (T61); iCloud saves (T62)
come later.

CI (`.github/workflows/builds.yml`) builds every commit for devices and for
the simulator, and runs the simulator build for 20 seconds as a smoke test.
By hand, on a Mac:

```sh
cargo build --release -p mofact-ios --target aarch64-apple-ios-sim
mkdir -p Mofact.app && cp target/aarch64-apple-ios-sim/release/Mofact platform/ios/Info.plist Mofact.app/
xcrun simctl install booted Mofact.app && xcrun simctl launch booted com.mofact.game
```

Device builds need signing with the team's certificate and a provisioning
profile before they install; TestFlight uploads arrive with the Apple account (T04).
