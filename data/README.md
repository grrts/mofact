# Content data

Items, recipes and buildings live here as RON files that deserialize into the
schema in `crates/core/src/content.rs` (R1: depth is data, not code). The
content lane owns this folder.

| File | Holds |
| --- | --- |
| `items.ron` | Items and fluids: name, icon shape, palette colour, tier |
| `recipes.ron` | Inputs, outputs, craft time and which building category crafts it |
| `buildings.ron` | Size, role (belt, miner, crafter, hub) and build cost |
| `research.ron` | Research: tier, prerequisites, pack cost and what it unlocks |

`crates/content` loads and checks them (unique ids, every reference resolves),
and its `Watcher` reloads them in dev builds when a file changes, so new content
needs no rebuild. CI runs `cargo run -p mofact-balance -- data` on every change.

Tier 1 is the phone prototype's chain (T13) plus stone, bricks and the
survey pack; tier 2 adds coal, steel, motors, the engineering pack and the
fast belt (T26). Anything no research unlocks is open from the start.
`tools/balance` checks it all on every change (T25).
