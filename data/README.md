# Content data

Items, recipes and buildings live here as RON files that deserialize into the
schema in `crates/core/src/content.rs` (R1: depth is data, not code). The
content lane owns this folder.

| File | Holds |
| --- | --- |
| `items.ron` | Items and fluids: name, icon shape, palette colour, tier |
| `recipes.ron` | Inputs, outputs, craft time and which building category crafts it |
| `buildings.ron` | Size, role (belt, miner, crafter, hub) and build cost |

`crates/content` loads and checks them (unique ids, every reference resolves),
and its `Watcher` reloads them in dev builds when a file changes, so new content
needs no rebuild. CI runs `cargo run -p mofact-balance -- data` on every change.

The current content is the phone prototype's: two ores, five crafted items,
five recipes and five buildings (T13). Balance checks arrive in T25.
