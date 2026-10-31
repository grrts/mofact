# Web build

The playable link runs **the real game**: the same Rust app the phones and
desktop run (`app`, with its renderer, HUD and simulation), compiled to
WebAssembly by `platform/web` and drawn with WebGL2 on the canvas in
`index.html`. Anything added to the app is on the link with the next deploy;
there is no separate web version to keep in step.

Differences from the native shell, all inside `platform/web`:

- the simulation ticks on the frame loop (`mofact_sim::LocalSim`) instead of
  its own thread, still at 60 ticks a second of real time
- saves go to the browser's `localStorage`, every 30 seconds and when the tab
  is hidden or closed; `?new` in the address starts a new game

The Pages workflow (`.github/workflows/pages.yml`) builds it and publishes this folder:

- every push to `main` goes to https://grrts.github.io/mofact/
- every pull request gets a preview at https://grrts.github.io/mofact/pr/<number>/, linked in a comment on the PR, and removed when the PR closes

`__MOFACT_BUILD__` is replaced with the commit at deploy time. The page loads
the game's script and wasm with that commit in their URLs, so a new deploy is
picked up on the next load instead of coming from the browser cache.

To run it locally (the `wasm-bindgen` CLI must match the version in `Cargo.lock`):

```sh
cargo build --release -p mofact-web --target wasm32-unknown-unknown
wasm-bindgen --target web --no-typescript --out-dir web target/wasm32-unknown-unknown/release/mofact_web.wasm
python3 -m http.server -d web 8000
```
