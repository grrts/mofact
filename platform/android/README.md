# Android

The Android build (T02): a Gradle project around SDL's Java activity, with
the game in `libmain.so`. Play Billing (T61) and Play Games saved games (T62)
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
