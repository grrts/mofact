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

## Still to come with the Apple account

- **The full game (T61).** Until then iOS uses the free sandbox shop, as
  desktop does. RevenueCat's iOS SDK and StoreKit 2 are Swift packages, so
  they arrive with the Xcode project that signing needs anyway; the game's
  side (`mofact::shop::Shop`) is ready for them.
- **iCloud saves (T62).** The game's side is `mofact::cloud::Cloud`; iCloud
  needs the app's container id and entitlement.
- **Haptics** already work: UIKit's feedback generators, through objc2.

Device builds need signing with the team's certificate and a provisioning
profile before they install; TestFlight uploads arrive with the Apple account (T04).
