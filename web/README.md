# Web build

`index.html` is what the playable link serves. Until the Rust app has a web
build, it is the phone prototype from the design phase: a single HTML file
with the core loop (drill, belt, furnace, assembler, five goals).

The Pages workflow (`.github/workflows/pages.yml`) publishes this folder:

- every push to `main` goes to https://grrts.github.io/mofact/
- every pull request gets a preview at https://grrts.github.io/mofact/pr/<number>/, linked in a comment on the PR, and removed when the PR closes

`__MOFACT_BUILD__` is replaced with the commit at deploy time and shown in the in-game menu.
