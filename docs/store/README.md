# Store pages

Everything the Google Play, App Store and Steam pages need, made from the
real game.

| File | What it is |
| --- | --- |
| [listing.md](listing.md) | Names, short and long descriptions, subtitle, promo text, keywords, tags and the in-app purchase's wording, for each store |
| [privacy-policy.md](privacy-policy.md) | The privacy policy. It is also served at https://grrts.github.io/mofact/privacy.html (from `web/privacy.html`), which is the URL the stores ask for |
| `screenshots/` | Six screenshots for each store size, in the order to upload them |
| `art/` | The Play feature graphic and Steam's capsules, hero and logo |

## Screenshots

Each folder holds the same six shots: `1-factory` (the factory at work),
`2-build` (the build sheet), `3-research`, `4-ratios` (the recipe browser's
ratio calculator), `5-train` (a train's schedule) and `6-night`.

| Folder | Size | Upload to |
| --- | --- | --- |
| `app-store-iphone` | 1320 × 2868 | App Store, 6.9" iPhone (Apple scales it for smaller iPhones) |
| `app-store-ipad` | 2064 × 2752 | App Store, 13" iPad (scaled for smaller iPads) |
| `play-phone` | 1080 × 1920 | Google Play, phone screenshots |
| `play-tablet` | 2560 × 1600 | Google Play, 7" and 10" tablet screenshots |
| `steam` | 1920 × 1080 | Steam, screenshots |

They are JPEGs, as the App Store refuses PNGs with an alpha channel.

## Art

| File | Store slot |
| --- | --- |
| `play-feature-graphic-1024x500.png` | Google Play feature graphic |
| `steam-header-920x430.png` | Steam header capsule |
| `steam-small-462x174.png` | Steam small capsule |
| `steam-main-1232x706.png` | Steam main capsule |
| `steam-library-600x900.png` | Steam library capsule |
| `steam-library-hero-3840x1240.png` | Steam library hero (no text, as Steam asks) |
| `steam-library-logo-1280x720.png` | Steam library logo (transparent) |

The name is set in the game's own font (Saira Semi Condensed, under the OFL
in `assets/fonts`).

## The app icon

A hub-yellow gear over a belt carrying iron and copper plates, on the
factory floor, drawn in code in `crates/artkit/src/icon.rs` with the game's
palette. `cargo run -p mofact-icon` writes it at every size: Android's
launcher icons and adaptive layers, iOS's home screen icons, the web link's
favicon and home screen icons, the Windows `.ico` in `platform/steam`, and
these store files:

| File | Store slot |
| --- | --- |
| `art/play-icon-512x512.png` | Google Play app icon |
| `art/app-store-icon-1024x1024.png` | App Store icon |
| `art/steam-community-icon-184x184.png` | Steam community icon (Steamworks asks for a JPG: save it as one when uploading) |

CI fails if a committed icon no longer matches the drawing.

## Making them again

The factory in the pictures is `tools/store/showcase.ron`, a scenario built
from the game's own buildings and recipes on the default map. The app
photographs it at every size after `warmup_seconds`, with the HUD as a
player of the full game sees it:

```sh
cargo run --release -p mofact -- --store /tmp/store-raw --scenario tools/store/showcase.ron
python3 tools/store/compose.py /tmp/store-raw docs/store   # needs Pillow
python3 tools/store/check.py                                # the copy fits each store's limits
```

CI runs the first and last on every pull request. The first fails if any
machine in the showcase is stuck, so a content change that breaks the
picture shows up at once.

## Privacy answers for the store forms

- **Google Play, Data safety:** the app collects "Purchase history" (through
  RevenueCat), for app functionality only, not shared, encrypted in transit,
  and not deletable on request because it is the store's own purchase record.
  Nothing else is collected. No ads.
- **App Store, App Privacy:** "Purchases" (purchase history), used for app
  functionality, not linked to the user's identity, not used for tracking.
  Nothing else.
- **Steam:** no form; link the policy URL above.

Check RevenueCat's current guidance for both forms before submitting; it
lists what its SDK sends.

## Still to come

- **A support email.** Every store asks for one on the listing; it is the one
  thing here only Leroy can provide.
- **A trailer** is optional on all three stores and not made yet.
