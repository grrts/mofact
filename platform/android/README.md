# Android

The Android build (T02): a Gradle project around SDL's Java activity, with
the game in `libmain.so`. The unlock purchase goes through RevenueCat (T61); Play Games saved games (T62)
come later.

CI (`.github/workflows/builds.yml`) builds a debug APK of every commit. By hand,
with the NDK, `cargo-ndk` and Gradle 8.12:

```sh
export ANDROID_NDK_HOME=/path/to/ndk
cargo ndk -t arm64-v8a --platform 26 -o platform/android/app/libs build --release -p mofact-android
cp -r "$(dirname "$(cargo metadata --format-version 1 | jq -r '.packages[] | select(.name == "sdl3-src") | .manifest_path')")/SDL/android-project/app/src/main/java" platform/android/sdl-java
gradle -p platform/android assembleDebug
adb install platform/android/app/build/outputs/apk/debug/app-debug.apk
```

- `native/` is the `mofact-android` crate: `SDL_main`, which starts the game.
- `app/` holds the manifest, `MofactActivity` and resources.
- The game data is built into the library, so the APK needs no assets.

## The full game (T61)

`MofactActivity` sells the full game through RevenueCat; `native/src/store.rs`
is the game's side of it. The public Android API key is built in from the
`mofactRevenueCatKey` Gradle property or `MOFACT_REVENUECAT_KEY` (CI reads the
`REVENUECAT_ANDROID_KEY` secret). Without a key the APK uses the free sandbox,
as desktop and the web do.

RevenueCat needs, once the Play account exists: a one-time in-app product for
the full game, an entitlement named `full_game` attached to it, and a current
offering with the product as its lifetime package. Whether it was bought is
remembered on the device, so the full game opens offline.
