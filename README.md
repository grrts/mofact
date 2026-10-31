# Mofact

A factory automation game built for phones first, then Steam. One Rust codebase
ships to iOS, Android and Windows.

**Play it:** https://grrts.github.io/mofact/ (always the latest `main`; every pull request gets its own preview link). Until the Rust app has a web build this serves the phone prototype in `web/`.

**Stack:** Rust (stable), wgpu for graphics, SDL3 for windows, input, audio and
app lifecycle, tiny-skia for art drawn in code, RON for content data.

## Layout

Each lane owns its own folder, so parallel work rarely touches the same files.

| Folder | Lane | Owns |
| --- | --- | --- |
| `crates/core` | shared | Contracts between lanes: ids, geometry, commands, snapshots, content schema |
| `crates/sim` | Sim | Belts, machines, power, fluids, trains, drones, command handling |
| `crates/world` | World | Chunks, noise, biomes, resources, terrain |
| `crates/render`, `crates/artkit` | Render and art | Renderer, atlases, zoom bands, building drawings |
| `crates/ui`, `crates/input` | UI and input | Radial menu, sheets, lasso, camera, hotkeys |
| `data/`, `crates/content` | Content | Items, recipes, research, biomes (`*.ron`) and their loader |
| `tools/balance` | Content | Validator and balance report |
| `app/`, `platform/*` | Platform | App shell, builds, saves, store, cloud |
| `web/` | Platform | The playable web link (the prototype for now) |
| `tools/headless`, `tools/bench` | QA | Headless scenarios and benchmarks |

## Architecture in one paragraph

The simulation runs on its own thread at 60 ticks a second and is the only
thing that changes the world. The main thread sends it `Command`s and draws the
latest `Snapshot` it publishes, so a slow frame never slows the factory. The
headless runner drives the same simulation in CI with no renderer at all.
Both types live in `crates/core`.

## Working on it

```sh
cargo test --workspace          # every lane's tests
cargo clippy --workspace --all-targets
cargo fmt --all
cargo run -p mofact             # the app (headless until the SDL3 shell lands)
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for task size, the definition of done
and the rules for changing `crates/core`.
