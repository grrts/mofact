# Web build

`index.html` is what the playable link serves. It is the phone prototype's
page (drawing, touch controls, goals) running on the **real Rust simulation**:
`platform/web` compiles `crates/sim` and the content in `data/` to
`mofact.wasm`, and the page steps it at 60 ticks a second and draws its
snapshots. The page changes the world only through engine commands, the same
ones the native app will send.

When the native renderer (T16) and UI (T21) land, this page is replaced by the
real app compiled for the web.

The Pages workflow (`.github/workflows/pages.yml`) builds the engine and publishes this folder:

- every push to `main` goes to https://grrts.github.io/mofact/
- every pull request gets a preview at https://grrts.github.io/mofact/pr/<number>/, linked in a comment on the PR, and removed when the PR closes

To run it locally:

```sh
cargo build --release -p mofact-web --target wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/release/mofact_web.wasm web/mofact.wasm
python3 -m http.server -d web 8000
```

`__MOFACT_BUILD__` is replaced with the commit at deploy time and shown in the in-game menu.
